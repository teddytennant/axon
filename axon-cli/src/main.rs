mod agents;
mod chat;
mod config;
mod onboarding;
mod providers;
mod tui;

use axon_core::negotiate::BiddingStrategy;
use axon_core::trust::TaskOutcome;
use axon_core::{
    Capability, DiscoveryEvent, EagerBidder, Identity, McpBridge, McpBridgeAgent, MdnsDiscovery,
    Message, NegotiationState, Negotiator, PeerInfo, PeerTable, PersistentTrustStore, ReceivedBid,
    Runtime, TaskQueue, TaskQueueConfig, TaskStatus, ToolRegistry, Transport, TrustGossipProcessor,
    TrustObservation, TrustScorer, TrustWeightedScoring,
};
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{error, info};
use uuid::Uuid;

use agents::{EchoAgent, LlmAgent, SystemInfoAgent};
use providers::ProviderKind;
use tui::{Dashboard, DashboardState, TaskLogEntry};

/// Atomic counters for node-level metrics.
pub struct NodeMetrics {
    pub tasks_processed: AtomicU64,
    pub tasks_failed: AtomicU64,
    pub messages_received: AtomicU64,
    pub messages_sent: AtomicU64,
    pub started_at: std::time::Instant,
}

impl NodeMetrics {
    fn new() -> Self {
        Self {
            tasks_processed: AtomicU64::new(0),
            tasks_failed: AtomicU64::new(0),
            messages_received: AtomicU64::new(0),
            messages_sent: AtomicU64::new(0),
            started_at: std::time::Instant::now(),
        }
    }

    fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }
}

#[derive(Parser)]
#[command(name = "axon", about = "Decentralized AI Agent Mesh", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a mesh node with TUI dashboard
    Start {
        /// Address to listen on
        #[arg(short, long, default_value = "0.0.0.0:4242")]
        listen: SocketAddr,

        /// Bootstrap peers to connect to (addr:port)
        #[arg(short, long)]
        peer: Vec<SocketAddr>,

        /// Disable TUI (run headless)
        #[arg(long)]
        headless: bool,

        /// LLM provider: ollama, xai, openrouter, custom (use openrouter for Anthropic, OpenAI, Gemini, etc.)
        #[arg(long, default_value = "ollama")]
        provider: ProviderKind,

        /// LLM endpoint URL (defaults per provider)
        #[arg(long, default_value = "")]
        llm_endpoint: String,

        /// API key for the LLM provider (or set env: XAI_API_KEY, OPENROUTER_API_KEY)
        #[arg(long, default_value = "")]
        api_key: String,

        /// Model name (defaults per provider)
        #[arg(long, default_value = "")]
        model: String,

        /// Enable health check TCP endpoint on this port (e.g., 4243)
        #[arg(long)]
        health_port: Option<u16>,

        /// Enable web UI on this port (e.g., 3000) — serves the dashboard at http://localhost:<port>
        #[arg(long)]
        web_port: Option<u16>,
    },

    /// Show node status
    Status,

    /// Send a task to the mesh
    Send {
        /// Target peer address
        #[arg(short, long)]
        peer: SocketAddr,

        /// Capability namespace (e.g., "echo")
        #[arg(short, long)]
        namespace: String,

        /// Capability name (e.g., "ping")
        #[arg(short = 'c', long)]
        name: String,

        /// Payload string
        #[arg(short = 'd', long, default_value = "")]
        data: String,
    },

    /// List known peers
    Peers {
        /// Address of a running node to query
        #[arg(short, long, default_value = "127.0.0.1:4242")]
        node: SocketAddr,
    },

    /// Generate a new identity
    Identity,

    /// Generate example config file at ~/.config/axon/config.toml
    Init,

    /// Serve aggregated MCP tools on stdio (for AI agent integration)
    ///
    /// Connects to configured MCP servers, aggregates their tools, and exposes
    /// them via the MCP protocol on stdio. Configure this as an MCP server in
    /// Claude Code, Cursor, or any MCP-capable AI tool.
    ///
    /// With --mesh, joins the axon mesh and also serves tools from remote peers.
    /// Remote tool calls are forwarded via QUIC to the owning peer.
    ///
    /// Example Claude Code config:
    ///   { "mcpServers": { "axon": { "command": "axon", "args": ["serve-mcp", "--mesh"] } } }
    ServeMcp {
        /// Join the mesh and serve remote tools alongside local ones.
        /// Reads [node] config for listen address and bootstrap peers.
        #[arg(long)]
        mesh: bool,
    },

    /// Query MCP tools available on a mesh node
    Tools {
        /// Address of a running node to query
        #[arg(short, long, default_value = "127.0.0.1:4242")]
        node: SocketAddr,

        /// Search query to filter tools by relevance
        #[arg(short, long)]
        query: Option<String>,

        /// Filter by MCP server name (e.g., "filesystem", "github")
        #[arg(short, long)]
        server: Option<String>,

        /// Maximum number of results
        #[arg(short, long, default_value = "20")]
        limit: u32,

        /// Maximum token budget for results (0 = unlimited)
        #[arg(short = 'b', long, default_value = "0")]
        budget: u32,

        /// Schema detail level: full, summary, or compact
        #[arg(short, long, default_value = "full")]
        detail: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Show trust scores for mesh peers
    Trust {
        #[command(subcommand)]
        action: TrustAction,
    },

    /// Interactive setup wizard — configure provider, API key, and model
    Setup,

    /// Authenticate with an LLM provider (save API key to config)
    Auth {
        /// Provider to authenticate: openrouter, xai, ollama, custom
        provider: ProviderKind,
    },

    /// List available models for the configured (or specified) provider
    Models {
        /// Provider to list models for (defaults to configured provider)
        #[arg(short, long)]
        provider: Option<ProviderKind>,

        /// Filter models by name/description
        #[arg(short, long)]
        filter: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Send a one-shot prompt to the LLM agent and print the response
    Ask {
        /// The prompt to send
        prompt: Vec<String>,

        /// Send to a remote node instead of local
        #[arg(short, long)]
        peer: Option<SocketAddr>,
    },

    /// Interactive chat TUI with the LLM agent — slash commands, model switching, conversation history
    Chat {
        /// Send to a remote node instead of local (uses basic REPL mode)
        #[arg(short, long)]
        peer: Option<SocketAddr>,
    },
}

#[derive(Subcommand)]
enum TrustAction {
    /// Show trust scores for all known peers (or a specific peer)
    Show {
        /// Hex-encoded peer ID to show (omit for all peers)
        #[arg(short, long)]
        peer: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show observation history for a specific peer
    History {
        /// Hex-encoded peer ID
        peer: String,
        /// Maximum observations to show (most recent first)
        #[arg(short, long, default_value = "20")]
        limit: usize,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Simulate trust scoring with example data (for testing/demo)
    Demo,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Start {
            listen,
            peer: bootstrap_peers,
            headless,
            provider,
            llm_endpoint,
            api_key,
            model,
            health_port,
            web_port,
        } => {
            // First-run detection: if no config exists, offer setup
            if !config::config_exists() && !headless && atty::is(atty::Stream::Stdin) {
                eprintln!("\x1b[36m▲ AXON\x1b[0m  No configuration found. Running setup wizard...\n");
                match onboarding::run_onboarding().await {
                    Ok(true) => {
                        eprintln!("\n\x1b[32m✓\x1b[0m Setup complete. Starting node...\n");
                    }
                    Ok(false) => {
                        eprintln!("Setup skipped. Using defaults.");
                    }
                    Err(e) => {
                        eprintln!("Setup error: {}. Using defaults.", e);
                    }
                }
            }

            // Load config file, then overlay CLI flags
            let file_config = config::load_config();

            let effective_listen = if listen.to_string() == "0.0.0.0:4242" {
                // CLI is at default — use config file value if set
                file_config.node.listen
            } else {
                listen
            };

            let effective_headless = headless || file_config.node.headless;

            let mut effective_peers = file_config.node.peers.clone();
            for p in &bootstrap_peers {
                if !effective_peers.contains(p) {
                    effective_peers.push(*p);
                }
            }

            let effective_provider: ProviderKind = if provider.to_string() == "ollama" {
                // CLI at default — try config file
                file_config.llm.provider.parse().unwrap_or(provider)
            } else {
                provider
            };

            if !effective_headless {
                tracing_subscriber::fmt().with_writer(std::io::sink).init();
            } else {
                tracing_subscriber::fmt::init();
            }

            // Resolve provider config: CLI flag > config file > env var > default
            let effective_api_key = if !api_key.is_empty() {
                api_key
            } else if !file_config.llm.api_key.is_empty() {
                file_config.llm.api_key
            } else {
                providers::resolve_api_key("", &effective_provider)
            };

            let effective_model = if !model.is_empty() {
                model
            } else if !file_config.llm.model.is_empty() {
                file_config.llm.model
            } else {
                providers::default_model(&effective_provider).to_string()
            };

            let effective_endpoint = if !llm_endpoint.is_empty() {
                llm_endpoint
            } else if !file_config.llm.endpoint.is_empty() {
                file_config.llm.endpoint
            } else {
                providers::default_endpoint(&effective_provider).to_string()
            };

            let llm_provider = providers::build_provider(
                &effective_provider,
                &effective_endpoint,
                &effective_api_key,
                &effective_model,
            )?;

            let effective_health_port = health_port.or(file_config.node.health_port);
            let effective_web_port = web_port.or(file_config.node.web_port);

            let mcp_configs: Vec<_> = file_config
                .mcp
                .servers
                .iter()
                .map(|s| s.to_server_config())
                .collect();

            run_node(
                effective_listen,
                effective_peers,
                effective_headless,
                llm_provider,
                effective_health_port,
                effective_web_port,
                mcp_configs,
            )
            .await?;
        }
        Commands::Status => {
            println!("axon mesh node status");
            let path = Identity::default_path();
            if path.exists() {
                let id = Identity::load_or_generate(&path)?;
                println!("Peer ID: {}", id.peer_id_hex());
            } else {
                println!("No identity found. Run 'axon start' to create one.");
            }
        }
        Commands::Send {
            peer,
            namespace,
            name,
            data,
        } => {
            tracing_subscriber::fmt::init();
            send_task(peer, &namespace, &name, data.as_bytes()).await?;
        }
        Commands::Peers { node } => {
            tracing_subscriber::fmt::init();
            println!("Querying node at {} for peers...\n", node);

            let identity = Identity::load_or_generate(&Identity::default_path())?;
            let transport = Transport::bind("0.0.0.0:0".parse()?, &identity).await?;
            let conn = transport.connect(node).await?;

            // Ask the remote node for peers that match any capability.
            let discover = Message::Discover {
                capability: Capability::new("*", "*", 0),
            };
            Transport::send(&conn, &discover).await?;

            let resp = Transport::recv(&conn).await?;
            match resp {
                Message::DiscoverResponse { peers } => {
                    if peers.is_empty() {
                        println!("No peers known by this node.");
                    } else {
                        println!(
                            "{:<10} {:<24} {:<30} LAST SEEN",
                            "PEER ID", "ADDRESS", "CAPABILITIES"
                        );
                        println!("{}", "-".repeat(76));
                        for p in &peers {
                            let id_short = short_id(&p.peer_id);
                            let caps = p
                                .capabilities
                                .iter()
                                .map(|c| c.tag())
                                .collect::<Vec<_>>()
                                .join(", ");
                            let ago = {
                                let now = now_secs();
                                let diff = now.saturating_sub(p.last_seen);
                                format!("{}s ago", diff)
                            };
                            println!("{:<10} {:<24} {:<30} {}", id_short, p.addr, caps, ago);
                        }
                        println!("\n{} peer(s) total.", peers.len());
                    }
                }
                other => {
                    println!("Unexpected response from node: {:?}", other);
                }
            }

            transport.shutdown().await;
        }
        Commands::Identity => {
            let path = Identity::default_path();
            let id = Identity::load_or_generate(&path)?;
            println!("Identity file: {}", path.display());
            println!("Peer ID: {}", id.peer_id_hex());
            println!("Short ID: {}", id.peer_id_short());
        }
        Commands::Init => {
            let path = config::generate_example_config()?;
            println!("Config file created at: {}", path.display());
            println!("Edit it to configure your node, then run `axon start`.");
        }
        Commands::ServeMcp { mesh } => {
            // Log to stderr (stdout is the MCP protocol channel)
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .init();

            // Load config
            let file_config = config::load_config();
            let mcp_configs: Vec<axon_core::McpServerConfig> = file_config
                .mcp
                .servers
                .iter()
                .map(|s| s.to_server_config())
                .collect();

            if mcp_configs.is_empty() && !mesh {
                eprintln!("No MCP servers configured.");
                eprintln!(
                    "Add [[mcp.servers]] entries to ~/.config/axon/config.toml or run `axon init`."
                );
                eprintln!("Or use --mesh to serve tools from remote mesh peers.");
                std::process::exit(1);
            }

            // Connect to all configured local MCP servers
            let bridge = Arc::new(McpBridge::new());
            let tools = bridge.connect_all(mcp_configs).await;
            eprintln!(
                "axon-mcp-gateway: {} local tools from {} servers",
                tools.len(),
                bridge.server_count().await
            );

            if mesh {
                // --- Mesh mode: join the mesh and aggregate remote tools ---
                let identity =
                    axon_core::Identity::load_or_generate(&axon_core::Identity::default_path())?;
                let local_peer_id = identity.public_key_bytes();
                eprintln!(
                    "axon-mcp-mesh-gateway: peer ID {}",
                    identity.peer_id_short()
                );

                let listen_addr = file_config.node.listen;
                let transport = axon_core::Transport::bind(listen_addr, &identity).await?;
                let local_addr = transport.local_addr()?;
                eprintln!("axon-mcp-mesh-gateway: listening on {}", local_addr);

                let transport = Arc::new(transport);
                let local_peer = axon_core::PeerInfo {
                    peer_id: local_peer_id.clone(),
                    addr: local_addr.to_string(),
                    capabilities: bridge.capabilities().await,
                    last_seen: now_secs(),
                };
                let peer_table = Arc::new(RwLock::new(axon_core::PeerTable::new(local_peer)));
                let tool_registry = Arc::new(RwLock::new(axon_core::ToolRegistry::new()));

                // Register local tools in the registry
                {
                    let mcp_tools = bridge.all_tools().await;
                    if !mcp_tools.is_empty() {
                        let mut reg = tool_registry.write().await;
                        reg.register_peer_tools(&local_peer_id, mcp_tools);
                    }
                }

                let active_connections: Arc<RwLock<Vec<(String, quinn::Connection)>>> =
                    Arc::new(RwLock::new(Vec::new()));

                // Spawn connection acceptor
                let accept_transport = transport.clone();
                let accept_peer_table = peer_table.clone();
                let accept_tool_registry = tool_registry.clone();
                tokio::spawn(async move {
                    loop {
                        let conn = match accept_transport.accept().await {
                            Some(c) => c,
                            None => continue,
                        };
                        let pt = accept_peer_table.clone();
                        let tr = accept_tool_registry.clone();
                        let remote = conn.remote_address();

                        tokio::spawn(async move {
                            while let Ok(msg) = Transport::recv(&conn).await {
                                handle_mesh_message(msg, &conn, &pt, &tr, remote).await;
                            }
                        });
                    }
                });

                // Connect to bootstrap peers
                let bootstrap_tools = bridge.all_tools().await;
                for peer_addr in &file_config.node.peers {
                    let t = transport.clone();
                    let addr = *peer_addr;
                    let id = local_peer_id.clone();
                    let caps = bridge.capabilities().await;
                    let la = local_addr.to_string();
                    let tools_clone = bootstrap_tools.clone();
                    let conns = active_connections.clone();

                    tokio::spawn(async move {
                        match t.connect(addr).await {
                            Ok(conn) => {
                                // Announce ourselves
                                let announce = Message::Announce(axon_core::PeerInfo {
                                    peer_id: id.clone(),
                                    addr: la,
                                    capabilities: caps,
                                    last_seen: now_secs(),
                                });
                                let _ = Transport::send(&conn, &announce).await;
                                // Send our tool catalog
                                axon_core::gossip::send_tool_catalog(&conn, &id, &tools_clone)
                                    .await;
                                eprintln!("axon-mcp-mesh-gateway: connected to peer at {}", addr);
                                {
                                    let mut c = conns.write().await;
                                    c.push((addr.to_string(), conn));
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "axon-mcp-mesh-gateway: failed to connect to {}: {}",
                                    addr, e
                                );
                            }
                        }
                    });
                }

                // Start gossip loop
                let local_mcp_tools = bridge.all_tools().await;
                let local_catalog = if local_mcp_tools.is_empty() {
                    None
                } else {
                    Some(axon_core::LocalToolCatalog {
                        peer_id: local_peer_id.clone(),
                        tools: local_mcp_tools,
                    })
                };
                let gossip_pt = peer_table.clone();
                let gossip_transport = transport.clone();
                let gossip_conns = active_connections.clone();
                let gossip_lpid = local_peer_id.clone();
                tokio::spawn(async move {
                    axon_core::gossip::run_gossip(
                        gossip_pt,
                        gossip_transport,
                        gossip_conns,
                        axon_core::GossipConfig::default(),
                        local_catalog,
                        None, // no trust store in mesh gateway mode
                        gossip_lpid,
                    )
                    .await;
                });

                // Brief wait for initial tool catalogs to arrive via gossip
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                {
                    let reg = tool_registry.read().await;
                    let remote_count = reg.remote_unique_tools(&local_peer_id).len();
                    eprintln!(
                        "axon-mcp-mesh-gateway: {} remote tools discovered from mesh",
                        remote_count,
                    );
                }

                // Build mesh-connected MCP server
                let mesh_context = axon_core::MeshContext {
                    transport: transport.clone(),
                    tool_registry,
                    peer_table,
                    local_peer_id,
                };
                let server =
                    axon_core::McpStdioServer::new_with_mesh(bridge.clone(), mesh_context).await;
                eprintln!("axon-mcp-mesh-gateway: ready (tools update dynamically via gossip)");

                if let Err(e) = server.run().await {
                    error!("MCP mesh server error: {}", e);
                }

                transport.shutdown().await;
            } else {
                // --- Local-only mode (existing behavior) ---
                let server = axon_core::McpStdioServer::new(bridge.clone()).await;
                if let Err(e) = server.run().await {
                    error!("MCP server error: {}", e);
                }
            }

            bridge.shutdown().await;
        }
        Commands::Tools {
            node,
            query,
            server,
            limit,
            budget,
            detail,
            json,
        } => {
            tracing_subscriber::fmt::init();
            let detail_byte = match detail.as_str() {
                "summary" | "s" => 1u8,
                "compact" | "c" => 2u8,
                _ => 0u8, // "full" or anything else
            };
            query_tools(node, query, server, limit, budget, detail_byte, json).await?;
        }
        Commands::Trust { action } => {
            handle_trust_command(action);
        }
        Commands::Setup => {
            match onboarding::run_onboarding().await {
                Ok(true) => {
                    println!("\n\x1b[32m✓\x1b[0m Configuration saved. Run \x1b[36maxon start\x1b[0m to launch your node.");
                }
                Ok(false) => {
                    println!("\nSetup cancelled.");
                }
                Err(e) => {
                    eprintln!("Setup error: {}", e);
                }
            }
        }
        Commands::Auth { provider } => {
            match onboarding::run_auth(&provider).await {
                Ok(true) => {
                    println!(
                        "\n\x1b[32m✓\x1b[0m {} configuration saved.",
                        provider
                    );
                }
                Ok(false) => {
                    println!("\nAuth cancelled.");
                }
                Err(e) => {
                    eprintln!("Auth error: {}", e);
                }
            }
        }
        Commands::Models {
            provider,
            filter,
            json,
        } => {
            let file_config = config::load_config();
            let kind = provider.unwrap_or_else(|| {
                file_config
                    .llm
                    .provider
                    .parse()
                    .unwrap_or(ProviderKind::Ollama)
            });

            let endpoint = if file_config.llm.endpoint.is_empty() {
                providers::default_endpoint(&kind).to_string()
            } else {
                file_config.llm.endpoint.clone()
            };

            let api_key = if file_config.llm.api_key.is_empty() {
                providers::resolve_api_key("", &kind)
            } else {
                file_config.llm.api_key.clone()
            };

            eprintln!("Fetching models for {}...", kind);
            match providers::fetch_models(&kind, &endpoint, &api_key).await {
                Ok(models) => {
                    let models: Vec<_> = if let Some(ref q) = filter {
                        let q = q.to_lowercase();
                        models
                            .into_iter()
                            .filter(|m| {
                                m.id.to_lowercase().contains(&q)
                                    || m.name.to_lowercase().contains(&q)
                            })
                            .collect()
                    } else {
                        models
                    };

                    if json {
                        println!("{}", serde_json::to_string_pretty(&models).unwrap_or_default());
                    } else {
                        if models.is_empty() {
                            println!("No models found.");
                        } else {
                            println!(
                                "\n{:<45} {:<30} {:>10}",
                                "MODEL ID", "NAME", "CONTEXT"
                            );
                            println!("{}", "─".repeat(87));
                            for m in &models {
                                let ctx = m
                                    .context_length
                                    .map(|c| {
                                        if c >= 1_000_000 {
                                            format!("{}M", c / 1_000_000)
                                        } else {
                                            format!("{}K", c / 1_000)
                                        }
                                    })
                                    .unwrap_or_else(|| "—".into());
                                println!("{:<45} {:<30} {:>10}", m.id, m.name, ctx);
                            }
                            println!("\n{} models total.", models.len());
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to fetch models: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Ask { prompt, peer } => {
            let prompt_text = prompt.join(" ");
            if prompt_text.is_empty() {
                eprintln!("Usage: axon ask <prompt>");
                std::process::exit(1);
            }

            if let Some(peer_addr) = peer {
                // Remote mode: send to a running node
                tracing_subscriber::fmt::init();
                send_task(peer_addr, "llm", "chat", prompt_text.as_bytes()).await?;
            } else {
                // Local mode: call the LLM provider directly
                let file_config = config::load_config();
                let kind: ProviderKind = file_config
                    .llm
                    .provider
                    .parse()
                    .unwrap_or(ProviderKind::Ollama);

                let api_key = if file_config.llm.api_key.is_empty() {
                    providers::resolve_api_key("", &kind)
                } else {
                    file_config.llm.api_key.clone()
                };

                let endpoint = if file_config.llm.endpoint.is_empty() {
                    providers::default_endpoint(&kind).to_string()
                } else {
                    file_config.llm.endpoint.clone()
                };

                let model = if file_config.llm.model.is_empty() {
                    providers::default_model(&kind).to_string()
                } else {
                    file_config.llm.model.clone()
                };

                let llm = providers::build_provider(&kind, &endpoint, &api_key, &model)?;

                eprintln!(
                    "\x1b[36m▲\x1b[0m {} · {}\n",
                    kind, model
                );

                let resp = llm
                    .complete(providers::CompletionRequest {
                        prompt: prompt_text,
                        max_tokens: None,
                        temperature: None,
                    })
                    .await?;

                println!("{}", resp.text);

                if let Some(usage) = resp.usage {
                    eprintln!(
                        "\n\x1b[2m({} prompt + {} completion tokens)\x1b[0m",
                        usage.prompt_tokens, usage.completion_tokens
                    );
                }
            }
        }
        Commands::Chat { peer } => {
            if let Some(peer_addr) = peer {
                // Remote chat: relay through a running node (basic REPL)
                run_chat_remote(peer_addr).await?;
            } else {
                // Local chat: full TUI experience
                chat::run_chat().await?;
            }
        }
    }

    Ok(())
}

fn trust_store_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".axon")
        .join("trust")
}

fn handle_trust_command(action: TrustAction) {
    use axon_core::{PersistentTrustStore, TaskOutcome, TrustObservation, TrustScorer, TrustStore};

    match action {
        TrustAction::Show { peer, json } => {
            let path = trust_store_path();
            match PersistentTrustStore::open(&path, TrustScorer::default()) {
                Ok(store) => {
                    if let Some(peer_hex) = peer {
                        let peer_bytes = hex::decode(&peer_hex).unwrap_or_default();
                        let score = store.score(&peer_bytes);
                        if json {
                            println!(
                                "{{\"peer\":\"{}\",\"overall\":{:.3},\"reliability\":{:.3},\"accuracy\":{:.3},\"availability\":{:.3},\"quality\":{:.3},\"confidence\":{:.3},\"observations\":{}}}",
                                peer_hex, score.overall, score.reliability, score.accuracy,
                                score.availability, score.quality, score.confidence, score.observation_count,
                            );
                        } else {
                            println!("Peer: {}", peer_hex);
                            print_trust_score(&score);
                        }
                    } else {
                        // Show all peers
                        let ranked = store.ranked_peers();
                        if ranked.is_empty() {
                            println!("No peer trust records. Interact with peers to build trust.");
                        } else {
                            println!(
                                "{:<16} {:>8} {:>8} {:>8} {:>8} {:>6}",
                                "PEER", "OVERALL", "RELIAB", "ACCUR", "AVAIL", "OBS"
                            );
                            for (id, score) in &ranked {
                                let hex: String =
                                    id.iter().take(8).map(|b| format!("{:02x}", b)).collect();
                                println!(
                                    "{:<16} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>6}",
                                    hex,
                                    score.overall,
                                    score.reliability,
                                    score.accuracy,
                                    score.availability,
                                    score.observation_count,
                                );
                            }
                            println!("\n{} peers tracked", ranked.len());
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to open trust store at {}: {}", path.display(), e);
                }
            }
        }
        TrustAction::History { peer, limit, json } => {
            let path = trust_store_path();
            match PersistentTrustStore::open(&path, TrustScorer::default()) {
                Ok(store) => {
                    let peer_bytes = hex::decode(&peer).unwrap_or_default();
                    match store.get_record(&peer_bytes) {
                        Some(record) => {
                            let observations: Vec<_> =
                                record.observations.iter().rev().take(limit).collect();
                            if json {
                                let obs_json: Vec<String> = observations
                                    .iter()
                                    .map(|o| {
                                        format!(
                                            "{{\"timestamp\":{},\"outcome\":\"{:?}\",\"estimated_ms\":{},\"actual_ms\":{}}}",
                                            o.timestamp, o.outcome, o.estimated_latency_ms, o.actual_latency_ms
                                        )
                                    })
                                    .collect();
                                println!(
                                    "{{\"peer\":\"{}\",\"total\":{},\"observations\":[{}]}}",
                                    peer,
                                    record.observation_count(),
                                    obs_json.join(",")
                                );
                            } else {
                                println!(
                                    "Peer: {} — {} observations (showing last {})",
                                    peer,
                                    record.observation_count(),
                                    observations.len()
                                );
                                for obs in &observations {
                                    println!(
                                        "  [{:>10}] {:?} — est {}ms, actual {}ms",
                                        obs.timestamp,
                                        obs.outcome,
                                        obs.estimated_latency_ms,
                                        obs.actual_latency_ms
                                    );
                                }
                            }
                        }
                        None => {
                            if json {
                                println!(
                                    "{{\"peer\":\"{}\",\"total\":0,\"observations\":[]}}",
                                    peer
                                );
                            } else {
                                println!("Peer: {} — no observations", peer);
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to open trust store: {}", e);
                }
            }
        }
        TrustAction::Demo => {
            println!("=== axon trust system demo ===\n");

            let scorer = TrustScorer::new();
            let mut store = TrustStore::new(scorer);

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            // Simulate 3 peers with different behavior patterns
            let reliable_peer = vec![0xAA, 0xBB, 0xCC];
            let flaky_peer = vec![0xDD, 0xEE, 0xFF];
            let liar_peer = vec![0x11, 0x22, 0x33];

            println!("Simulating 3 peers over 100 interactions:\n");
            println!("  Peer AABBCC — reliable (95% success, accurate estimates)");
            println!("  Peer DDEEFF — flaky (60% success, 20% timeouts)");
            println!("  Peer 112233 — liar (90% success, but estimates 100ms, takes 2000ms)\n");

            // Reliable peer: 95/100 success, accurate latency
            for i in 0..100 {
                let outcome = if i % 20 == 17 {
                    TaskOutcome::Failure
                } else {
                    TaskOutcome::Success
                };
                store.record_observation(
                    &reliable_peer,
                    TrustObservation::new(outcome, 100, 110).with_timestamp(now - 100 + i),
                );
            }

            // Flaky peer: 60% success, 20% timeout, 20% failure
            for i in 0..100 {
                let outcome = if i % 5 == 0 {
                    TaskOutcome::Timeout
                } else if i % 5 == 1 {
                    TaskOutcome::Failure
                } else {
                    TaskOutcome::Success
                };
                let actual = if outcome == TaskOutcome::Timeout {
                    0
                } else {
                    200
                };
                store.record_observation(
                    &flaky_peer,
                    TrustObservation::new(outcome, 150, actual).with_timestamp(now - 100 + i),
                );
            }

            // Liar peer: high success but wildly inaccurate estimates
            for i in 0..100 {
                store.record_observation(
                    &liar_peer,
                    TrustObservation::new(TaskOutcome::Success, 100, 2000)
                        .with_timestamp(now - 100 + i),
                );
            }

            let ranked = store.ranked_peers_at(now);
            println!("Trust scores (ranked):\n");
            for (id, score) in &ranked {
                let hex: String = id.iter().map(|b| format!("{:02X}", b)).collect();
                print!("  Peer {} ", hex);
                print_trust_score(score);
                println!();
            }

            // Demonstrate trust-weighted bid scoring
            println!("--- Trust-weighted negotiation demo ---\n");
            println!("All 3 peers bid identically: 50ms latency, 0.2 load, 0.9 confidence\n");

            let tws = axon_core::TrustWeightedScoring::new(0.5);
            let negotiator = axon_core::Negotiator::new(
                std::time::Duration::from_millis(500),
                axon_core::BidScoring::default(),
            );

            for (id, trust) in &ranked {
                let hex: String = id.iter().map(|b| format!("{:02X}", b)).collect();
                let bid =
                    axon_core::ReceivedBid::new(uuid::Uuid::new_v4(), id.clone(), 50, 0.2, 0.9);
                let raw = negotiator.score_bid(&bid);
                let weighted = tws.weighted_score(raw, trust);
                println!(
                    "  Peer {} — raw: {:.3}, trust-weighted: {:.3}",
                    hex, raw, weighted
                );
            }
            println!("\nThe reliable peer wins despite identical bids. Trust breaks ties.");
        }
    }
}

fn print_trust_score(score: &axon_core::TrustScore) {
    println!(
        "[overall: {:.3}] reliability: {:.3}, accuracy: {:.3}, availability: {:.3}, quality: {:.3} (confidence: {:.3}, obs: {})",
        score.overall,
        score.reliability,
        score.accuracy,
        score.availability,
        score.quality,
        score.confidence,
        score.observation_count,
    );
}

async fn run_node(
    listen: SocketAddr,
    bootstrap_peers: Vec<SocketAddr>,
    headless: bool,
    llm_provider: Box<dyn providers::LlmProvider>,
    health_port: Option<u16>,
    web_port: Option<u16>,
    mcp_configs: Vec<axon_core::McpServerConfig>,
) -> anyhow::Result<()> {
    let identity = Identity::load_or_generate(&Identity::default_path())?;
    let peer_id_hex = identity.peer_id_hex();
    info!("Starting axon node with Peer ID: {}", peer_id_hex);

    // Open persistent task queue
    let queue_path = Identity::default_path()
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("queue");
    let task_queue = Arc::new(
        TaskQueue::open(&queue_path, TaskQueueConfig::default())
            .expect("Failed to open task queue"),
    );
    let recovered = task_queue.recover().unwrap_or(0);
    if recovered > 0 {
        info!("Recovered {} tasks from previous session", recovered);
    }

    // Open persistent trust store
    let trust_path = Identity::default_path()
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("trust");
    let trust_store = Arc::new(Mutex::new(
        PersistentTrustStore::open(&trust_path, TrustScorer::default())
            .expect("Failed to open trust store"),
    ));
    {
        let ts = trust_store.lock().await;
        let peer_count = ts.peer_count();
        if peer_count > 0 {
            info!("Loaded trust records for {} peers", peer_count);
        }
    }

    // Initialize negotiation state
    let negotiation_state = Arc::new(Mutex::new(NegotiationState::new()));
    let negotiator = Arc::new(Negotiator::new(
        std::time::Duration::from_millis(2000),
        axon_core::BidScoring::default(),
    ));
    let trust_scoring = Arc::new(TrustWeightedScoring::default());
    // Track requester connections for async negotiation responses
    let negotiation_requesters: Arc<Mutex<HashMap<Uuid, quinn::Connection>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let transport = Transport::bind(listen, &identity).await?;
    let local_addr = transport.local_addr()?;
    info!("Listening on {}", local_addr);

    let runtime = Arc::new(Runtime::new());
    runtime.register(Arc::new(EchoAgent)).await;
    runtime.register(Arc::new(SystemInfoAgent)).await;
    runtime
        .register(Arc::new(LlmAgent::new(Arc::from(llm_provider))))
        .await;

    // Connect to MCP servers and register the bridge agent
    let mcp_bridge = Arc::new(McpBridge::new());
    if !mcp_configs.is_empty() {
        info!("Connecting to {} MCP server(s)...", mcp_configs.len());
        let tools = mcp_bridge.connect_all(mcp_configs).await;
        if !tools.is_empty() {
            info!(
                "MCP bridge: {} tools discovered across {} server(s)",
                tools.len(),
                mcp_bridge.server_count().await
            );
        }
        runtime
            .register(Arc::new(McpBridgeAgent::new(mcp_bridge.clone())))
            .await;
    }

    let all_caps = runtime.all_capabilities().await;
    let bidder_queue_depth = Arc::new(AtomicU64::new(0));
    let bidder = Arc::new(EagerBidder::new(
        identity.public_key_bytes(),
        bidder_queue_depth,
        100, // max queue depth
        all_caps.clone(),
        50, // base latency ms
    ));

    let local_peer = PeerInfo {
        peer_id: identity.public_key_bytes(),
        addr: local_addr.to_string(),
        capabilities: all_caps,
        last_seen: now_secs(),
    };

    let peer_table = Arc::new(RwLock::new(PeerTable::new(local_peer)));
    let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));
    let metrics = Arc::new(NodeMetrics::new());

    let dashboard_state = Arc::new(RwLock::new(DashboardState::new(
        peer_id_hex.clone(),
        local_addr.to_string(),
    )));

    {
        let mut state = dashboard_state.write().await;
        state.agent_names = runtime.agent_names().await;
        state.capabilities = runtime.all_capabilities().await;
        state.add_log(format!("Node started: {}", peer_id_hex));
        state.add_log(format!("Listening on {}", local_addr));
        if recovered > 0 {
            state.add_log(format!(
                "Recovered {} tasks from previous session",
                recovered
            ));
        }
        let mcp_tool_count = mcp_bridge.all_tools().await.len();
        let mcp_server_count = mcp_bridge.server_count().await;
        if mcp_tool_count > 0 {
            state.add_log(format!(
                "MCP bridge: {} tools from {} server(s)",
                mcp_tool_count,
                mcp_server_count,
            ));
        }

        // Populate settings tab
        let settings_config = config::load_config();
        state.provider_name = settings_config.llm.provider.clone();
        state.model_name = if settings_config.llm.model.is_empty() {
            providers::default_model(
                &settings_config
                    .llm
                    .provider
                    .parse()
                    .unwrap_or(ProviderKind::Ollama),
            )
            .to_string()
        } else {
            settings_config.llm.model.clone()
        };
        state.config_path = config::config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        state.mcp_server_count = mcp_server_count;
        state.mcp_tool_count = mcp_tool_count;
    }

    // Register MCP tools in the local tool registry
    {
        let mcp_tools = mcp_bridge.all_tools().await;
        if !mcp_tools.is_empty() {
            let mut reg = tool_registry.write().await;
            reg.register_peer_tools(&identity.public_key_bytes(), mcp_tools);
        }
    }

    // Shutdown coordination: a broadcast channel that all tasks listen to.
    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

    let transport = Arc::new(transport);
    let active_connections: Arc<RwLock<Vec<(String, quinn::Connection)>>> =
        Arc::new(RwLock::new(Vec::new()));

    // --- Web UI ---
    let web_state = Arc::new(RwLock::new(axon_web::WebState::new(
        peer_id_hex.clone(),
        local_addr.to_string(),
    )));
    {
        let mut ws = web_state.write().await;
        let web_cfg = config::load_config();
        ws.provider_name = web_cfg.llm.provider.clone();
        ws.model_name = if web_cfg.llm.model.is_empty() {
            providers::default_model(
                &web_cfg.llm.provider.parse().unwrap_or(ProviderKind::Ollama),
            )
            .to_string()
        } else {
            web_cfg.llm.model.clone()
        };
        ws.config_path = config::config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
    }
    if let Some(wp) = web_port {
        let shared_web = Arc::new(axon_web::SharedWebState {
            peer_table: peer_table.clone(),
            tool_registry: tool_registry.clone(),
            trust_store: trust_store.clone(),
            task_queue: task_queue.clone(),
            runtime: runtime.clone(),
            mcp_bridge: mcp_bridge.clone(),
            local_peer_id: identity.public_key_bytes(),
            web_state: web_state.clone(),
        });
        tokio::spawn(async move {
            axon_web::start_web_server(shared_web, wp).await;
        });
        dashboard_state.write().await.add_log(format!(
            "Web UI available at http://localhost:{}",
            wp
        ));
    }

    // Spawn connection acceptor
    let accept_transport = transport.clone();
    let accept_runtime = runtime.clone();
    let accept_peer_table = peer_table.clone();
    let accept_dashboard = dashboard_state.clone();
    let accept_conns = active_connections.clone();
    let accept_metrics = metrics.clone();
    let accept_queue = task_queue.clone();
    let accept_fwd_transport = transport.clone();
    let accept_tool_registry = tool_registry.clone();
    let accept_trust_store = trust_store.clone();
    let accept_neg_state = negotiation_state.clone();
    let accept_negotiator = negotiator.clone();
    let accept_trust_scoring = trust_scoring.clone();
    let accept_bidder = bidder.clone();
    let accept_local_peer_id = identity.public_key_bytes();
    let accept_neg_requesters = negotiation_requesters.clone();
    let mut accept_shutdown = shutdown_tx.subscribe();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                maybe_conn = accept_transport.accept() => {
                    let conn = match maybe_conn {
                        Some(c) => c,
                        None => continue,
                    };
                    let rt = accept_runtime.clone();
                    let pt = accept_peer_table.clone();
                    let ds = accept_dashboard.clone();
                    let m = accept_metrics.clone();
                    let tq = accept_queue.clone();
                    let fwd_t = accept_fwd_transport.clone();
                    let tr = accept_tool_registry.clone();
                    let ts = accept_trust_store.clone();
                    let ns = accept_neg_state.clone();
                    let neg = accept_negotiator.clone();
                    let tws = accept_trust_scoring.clone();
                    let bid = accept_bidder.clone();
                    let lpid = accept_local_peer_id.clone();
                    let nr = accept_neg_requesters.clone();
                    let remote = conn.remote_address();

                    {
                        let mut conns = accept_conns.write().await;
                        conns.push((remote.to_string(), conn.clone()));
                    }

                    tokio::spawn(async move {
                        loop {
                            match Transport::recv(&conn).await {
                                Ok(msg) => {
                                    m.messages_received.fetch_add(1, Ordering::Relaxed);
                                    handle_message(msg, &conn, &rt, &pt, &ds, remote, &m, &tq, &fwd_t, &tr, &ts, &ns, &neg, &tws, &bid, &lpid, &nr).await;
                                }
                                Err(e) => {
                                    info!("Connection from {} closed: {}", remote, e);
                                    let mut table = pt.write().await;
                                    let peers_snapshot: Vec<_> = table.all_peers_owned();
                                    for peer in &peers_snapshot {
                                        if peer.addr == remote.to_string() {
                                            table.remove(&peer.peer_id);
                                            let mut state = ds.write().await;
                                            state.add_log(format!(
                                                "Removed disconnected peer {} at {}",
                                                short_id(&peer.peer_id),
                                                peer.addr
                                            ));
                                            break;
                                        }
                                    }
                                    break;
                                }
                            }
                        }
                    });
                }
                _ = accept_shutdown.recv() => {
                    info!("Acceptor shutting down");
                    break;
                }
            }
        }
    });

    // Connect to bootstrap peers
    let bootstrap_tools = mcp_bridge.all_tools().await;
    for peer_addr in &bootstrap_peers {
        let t = transport.clone();
        let addr = *peer_addr;
        let ds = dashboard_state.clone();
        let id = identity.public_key_bytes();
        let caps = runtime.all_capabilities().await;
        let la = local_addr.to_string();
        let tools = bootstrap_tools.clone();

        tokio::spawn(async move {
            match t.connect(addr).await {
                Ok(conn) => {
                    let announce = Message::Announce(PeerInfo {
                        peer_id: id.clone(),
                        addr: la,
                        capabilities: caps,
                        last_seen: now_secs(),
                    });
                    if let Err(e) = Transport::send(&conn, &announce).await {
                        error!("Failed to announce to {}: {}", addr, e);
                    } else {
                        // Send our ToolCatalog so the peer discovers our MCP tools
                        axon_core::gossip::send_tool_catalog(&conn, &id, &tools).await;
                        let mut state = ds.write().await;
                        state.add_log(format!("Connected to bootstrap peer: {}", addr));
                    }
                }
                Err(e) => {
                    error!("Failed to connect to bootstrap peer {}: {}", addr, e);
                    let mut state = ds.write().await;
                    state.add_log(format!("Failed to connect to {}: {}", addr, e));
                }
            }
        });
    }

    // Start mDNS discovery
    let all_caps = runtime.all_capabilities().await;
    let mdns_result = MdnsDiscovery::new(peer_id_hex.clone(), local_addr.port(), all_caps);

    let mdns_tools = mcp_bridge.all_tools().await;
    let _mdns = if let Ok((mdns, mut mdns_rx)) = mdns_result {
        let mdns_pt = peer_table.clone();
        let mdns_ds = dashboard_state.clone();
        let mdns_transport = transport.clone();
        let mdns_id = identity.public_key_bytes();
        let mdns_caps = runtime.all_capabilities().await;
        let mdns_la = local_addr.to_string();
        let mdns_tools = mdns_tools.clone();

        tokio::spawn(async move {
            while let Some(event) = mdns_rx.recv().await {
                match event {
                    DiscoveryEvent::PeerDiscovered(info) => {
                        let addr_str = info.addr.clone();
                        let mut pt = mdns_pt.write().await;
                        let is_new = pt.upsert(info);
                        drop(pt);

                        if is_new {
                            let mut state = mdns_ds.write().await;
                            state.add_log(format!("mDNS: discovered peer at {}", addr_str));
                            drop(state);

                            if let Ok(addr) = addr_str.parse::<SocketAddr>() {
                                match mdns_transport.connect(addr).await {
                                    Ok(conn) => {
                                        let announce = Message::Announce(PeerInfo {
                                            peer_id: mdns_id.clone(),
                                            addr: mdns_la.clone(),
                                            capabilities: mdns_caps.clone(),
                                            last_seen: now_secs(),
                                        });
                                        let _ = Transport::send(&conn, &announce).await;
                                        // Share our MCP tools with the new peer
                                        axon_core::gossip::send_tool_catalog(
                                            &conn,
                                            &mdns_id,
                                            &mdns_tools,
                                        )
                                        .await;
                                    }
                                    Err(e) => {
                                        let mut state = mdns_ds.write().await;
                                        state.add_log(format!(
                                            "mDNS: failed to connect to {}: {}",
                                            addr, e
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    DiscoveryEvent::PeerRemoved(id) => {
                        let mut pt = mdns_pt.write().await;
                        pt.remove(&id);
                    }
                }
            }
        });

        {
            let mut state = dashboard_state.write().await;
            state.add_log("mDNS discovery started".to_string());
        }
        Some(mdns)
    } else {
        {
            let mut state = dashboard_state.write().await;
            state.add_log("mDNS discovery failed to start (non-fatal)".to_string());
        }
        None
    };

    // Build local tool catalog for gossip propagation
    let local_mcp_tools = mcp_bridge.all_tools().await;
    let local_catalog = if local_mcp_tools.is_empty() {
        None
    } else {
        Some(axon_core::LocalToolCatalog {
            peer_id: identity.public_key_bytes(),
            tools: local_mcp_tools.clone(),
        })
    };

    // Broadcast ToolCatalog to all currently connected peers
    if !local_mcp_tools.is_empty() {
        axon_core::broadcast_tool_catalog(
            &active_connections,
            &identity.public_key_bytes(),
            &local_mcp_tools,
        )
        .await;
        let mut state = dashboard_state.write().await;
        state.add_log(format!(
            "Broadcast {} MCP tools to connected peers",
            local_mcp_tools.len()
        ));
    }

    // Spawn gossip protocol
    let gossip_pt = peer_table.clone();
    let gossip_transport = transport.clone();
    let gossip_conns = active_connections.clone();
    let gossip_trust = trust_store.clone();
    let gossip_peer_id = identity.public_key_bytes();
    tokio::spawn(async move {
        axon_core::gossip::run_gossip(
            gossip_pt,
            gossip_transport,
            gossip_conns,
            axon_core::GossipConfig::default(),
            local_catalog,
            Some(gossip_trust),
            gossip_peer_id,
        )
        .await;
    });

    // Spawn periodic peer table sync to dashboard + connection cleanup + metrics
    let sync_pt = peer_table.clone();
    let sync_ds = dashboard_state.clone();
    let sync_conns = active_connections.clone();
    let sync_metrics = metrics.clone();
    let sync_web = web_state.clone();
    let sync_trust = trust_store.clone();
    let mut sync_shutdown = shutdown_tx.subscribe();
    let mut sync_last_tasks: u64 = 0;
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(1)) => {
                    let table = sync_pt.read().await;
                    let peers = table.all_peers_owned();
                    drop(table);

                    let tasks_now = sync_metrics.tasks_processed.load(Ordering::Relaxed);
                    let tasks_delta = tasks_now.saturating_sub(sync_last_tasks);
                    sync_last_tasks = tasks_now;

                    let tasks_failed = sync_metrics.tasks_failed.load(Ordering::Relaxed);
                    let msgs_in = sync_metrics.messages_received.load(Ordering::Relaxed);
                    let msgs_out = sync_metrics.messages_sent.load(Ordering::Relaxed);

                    // Snapshot trust scores outside the dashboard write lock.
                    let trust_scores: Vec<(String, f64)> = {
                        let ts = sync_trust.lock().await;
                        ts.ranked_peers()
                            .into_iter()
                            .map(|(id, score)| {
                                let hex: String =
                                    id.iter().map(|b| format!("{:02x}", b)).collect();
                                (hex, score.overall)
                            })
                            .collect()
                    };

                    let mut state = sync_ds.write().await;
                    state.peers = peers;
                    state.uptime_secs = sync_metrics.uptime_secs();
                    state.tasks_total = tasks_now;
                    state.tasks_failed = tasks_failed;

                    // Populate CRDT counter view from node metrics.
                    state.crdt_counters = vec![
                        ("tasks.processed".to_string(), tasks_now),
                        ("tasks.failed".to_string(), tasks_failed),
                        ("messages.received".to_string(), msgs_in),
                        ("messages.sent".to_string(), msgs_out),
                    ];

                    // Populate peer trust scores.
                    state.peer_trust = trust_scores;

                    // Update throughput history (tasks/sec, capped at 60 samples).
                    state.throughput_history.push_back(tasks_delta);
                    if state.throughput_history.len() > 60 {
                        state.throughput_history.pop_front();
                    }

                    drop(state);

                    // Sync web state
                    {
                        let mut ws = sync_web.write().await;
                        ws.uptime_secs = sync_metrics.uptime_secs();
                        ws.tasks_total = sync_metrics.tasks_processed.load(Ordering::Relaxed);
                        ws.tasks_failed = sync_metrics.tasks_failed.load(Ordering::Relaxed);
                        ws.messages_received = sync_metrics.messages_received.load(Ordering::Relaxed);
                        ws.messages_sent = sync_metrics.messages_sent.load(Ordering::Relaxed);
                    }

                    // Prune closed connections to prevent memory leaks.
                    let mut conns = sync_conns.write().await;
                    conns.retain(|(_, conn)| conn.close_reason().is_none());
                }
                _ = sync_shutdown.recv() => {
                    break;
                }
            }
        }
    });

    // Spawn background queue drain worker.
    // Periodically dequeues pending tasks (from retries or crash recovery)
    // and dispatches them through the runtime. Also runs cleanup.
    let drain_queue = task_queue.clone();
    let drain_runtime = runtime.clone();
    let drain_metrics = metrics.clone();
    let drain_dashboard = dashboard_state.clone();
    let mut drain_shutdown = shutdown_tx.subscribe();
    tokio::spawn(async move {
        let mut cleanup_counter: u32 = 0;
        loop {
            tokio::select! {
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(2)) => {
                    // Drain all pending tasks in this tick
                    loop {
                        match drain_queue.dequeue() {
                            Ok(Some(record)) => {
                                let task_id = record.request.id;
                                let cap_tag = record.request.capability.tag();
                                let attempt = record.attempts;

                                info!(
                                    "Drain worker: dispatching task {} ({}) attempt {}",
                                    task_id, cap_tag, attempt
                                );

                                let resp = drain_runtime.dispatch(record.request).await;

                                match &resp.status {
                                    TaskStatus::Success => {
                                        drain_metrics.tasks_processed.fetch_add(1, Ordering::Relaxed);
                                        let _ = drain_queue.complete(task_id, resp.clone());
                                        info!("Drain worker: task {} completed", task_id);
                                    }
                                    TaskStatus::Error(e) => {
                                        drain_metrics.tasks_processed.fetch_add(1, Ordering::Relaxed);
                                        drain_metrics.tasks_failed.fetch_add(1, Ordering::Relaxed);
                                        let retried = drain_queue.fail(task_id, e.clone()).unwrap_or(false);
                                        if retried {
                                            info!("Drain worker: task {} failed, re-enqueued for retry", task_id);
                                        } else {
                                            info!("Drain worker: task {} failed permanently: {}", task_id, e);
                                        }
                                    }
                                    TaskStatus::Timeout => {
                                        drain_metrics.tasks_processed.fetch_add(1, Ordering::Relaxed);
                                        drain_metrics.tasks_failed.fetch_add(1, Ordering::Relaxed);
                                        let retried = drain_queue.timeout(task_id).unwrap_or(false);
                                        if retried {
                                            info!("Drain worker: task {} timed out, re-enqueued", task_id);
                                        } else {
                                            info!("Drain worker: task {} timed out permanently", task_id);
                                        }
                                    }
                                    TaskStatus::NoCapability => {
                                        // No local agent can handle this — mark complete (no retry)
                                        drain_metrics.tasks_processed.fetch_add(1, Ordering::Relaxed);
                                        let _ = drain_queue.complete(task_id, resp.clone());
                                    }
                                }

                                {
                                    let status_str = match &resp.status {
                                        TaskStatus::Success => "Success (drain)".to_string(),
                                        TaskStatus::Error(e) => format!("Error (drain): {}", e),
                                        TaskStatus::Timeout => "Timeout (drain)".to_string(),
                                        TaskStatus::NoCapability => "NoCapability (drain)".to_string(),
                                    };
                                    let mut state = drain_dashboard.write().await;
                                    state.task_log.push(TaskLogEntry {
                                        id: task_id.to_string(),
                                        capability: cap_tag,
                                        status: status_str,
                                        duration_ms: resp.duration_ms,
                                        peer: "local (drain)".to_string(),
                                    });
                                    if state.task_log.len() > 1000 {
                                        state.task_log.remove(0);
                                    }
                                }
                            }
                            Ok(None) => break, // queue empty
                            Err(e) => {
                                tracing::warn!("Drain worker: dequeue error: {}", e);
                                break;
                            }
                        }
                    }

                    // Run cleanup every ~60 seconds (30 ticks × 2s)
                    cleanup_counter += 1;
                    if cleanup_counter >= 30 {
                        cleanup_counter = 0;
                        match drain_queue.cleanup() {
                            Ok(n) if n > 0 => info!("Drain worker: cleaned up {} expired records", n),
                            Err(e) => tracing::warn!("Drain worker: cleanup error: {}", e),
                            _ => {}
                        }
                    }
                }
                _ = drain_shutdown.recv() => {
                    info!("Drain worker shutting down");
                    break;
                }
            }
        }
    });

    // Spawn health check TCP endpoint if configured.
    if let Some(port) = health_port {
        let health_metrics = metrics.clone();
        let health_pt = peer_table.clone();
        let health_peer_id = peer_id_hex.clone();
        let health_queue = task_queue.clone();
        let mut health_shutdown = shutdown_tx.subscribe();

        tokio::spawn(async move {
            let bind_addr: SocketAddr = ([0, 0, 0, 0], port).into();
            let listener = match tokio::net::TcpListener::bind(bind_addr).await {
                Ok(l) => l,
                Err(e) => {
                    error!("Failed to bind health check on port {}: {}", port, e);
                    return;
                }
            };
            info!("Health check listening on port {}", port);

            loop {
                tokio::select! {
                    result = listener.accept() => {
                        if let Ok((mut stream, _)) = result {
                            let uptime = health_metrics.uptime_secs();
                            let tasks = health_metrics.tasks_processed.load(Ordering::Relaxed);
                            let failed = health_metrics.tasks_failed.load(Ordering::Relaxed);
                            let msgs_in = health_metrics.messages_received.load(Ordering::Relaxed);
                            let msgs_out = health_metrics.messages_sent.load(Ordering::Relaxed);
                            let peer_count = health_pt.read().await.len();
                            let (q_pending, q_total) = health_queue.stats()
                                .map(|s| (s.pending, s.total()))
                                .unwrap_or((0, 0));

                            let json = format!(
                                concat!(
                                    "{{",
                                    "\"status\":\"healthy\",",
                                    "\"peer_id\":\"{}\",",
                                    "\"uptime_secs\":{},",
                                    "\"tasks_processed\":{},",
                                    "\"tasks_failed\":{},",
                                    "\"messages_received\":{},",
                                    "\"messages_sent\":{},",
                                    "\"peers\":{},",
                                    "\"queue_pending\":{},",
                                    "\"queue_total\":{}",
                                    "}}\n"
                                ),
                                health_peer_id, uptime, tasks, failed, msgs_in, msgs_out,
                                peer_count, q_pending, q_total,
                            );

                            let _ = tokio::io::AsyncWriteExt::write_all(
                                &mut stream,
                                json.as_bytes(),
                            ).await;
                        }
                    }
                    _ = health_shutdown.recv() => {
                        info!("Health check shutting down");
                        break;
                    }
                }
            }
        });

        {
            let mut state = dashboard_state.write().await;
            state.add_log(format!("Health check enabled on port {}", port));
        }
    }

    if headless {
        info!("Running in headless mode. Press Ctrl+C to stop.");
        // Wait for SIGINT or SIGTERM
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Received SIGINT");
            }
            _ = sigterm.recv() => {
                info!("Received SIGTERM");
            }
        }
    } else {
        let mut dashboard = Dashboard::new(dashboard_state.clone());
        dashboard.run().await?;
    }

    // --- Graceful shutdown sequence ---
    info!("Initiating graceful shutdown...");

    // 0. Flush the task queue and trust store to disk
    if let Err(e) = task_queue.flush() {
        error!("Failed to flush task queue: {}", e);
    }
    {
        let ts = trust_store.lock().await;
        if let Err(e) = ts.flush() {
            error!("Failed to flush trust store: {}", e);
        }
        if let Err(e) = ts.sync() {
            error!("Failed to sync trust store: {}", e);
        }
        let peer_count = ts.peer_count();
        if peer_count > 0 {
            info!("Trust store: {} peers saved to disk", peer_count);
        }
    }

    // 1. Signal all background tasks to stop
    let _ = shutdown_tx.send(());

    // 2. Log final metrics
    let uptime = metrics.uptime_secs();
    let total_tasks = metrics.tasks_processed.load(Ordering::Relaxed);
    let failed_tasks = metrics.tasks_failed.load(Ordering::Relaxed);
    let total_msgs = metrics.messages_received.load(Ordering::Relaxed);
    info!(
        "Session stats: uptime={}s, tasks={} (failed={}), messages={}",
        uptime, total_tasks, failed_tasks, total_msgs
    );

    // 3. Shut down MCP bridge (kills server processes)
    mcp_bridge.shutdown().await;

    // 4. Shut down mDNS (stops advertising)
    if let Some(mdns) = _mdns {
        mdns.shutdown();
        info!("mDNS discovery stopped");
    }

    // 5. Close transport (sends close frames to all peers)
    transport.shutdown().await;
    info!("Node shut down cleanly.");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_message(
    msg: Message,
    conn: &quinn::Connection,
    runtime: &Arc<Runtime>,
    peer_table: &Arc<RwLock<PeerTable>>,
    dashboard: &Arc<RwLock<DashboardState>>,
    remote: SocketAddr,
    metrics: &Arc<NodeMetrics>,
    task_queue: &Arc<TaskQueue>,
    transport: &Arc<Transport>,
    tool_registry: &Arc<RwLock<ToolRegistry>>,
    trust_store: &Arc<Mutex<PersistentTrustStore>>,
    negotiation_state: &Arc<Mutex<NegotiationState>>,
    negotiator: &Arc<Negotiator>,
    trust_scoring: &Arc<TrustWeightedScoring>,
    bidder: &Arc<EagerBidder>,
    local_peer_id: &[u8],
    negotiation_requesters: &Arc<Mutex<HashMap<Uuid, quinn::Connection>>>,
) {
    match msg {
        Message::Ping { nonce } => {
            let pong = Message::Pong { nonce };
            let _ = Transport::send(conn, &pong).await;
            metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
        }
        Message::Announce(info) => {
            let local_peer_id = {
                let pt_read = peer_table.read().await;
                pt_read.local_peer().peer_id.clone()
            };
            let mut pt = peer_table.write().await;
            pt.upsert(info.clone());
            drop(pt);
            let mut state = dashboard.write().await;
            state.add_log(format!(
                "Peer announced: {} at {}",
                short_id(&info.peer_id),
                info.addr
            ));
            drop(state);
            // Share our MCP tools with the new peer
            let local_tools: Vec<_> = {
                let reg = tool_registry.read().await;
                reg.tools_for_peer(&local_peer_id)
                    .into_iter()
                    .cloned()
                    .collect()
            };
            if !local_tools.is_empty() {
                let catalog = Message::ToolCatalog {
                    peer_id: local_peer_id,
                    tools: local_tools,
                };
                let _ = Transport::send(conn, &catalog).await;
                metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
            }
        }
        Message::Gossip { peers } => {
            let mut pt = peer_table.write().await;
            let new = pt.merge_gossip(peers);
            drop(pt);
            if new > 0 {
                let mut state = dashboard.write().await;
                state.add_log(format!("Gossip: discovered {} new peers", new));
            }
        }
        Message::TaskRequest(req) => {
            let task_id = req.id;
            let req_id = req.id.to_string();
            let cap_tag = req.capability.tag();

            // Write-ahead: persist task before dispatch
            if let Err(e) = task_queue.enqueue(req.clone()) {
                tracing::warn!("Failed to enqueue task {}: {}", task_id, e);
            }

            let mut resp = runtime.dispatch(req.clone()).await;

            // If no local agent can handle it, try negotiation with capable peers.
            if resp.status == TaskStatus::NoCapability {
                let candidates: Vec<PeerInfo> = {
                    let pt = peer_table.read().await;
                    pt.find_by_capability(&req.capability)
                        .into_iter()
                        .filter(|p| p.addr != remote.to_string())
                        .cloned()
                        .collect()
                };

                if !candidates.is_empty() {
                    // Initiate negotiation: send TaskOffer to all capable peers
                    let payload_hint = req.payload.len() as u64;
                    let bid_deadline_ms = negotiator.bid_timeout.as_millis() as u64;
                    let offer = Message::TaskOffer {
                        request_id: task_id,
                        capability: req.capability.clone(),
                        payload_hint,
                        bid_deadline_ms,
                    };

                    let mut peers_solicited = 0;
                    for peer in &candidates {
                        let addr: SocketAddr = match peer.addr.parse() {
                            Ok(a) => a,
                            Err(_) => continue,
                        };
                        match transport.connect(addr).await {
                            Ok(peer_conn) => {
                                if Transport::send(&peer_conn, &offer).await.is_ok() {
                                    metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
                                    peers_solicited += 1;
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to connect to {} for TaskOffer: {}",
                                    addr,
                                    e
                                );
                            }
                        }
                    }

                    if peers_solicited > 0 {
                        // Start negotiation and store the request for later dispatch
                        let mut ns = negotiation_state.lock().await;
                        ns.start_with_request(
                            task_id,
                            req.capability.clone(),
                            payload_hint,
                            negotiator.bid_timeout,
                            peers_solicited,
                            req,
                        );
                        drop(ns);

                        // Store the requester's connection so we can respond after winner selection
                        let mut nr = negotiation_requesters.lock().await;
                        nr.insert(task_id, conn.clone());
                        drop(nr);

                        info!(
                            "Started negotiation {} — {} peers solicited for {}",
                            task_id, peers_solicited, cap_tag,
                        );
                        {
                            let mut state = dashboard.write().await;
                            state.add_log(format!(
                                "Negotiation started for task {} ({}) — {} peers",
                                short_id(task_id.as_bytes()),
                                cap_tag,
                                peers_solicited,
                            ));
                        }

                        // Don't send response yet — it comes after winner selection.
                        // Spawn a deadline timeout to handle the case where no bids arrive.
                        let ns_timeout = negotiation_state.clone();
                        let nr_timeout = negotiation_requesters.clone();
                        let tq_timeout = task_queue.clone();
                        let m_timeout = metrics.clone();
                        let ds_timeout = dashboard.clone();
                        let deadline = negotiator.bid_timeout;
                        tokio::spawn(async move {
                            tokio::time::sleep(deadline + std::time::Duration::from_millis(100))
                                .await;
                            let mut ns = ns_timeout.lock().await;
                            // If the negotiation is still active (wasn't completed by a bid),
                            // it expired with no bids — respond with NoCapability.
                            if let Some(mut neg) = ns.complete(&task_id) {
                                drop(ns);
                                let req = neg.take_request();
                                let mut nr = nr_timeout.lock().await;
                                if let Some(requester_conn) = nr.remove(&task_id) {
                                    drop(nr);
                                    let resp = axon_core::TaskResponse {
                                        request_id: task_id,
                                        status: TaskStatus::NoCapability,
                                        payload: Vec::new(),
                                        duration_ms: 0,
                                    };
                                    let _ = Transport::send(
                                        &requester_conn,
                                        &Message::TaskResponse(resp.clone()),
                                    )
                                    .await;
                                    m_timeout.tasks_processed.fetch_add(1, Ordering::Relaxed);
                                    m_timeout.tasks_failed.fetch_add(1, Ordering::Relaxed);
                                    if let Some(req) = req {
                                        let _ = tq_timeout.complete(req.id, resp);
                                    }
                                    let mut state = ds_timeout.write().await;
                                    state.add_log(format!(
                                        "Negotiation {} expired — no winning bids",
                                        short_id(task_id.as_bytes()),
                                    ));
                                }
                            }
                        });

                        return; // Exit early — response deferred to negotiation completion
                    }
                }

                // Fallback: no capable peers or no peers responded to offers —
                // use simple forwarding
                resp = forward_to_peer(
                    &req,
                    peer_table,
                    transport,
                    dashboard,
                    metrics,
                    remote,
                    trust_store,
                )
                .await
                .unwrap_or(resp);
            }

            // Update persistent record with result
            match &resp.status {
                TaskStatus::Success => {
                    metrics.tasks_processed.fetch_add(1, Ordering::Relaxed);
                    let _ = task_queue.complete(task_id, resp.clone());
                }
                TaskStatus::Error(e) => {
                    metrics.tasks_processed.fetch_add(1, Ordering::Relaxed);
                    metrics.tasks_failed.fetch_add(1, Ordering::Relaxed);
                    let _ = task_queue.fail(task_id, e.clone());
                }
                TaskStatus::Timeout => {
                    metrics.tasks_processed.fetch_add(1, Ordering::Relaxed);
                    metrics.tasks_failed.fetch_add(1, Ordering::Relaxed);
                    let _ = task_queue.timeout(task_id);
                }
                TaskStatus::NoCapability => {
                    metrics.tasks_processed.fetch_add(1, Ordering::Relaxed);
                    metrics.tasks_failed.fetch_add(1, Ordering::Relaxed);
                    // No retry for NoCapability — clear from queue
                    let _ = task_queue.complete(task_id, resp.clone());
                }
            }

            let status_str = match &resp.status {
                TaskStatus::Success => "Success".to_string(),
                TaskStatus::Error(e) => format!("Error: {}", e),
                TaskStatus::Timeout => "Timeout".to_string(),
                TaskStatus::NoCapability => "NoCapability".to_string(),
            };

            {
                let mut state = dashboard.write().await;
                state.task_log.push(TaskLogEntry {
                    id: req_id,
                    capability: cap_tag,
                    status: status_str,
                    duration_ms: resp.duration_ms,
                    peer: remote.to_string(),
                });
                if state.task_log.len() > 1000 {
                    state.task_log.remove(0);
                }
            }

            let _ = Transport::send(conn, &Message::TaskResponse(resp)).await;
            metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
        }
        Message::ForwardedTask(req) => {
            // Forwarded from another node — dispatch locally, never forward again.
            let task_id = req.id;
            let req_id = req.id.to_string();
            let cap_tag = req.capability.tag();

            info!(
                "Received forwarded task {} ({}) from {}",
                task_id, cap_tag, remote
            );

            let resp = runtime.dispatch(req).await;
            metrics.tasks_processed.fetch_add(1, Ordering::Relaxed);
            if !matches!(resp.status, TaskStatus::Success) {
                metrics.tasks_failed.fetch_add(1, Ordering::Relaxed);
            }

            let status_str = match &resp.status {
                TaskStatus::Success => "Success (forwarded)".to_string(),
                TaskStatus::Error(e) => format!("Error (forwarded): {}", e),
                TaskStatus::Timeout => "Timeout (forwarded)".to_string(),
                TaskStatus::NoCapability => "NoCapability (forwarded)".to_string(),
            };

            {
                let mut state = dashboard.write().await;
                state.task_log.push(TaskLogEntry {
                    id: req_id,
                    capability: cap_tag,
                    status: status_str,
                    duration_ms: resp.duration_ms,
                    peer: remote.to_string(),
                });
                if state.task_log.len() > 1000 {
                    state.task_log.remove(0);
                }
            }

            let _ = Transport::send(conn, &Message::TaskResponse(resp)).await;
            metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
        }
        Message::Discover { capability } => {
            let pt = peer_table.read().await;
            let peers: Vec<PeerInfo> = if capability.namespace == "*" {
                pt.all_peers_owned()
            } else {
                pt.find_by_capability(&capability)
                    .into_iter()
                    .cloned()
                    .collect()
            };
            let _ = Transport::send(conn, &Message::DiscoverResponse { peers }).await;
            metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
        }
        Message::Pong { nonce } => {
            tracing::debug!("Received Pong from {} with nonce {}", remote, nonce);
        }
        Message::ToolCatalog { peer_id, tools } => {
            let tool_count = tools.len();
            {
                let mut reg = tool_registry.write().await;
                reg.register_peer_tools(&peer_id, tools);
            }
            {
                let mut state = dashboard.write().await;
                state.add_log(format!(
                    "Received {} MCP tools from peer {}",
                    tool_count,
                    short_id(&peer_id),
                ));
            }
            info!(
                "Registered {} MCP tools from peer {}",
                tool_count,
                short_id(&peer_id),
            );
        }
        Message::ToolQuery {
            query,
            server_filter,
            limit,
            max_tokens,
            detail,
        } => {
            let detail_level = axon_core::SchemaDetail::from_u8(detail);
            let mut filter = axon_core::ToolFilter::new()
                .with_limit(limit as usize)
                .with_detail(detail_level);
            if !query.is_empty() {
                filter = filter.with_query(&query);
            }
            if let Some(s) = server_filter {
                filter = filter.with_server(s);
            }
            if max_tokens > 0 {
                filter = filter.with_max_tokens(max_tokens as usize);
            }

            let budget_result = {
                let reg = tool_registry.read().await;
                reg.search_within_budget(&filter)
            };

            let response_tools: Vec<axon_core::ToolQueryResult> = budget_result
                .tools
                .into_iter()
                .map(|r| axon_core::ToolQueryResult {
                    tool: r.tool,
                    score: r.score,
                    peer_id: hex_to_bytes(&r.peer_id_hex),
                })
                .collect();

            let resp = Message::ToolQueryResponse {
                tools: response_tools,
                total_tokens: budget_result.total_tokens as u32,
                truncated: budget_result.truncated,
            };
            let _ = Transport::send(conn, &resp).await;
            metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
        }
        Message::ToolQueryResponse { .. } => {
            // Response to a query we initiated — handled at call site, not here.
            tracing::debug!("Received ToolQueryResponse from {}", remote);
        }
        Message::TaskOffer {
            request_id,
            capability,
            payload_hint,
            bid_deadline_ms: _,
        } => {
            // A peer is soliciting bids for a task. Check if we can handle it
            // and respond with a TaskBid if so.
            let can_handle = runtime
                .all_capabilities()
                .await
                .iter()
                .any(|c| c.matches(&capability));
            if can_handle && bidder.should_bid(&capability, payload_hint) {
                let bid = bidder.compute_bid(request_id, &capability, payload_hint);
                let msg = Message::TaskBid {
                    request_id,
                    peer_id: local_peer_id.to_vec(),
                    estimated_latency_ms: bid.estimated_latency_ms,
                    load_factor: bid.load_factor,
                    confidence: bid.confidence,
                };
                let _ = Transport::send(conn, &msg).await;
                metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
                info!(
                    "Bid on task {} ({}) — latency: {}ms, load: {:.2}, confidence: {:.2}",
                    request_id,
                    capability.tag(),
                    bid.estimated_latency_ms,
                    bid.load_factor,
                    bid.confidence,
                );
                let mut state = dashboard.write().await;
                state.add_log(format!(
                    "Bid on task {} from {}",
                    short_id(request_id.as_bytes()),
                    remote,
                ));
            }
        }
        Message::TaskBid {
            request_id,
            peer_id,
            estimated_latency_ms,
            load_factor,
            confidence,
        } => {
            // A peer responded to our TaskOffer with a bid.
            let bid = ReceivedBid::new(
                request_id,
                peer_id.clone(),
                estimated_latency_ms,
                load_factor,
                confidence,
            );
            let mut ns = negotiation_state.lock().await;
            let recorded = ns.record_bid(bid);
            drop(ns);
            if recorded {
                info!(
                    "Received bid from {} for task {} — latency: {}ms, load: {:.2}, conf: {:.2}",
                    short_id(&peer_id),
                    request_id,
                    estimated_latency_ms,
                    load_factor,
                    confidence,
                );
            }

            // Check if any negotiations are ready for winner selection
            let mut ns = negotiation_state.lock().await;
            let ready: Vec<Uuid> = ns.ready_negotiations();
            for neg_id in ready {
                if let Some(mut neg) = ns.complete(&neg_id) {
                    let bids = &neg.bids;
                    if bids.is_empty() {
                        continue;
                    }

                    // Apply trust-weighted scoring to select winner
                    let scored_bids: Vec<(ReceivedBid, f64)> = bids
                        .iter()
                        .map(|b| {
                            let raw_score = negotiator.score_bid(b);
                            let ts_lock = trust_store.try_lock();
                            let weighted = if let Ok(ts) = ts_lock {
                                let peer_trust = ts.score(&b.peer_id);
                                trust_scoring.weighted_score(raw_score, &peer_trust)
                            } else {
                                raw_score
                            };
                            (b.clone(), weighted)
                        })
                        .collect();

                    if let Some((winner, score)) = scored_bids
                        .iter()
                        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                    {
                        info!(
                            "Negotiation {} complete — winner: {} (score: {:.3})",
                            neg_id,
                            short_id(&winner.peer_id),
                            score,
                        );

                        let winner_peer_id = winner.peer_id.clone();
                        let winner_estimated_latency = winner.estimated_latency_ms;

                        // Send BidAccept to winner, BidReject to losers
                        let accept_msg = Message::BidAccept {
                            request_id: neg_id,
                            winner_peer_id: winner_peer_id.clone(),
                        };
                        let reject_msg = Message::BidReject { request_id: neg_id };

                        // Find peer addresses for notification
                        let pt = peer_table.read().await;
                        for bid in bids {
                            let msg = if bid.peer_id == winner_peer_id {
                                &accept_msg
                            } else {
                                &reject_msg
                            };
                            // Find peer address
                            if let Some(peer_info) = pt
                                .all_peers_owned()
                                .iter()
                                .find(|p| p.peer_id == bid.peer_id)
                            {
                                if let Ok(addr) = peer_info.addr.parse::<SocketAddr>() {
                                    if let Ok(peer_conn) = transport.connect(addr).await {
                                        let _ = Transport::send(&peer_conn, msg).await;
                                        metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
                                    }
                                }
                            }
                        }
                        drop(pt);

                        // Forward the stored TaskRequest to the winner
                        let task_req = neg.take_request();
                        if let Some(req) = task_req {
                            let task_id = req.id;
                            let cap_tag = req.capability.tag();

                            // Find winner's address and forward
                            let pt = peer_table.read().await;
                            let winner_addr = pt
                                .all_peers_owned()
                                .iter()
                                .find(|p| p.peer_id == winner_peer_id)
                                .and_then(|p| p.addr.parse::<SocketAddr>().ok());
                            drop(pt);

                            let mut task_resp = None;
                            if let Some(addr) = winner_addr {
                                match transport.connect(addr).await {
                                    Ok(fwd_conn) => {
                                        let fwd_msg = Message::ForwardedTask(req.clone());
                                        if let Err(e) = Transport::send(&fwd_conn, &fwd_msg).await {
                                            tracing::warn!(
                                                "Forward to winner {} failed: {}",
                                                addr,
                                                e
                                            );
                                            record_trust(
                                                trust_store,
                                                &winner_peer_id,
                                                TaskOutcome::Timeout,
                                                0,
                                                0,
                                            )
                                            .await;
                                        } else {
                                            metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
                                            match Transport::recv(&fwd_conn).await {
                                                Ok(Message::TaskResponse(resp)) => {
                                                    info!(
                                                        "Negotiated task {} dispatched to {} — {:?}",
                                                        task_id, addr, resp.status
                                                    );
                                                    record_trust(
                                                        trust_store,
                                                        &winner_peer_id,
                                                        status_to_outcome(&resp.status),
                                                        resp.duration_ms,
                                                        winner_estimated_latency,
                                                    )
                                                    .await;
                                                    task_resp = Some(resp);
                                                }
                                                Ok(_) => {
                                                    tracing::warn!(
                                                        "Winner {} returned unexpected message",
                                                        addr
                                                    );
                                                    record_trust(
                                                        trust_store,
                                                        &winner_peer_id,
                                                        TaskOutcome::Failure,
                                                        0,
                                                        0,
                                                    )
                                                    .await;
                                                }
                                                Err(e) => {
                                                    tracing::warn!(
                                                        "Recv from winner {} failed: {}",
                                                        addr,
                                                        e
                                                    );
                                                    record_trust(
                                                        trust_store,
                                                        &winner_peer_id,
                                                        TaskOutcome::Timeout,
                                                        0,
                                                        0,
                                                    )
                                                    .await;
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!("Connect to winner {} failed: {}", addr, e);
                                        record_trust(
                                            trust_store,
                                            &winner_peer_id,
                                            TaskOutcome::Timeout,
                                            0,
                                            0,
                                        )
                                        .await;
                                    }
                                }
                            }

                            // Send response back to the original requester
                            let resp = task_resp.unwrap_or(axon_core::TaskResponse {
                                request_id: task_id,
                                status: TaskStatus::Error(
                                    "Negotiation dispatch failed".to_string(),
                                ),
                                payload: Vec::new(),
                                duration_ms: 0,
                            });

                            // Update task queue
                            match &resp.status {
                                TaskStatus::Success => {
                                    metrics.tasks_processed.fetch_add(1, Ordering::Relaxed);
                                    let _ = task_queue.complete(task_id, resp.clone());
                                }
                                TaskStatus::Error(e) => {
                                    metrics.tasks_processed.fetch_add(1, Ordering::Relaxed);
                                    metrics.tasks_failed.fetch_add(1, Ordering::Relaxed);
                                    let _ = task_queue.fail(task_id, e.clone());
                                }
                                TaskStatus::Timeout => {
                                    metrics.tasks_processed.fetch_add(1, Ordering::Relaxed);
                                    metrics.tasks_failed.fetch_add(1, Ordering::Relaxed);
                                    let _ = task_queue.timeout(task_id);
                                }
                                TaskStatus::NoCapability => {
                                    metrics.tasks_processed.fetch_add(1, Ordering::Relaxed);
                                    metrics.tasks_failed.fetch_add(1, Ordering::Relaxed);
                                    let _ = task_queue.complete(task_id, resp.clone());
                                }
                            }

                            // Dashboard log
                            let status_str = match &resp.status {
                                TaskStatus::Success => "Success".to_string(),
                                TaskStatus::Error(e) => format!("Error: {}", e),
                                TaskStatus::Timeout => "Timeout".to_string(),
                                TaskStatus::NoCapability => "NoCapability".to_string(),
                            };
                            {
                                let mut state = dashboard.write().await;
                                state.task_log.push(TaskLogEntry {
                                    id: task_id.to_string(),
                                    capability: cap_tag,
                                    status: status_str,
                                    duration_ms: resp.duration_ms,
                                    peer: format!("negotiated:{}", short_id(&winner_peer_id)),
                                });
                                if state.task_log.len() > 1000 {
                                    state.task_log.remove(0);
                                }
                            }

                            // Respond to original requester
                            let mut nr = negotiation_requesters.lock().await;
                            if let Some(requester_conn) = nr.remove(&task_id) {
                                drop(nr);
                                let _ =
                                    Transport::send(&requester_conn, &Message::TaskResponse(resp))
                                        .await;
                                metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
            }
        }
        Message::BidAccept {
            request_id,
            winner_peer_id: _,
        } => {
            info!("Our bid for task {} was accepted by {}", request_id, remote);
            let mut state = dashboard.write().await;
            state.add_log(format!("Bid accepted for task {}", request_id));
        }
        Message::BidReject { request_id } => {
            tracing::debug!("Our bid for task {} was rejected by {}", request_id, remote);
        }
        Message::TrustGossip {
            observer_peer_id,
            observations,
        } => {
            // Process trust observations shared by a peer.
            let processor = TrustGossipProcessor::new();
            let mut accepted = 0usize;
            let total = observations.len();

            let mut store = trust_store.lock().await;
            for entry in &observations {
                let outcome = match entry.outcome {
                    0 => TaskOutcome::Success,
                    1 => TaskOutcome::Failure,
                    2 => TaskOutcome::Timeout,
                    _ => TaskOutcome::Rejected,
                };
                let obs = TrustObservation::new(
                    outcome,
                    entry.estimated_latency_ms,
                    entry.actual_latency_ms,
                )
                .with_timestamp(entry.timestamp);

                let shared = axon_core::SharedTrustObservation {
                    subject_peer_id: entry.subject_peer_id.clone(),
                    observer_peer_id: observer_peer_id.clone(),
                    observation: obs,
                };

                if processor.process(store.inner_mut(), &shared) {
                    accepted += 1;
                }
            }
            drop(store);

            if accepted > 0 {
                info!(
                    "Accepted {}/{} trust observations from peer {}",
                    accepted,
                    total,
                    short_id(&observer_peer_id),
                );
            }
        }
        _ => {
            tracing::warn!("Unhandled message type from {}", remote);
        }
    }
}

/// Attempt to forward a task to a capable peer.
///
/// Looks up the peer table for peers advertising the required capability,
/// connects to the best candidate, sends a `ForwardedTask` message, and
/// returns the response. Returns `None` if no capable peers exist or all
/// forwarding attempts fail.
async fn forward_to_peer(
    req: &axon_core::TaskRequest,
    peer_table: &Arc<RwLock<PeerTable>>,
    transport: &Arc<Transport>,
    dashboard: &Arc<RwLock<DashboardState>>,
    metrics: &Arc<NodeMetrics>,
    requester: SocketAddr,
    trust_store: &Arc<Mutex<PersistentTrustStore>>,
) -> Option<axon_core::TaskResponse> {
    let pt = peer_table.read().await;
    let capable = pt.find_by_capability(&req.capability);

    // Filter out the requester to avoid bouncing the task back
    let candidates: Vec<_> = capable
        .into_iter()
        .filter(|p| p.addr != requester.to_string())
        .collect();

    if candidates.is_empty() {
        return None;
    }

    let cap_tag = req.capability.tag();
    info!(
        "Forwarding task {} ({}) — {} candidate peer(s)",
        req.id,
        cap_tag,
        candidates.len()
    );

    // Try candidates in order until one succeeds
    for peer in &candidates {
        let addr: SocketAddr = match peer.addr.parse() {
            Ok(a) => a,
            Err(_) => continue,
        };

        {
            let mut state = dashboard.write().await;
            state.add_log(format!(
                "Forwarding task {} to peer {} at {}",
                req.id,
                short_id(&peer.peer_id),
                addr,
            ));
        }

        match transport.connect(addr).await {
            Ok(fwd_conn) => {
                let fwd_msg = Message::ForwardedTask(req.clone());
                if let Err(e) = Transport::send(&fwd_conn, &fwd_msg).await {
                    tracing::warn!("Forward send to {} failed: {}", addr, e);
                    continue;
                }
                metrics.messages_sent.fetch_add(1, Ordering::Relaxed);

                match Transport::recv(&fwd_conn).await {
                    Ok(Message::TaskResponse(resp)) => {
                        info!(
                            "Forward of task {} to {} returned {:?}",
                            req.id, addr, resp.status
                        );
                        // Record trust observation for the peer that handled the task
                        record_trust(
                            trust_store,
                            &peer.peer_id,
                            status_to_outcome(&resp.status),
                            resp.duration_ms,
                            0, // no bid estimate in forwarding
                        )
                        .await;
                        return Some(resp);
                    }
                    Ok(other) => {
                        tracing::warn!(
                            "Forward to {} returned unexpected message: {:?}",
                            addr,
                            other
                        );
                        // Record as failure — unexpected response
                        record_trust(trust_store, &peer.peer_id, TaskOutcome::Failure, 0, 0).await;
                    }
                    Err(e) => {
                        tracing::warn!("Forward recv from {} failed: {}", addr, e);
                        // Record as timeout — connection failed
                        record_trust(trust_store, &peer.peer_id, TaskOutcome::Timeout, 0, 0).await;
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Forward connect to {} failed: {}", addr, e);
                // Record as timeout — couldn't connect
                record_trust(trust_store, &peer.peer_id, TaskOutcome::Timeout, 0, 0).await;
            }
        }
    }

    info!("All forwarding attempts for task {} failed", req.id);
    None
}

async fn run_chat_remote(peer_addr: SocketAddr) -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::sink)
        .init();

    let identity = Identity::load_or_generate(&Identity::default_path())?;
    let transport = Transport::bind("0.0.0.0:0".parse()?, &identity).await?;

    println!(
        "\x1b[36m▲ AXON Chat\x1b[0m · remote node {}\n\
         Type your message and press Enter. Empty line or Ctrl-C to quit.\n",
        peer_addr
    );

    let conn = transport.connect(peer_addr).await?;
    eprintln!("\x1b[32mConnected.\x1b[0m\n");

    let stdin = std::io::stdin();
    let mut input = String::new();

    loop {
        eprint!("\x1b[36m>\x1b[0m ");
        input.clear();
        if stdin.read_line(&mut input)? == 0 || input.trim().is_empty() {
            break;
        }

        let prompt = input.trim().to_string();

        let req = axon_core::TaskRequest {
            id: Uuid::new_v4(),
            capability: Capability::new("llm", "chat", 1),
            payload: prompt.into_bytes(),
            timeout_ms: 60000,
        };

        Transport::send(&conn, &Message::TaskRequest(req)).await?;

        match Transport::recv(&conn).await? {
            Message::TaskResponse(r) => match r.status {
                TaskStatus::Success => {
                    let text = String::from_utf8(r.payload).unwrap_or_default();
                    println!("\n{}\n", text);
                }
                TaskStatus::Error(e) => {
                    eprintln!("\x1b[31mAgent error: {}\x1b[0m\n", e);
                }
                TaskStatus::Timeout => {
                    eprintln!("\x1b[33mRequest timed out.\x1b[0m\n");
                }
                TaskStatus::NoCapability => {
                    eprintln!(
                        "\x1b[33mNo LLM agent on this node. \
                         Make sure the node was started with a provider configured.\x1b[0m\n"
                    );
                }
            },
            other => {
                eprintln!("Unexpected response: {:?}", other);
            }
        }
    }

    transport.shutdown().await;
    println!("\nBye!");
    Ok(())
}

async fn send_task(
    peer_addr: SocketAddr,
    namespace: &str,
    name: &str,
    data: &[u8],
) -> anyhow::Result<()> {
    let identity = Identity::load_or_generate(&Identity::default_path())?;
    let transport = Transport::bind("0.0.0.0:0".parse()?, &identity).await?;

    println!("Connecting to {}...", peer_addr);
    let conn = transport.connect(peer_addr).await?;

    let req = axon_core::TaskRequest {
        id: Uuid::new_v4(),
        capability: Capability::new(namespace, name, 1),
        payload: data.to_vec(),
        timeout_ms: 30000,
    };

    println!("Sending task {} ({}) ...", req.id, req.capability.tag());
    Transport::send(&conn, &Message::TaskRequest(req.clone())).await?;

    println!("Waiting for response...");
    let resp = Transport::recv(&conn).await?;

    match resp {
        Message::TaskResponse(r) => {
            println!("Task ID: {}", r.request_id);
            println!("Status: {:?}", r.status);
            println!("Duration: {}ms", r.duration_ms);
            if !r.payload.is_empty() {
                if let Ok(text) = String::from_utf8(r.payload.clone()) {
                    println!("Response: {}", text);
                } else {
                    println!("Response: {} bytes (binary)", r.payload.len());
                }
            }
        }
        other => {
            println!("Unexpected response: {:?}", other);
        }
    }

    transport.shutdown().await;
    Ok(())
}

async fn query_tools(
    node_addr: SocketAddr,
    query: Option<String>,
    server: Option<String>,
    limit: u32,
    budget: u32,
    detail: u8,
    json_output: bool,
) -> anyhow::Result<()> {
    let identity = Identity::load_or_generate(&Identity::default_path())?;
    let transport = Transport::bind("0.0.0.0:0".parse()?, &identity).await?;

    println!("Querying tools on {}...\n", node_addr);
    let conn = transport.connect(node_addr).await?;

    let msg = Message::ToolQuery {
        query: query.clone().unwrap_or_default(),
        server_filter: server.clone(),
        limit,
        max_tokens: budget,
        detail,
    };
    Transport::send(&conn, &msg).await?;

    let resp = Transport::recv(&conn).await?;
    match resp {
        Message::ToolQueryResponse {
            tools,
            total_tokens,
            truncated,
        } => {
            if json_output {
                let output = serde_json::json!({
                    "tools": tools,
                    "total_tokens": total_tokens,
                    "truncated": truncated,
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else if tools.is_empty() {
                println!("No tools found.");
                if let Some(q) = &query {
                    println!("Try a broader query than \"{}\".", q);
                }
            } else {
                let detail_label = match detail {
                    1 => "summary",
                    2 => "compact",
                    _ => "full",
                };
                println!(
                    "{:<24} {:<16} {:<8} {:<40}",
                    "TOOL", "SERVER", "SCORE", "DESCRIPTION"
                );
                println!("{}", "-".repeat(88));
                for result in &tools {
                    let desc = if result.tool.description.len() > 38 {
                        format!("{}...", &result.tool.description[..35])
                    } else {
                        result.tool.description.clone()
                    };
                    println!(
                        "{:<24} {:<16} {:<8.2} {:<40}",
                        result.tool.name, result.tool.server_name, result.score, desc
                    );
                }
                println!(
                    "\n{} tool(s) returned. ~{} tokens ({} detail).{}",
                    tools.len(),
                    total_tokens,
                    detail_label,
                    if truncated {
                        " [truncated by budget]"
                    } else {
                        ""
                    }
                );
            }
        }
        other => {
            println!("Unexpected response from node: {:?}", other);
        }
    }

    transport.shutdown().await;
    Ok(())
}

fn short_id(id: &[u8]) -> String {
    id.iter().take(4).map(|b| format!("{:02x}", b)).collect()
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

/// Convert a TaskStatus to a TaskOutcome for trust recording.
fn status_to_outcome(status: &TaskStatus) -> TaskOutcome {
    match status {
        TaskStatus::Success => TaskOutcome::Success,
        TaskStatus::Error(_) => TaskOutcome::Failure,
        TaskStatus::Timeout => TaskOutcome::Timeout,
        TaskStatus::NoCapability => TaskOutcome::Rejected,
    }
}

/// Record a trust observation for a peer based on task outcome.
async fn record_trust(
    trust_store: &Mutex<PersistentTrustStore>,
    peer_id: &[u8],
    outcome: TaskOutcome,
    duration_ms: u64,
    estimated_latency_ms: u64,
) {
    let obs = TrustObservation::new(outcome, estimated_latency_ms, duration_ms);
    let mut store = trust_store.lock().await;
    let _ = store.record_observation(peer_id, obs);
}

/// Lightweight message handler for the MCP mesh gateway.
/// Only handles gossip, announcements, and tool catalogs — no task dispatch.
async fn handle_mesh_message(
    msg: Message,
    conn: &quinn::Connection,
    peer_table: &Arc<RwLock<axon_core::PeerTable>>,
    tool_registry: &Arc<RwLock<axon_core::ToolRegistry>>,
    remote: SocketAddr,
) {
    match msg {
        Message::Ping { nonce } => {
            let _ = Transport::send(conn, &Message::Pong { nonce }).await;
        }
        Message::Pong { .. } => {}
        Message::Announce(info) => {
            let mut pt = peer_table.write().await;
            pt.upsert(info);
        }
        Message::Gossip { peers } => {
            let mut pt = peer_table.write().await;
            pt.merge_gossip(peers);
        }
        Message::ToolCatalog { peer_id, tools } => {
            let tool_count = tools.len();
            let mut reg = tool_registry.write().await;
            reg.register_peer_tools(&peer_id, tools);
            eprintln!(
                "axon-mcp-mesh-gateway: received {} tools from peer {}",
                tool_count,
                short_id(&peer_id),
            );
        }
        Message::Discover { .. } => {
            // Respond with our known peers
            let pt = peer_table.read().await;
            let peers = pt.all_peers_owned();
            let _ = Transport::send(conn, &Message::DiscoverResponse { peers }).await;
        }
        _ => {
            tracing::debug!("MCP gateway ignoring message type from {}", remote);
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
