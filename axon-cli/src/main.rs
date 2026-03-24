mod agents;
mod config;
mod providers;
mod tui;

use axon_core::{
    Capability, DiscoveryEvent, Identity, McpBridge, McpBridgeAgent, MdnsDiscovery, Message,
    PeerInfo, PeerTable, Runtime, TaskQueue, TaskQueueConfig, TaskStatus, ToolRegistry, Transport,
};
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
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

        /// LLM provider: ollama, openai, xai, openrouter, custom
        #[arg(long, default_value = "ollama")]
        provider: ProviderKind,

        /// LLM endpoint URL (defaults per provider)
        #[arg(long, default_value = "")]
        llm_endpoint: String,

        /// API key for the LLM provider (or set env: OPENAI_API_KEY, XAI_API_KEY, OPENROUTER_API_KEY)
        #[arg(long, default_value = "")]
        api_key: String,

        /// Model name (defaults per provider)
        #[arg(long, default_value = "")]
        model: String,

        /// Enable health check TCP endpoint on this port (e.g., 4243)
        #[arg(long)]
        health_port: Option<u16>,
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

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
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
        } => {
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
        Commands::Tools {
            node,
            query,
            server,
            limit,
            json,
        } => {
            tracing_subscriber::fmt::init();
            query_tools(node, query, server, limit, json).await?;
        }
    }

    Ok(())
}

async fn run_node(
    listen: SocketAddr,
    bootstrap_peers: Vec<SocketAddr>,
    headless: bool,
    llm_provider: Box<dyn providers::LlmProvider>,
    health_port: Option<u16>,
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

    let local_peer = PeerInfo {
        peer_id: identity.public_key_bytes(),
        addr: local_addr.to_string(),
        capabilities: runtime.all_capabilities().await,
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
        if mcp_tool_count > 0 {
            state.add_log(format!(
                "MCP bridge: {} tools from {} server(s)",
                mcp_tool_count,
                mcp_bridge.server_count().await,
            ));
        }
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
                                    handle_message(msg, &conn, &rt, &pt, &ds, remote, &m, &tq, &fwd_t, &tr).await;
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
                                            &conn, &mdns_id, &mdns_tools,
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
    tokio::spawn(async move {
        axon_core::gossip::run_gossip(
            gossip_pt,
            gossip_transport,
            gossip_conns,
            axon_core::GossipConfig::default(),
            local_catalog,
        )
        .await;
    });

    // Spawn periodic peer table sync to dashboard + connection cleanup + metrics
    let sync_pt = peer_table.clone();
    let sync_ds = dashboard_state.clone();
    let sync_conns = active_connections.clone();
    let sync_metrics = metrics.clone();
    let mut sync_shutdown = shutdown_tx.subscribe();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(1)) => {
                    let table = sync_pt.read().await;
                    let peers = table.all_peers_owned();
                    drop(table);
                    let mut state = sync_ds.write().await;
                    state.peers = peers;
                    state.uptime_secs = sync_metrics.uptime_secs();
                    state.tasks_total = sync_metrics.tasks_processed.load(Ordering::Relaxed);
                    state.tasks_failed = sync_metrics.tasks_failed.load(Ordering::Relaxed);
                    drop(state);

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

    // 0. Flush the task queue to disk
    if let Err(e) = task_queue.flush() {
        error!("Failed to flush task queue: {}", e);
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

            // If no local agent can handle it, try forwarding to a capable peer.
            if resp.status == TaskStatus::NoCapability {
                resp = forward_to_peer(&req, peer_table, transport, dashboard, metrics, remote)
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
        } => {
            let filter = axon_core::ToolFilter::new().with_limit(limit as usize);
            let filter = if query.is_empty() {
                filter
            } else {
                filter.with_query(&query)
            };
            let filter = match server_filter {
                Some(s) => filter.with_server(s),
                None => filter,
            };

            let results = {
                let reg = tool_registry.read().await;
                reg.search(&filter)
            };

            let response_tools: Vec<axon_core::ToolQueryResult> = results
                .into_iter()
                .map(|r| axon_core::ToolQueryResult {
                    tool: r.tool,
                    score: r.score,
                    peer_id: hex_to_bytes(&r.peer_id_hex),
                })
                .collect();

            let resp = Message::ToolQueryResponse {
                tools: response_tools,
            };
            let _ = Transport::send(conn, &resp).await;
            metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
        }
        Message::ToolQueryResponse { .. } => {
            // Response to a query we initiated — handled at call site, not here.
            tracing::debug!("Received ToolQueryResponse from {}", remote);
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
                        return Some(resp);
                    }
                    Ok(other) => {
                        tracing::warn!(
                            "Forward to {} returned unexpected message: {:?}",
                            addr,
                            other
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Forward recv from {} failed: {}", addr, e);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Forward connect to {} failed: {}", addr, e);
            }
        }
    }

    info!("All forwarding attempts for task {} failed", req.id);
    None
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
    };
    Transport::send(&conn, &msg).await?;

    let resp = Transport::recv(&conn).await?;
    match resp {
        Message::ToolQueryResponse { tools } => {
            if json_output {
                println!("{}", serde_json::to_string_pretty(&tools)?);
            } else if tools.is_empty() {
                println!("No tools found.");
                if let Some(q) = &query {
                    println!("Try a broader query than \"{}\".", q);
                }
            } else {
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
                    "\n{} tool(s) found. ~{} tokens if loaded into context.",
                    tools.len(),
                    tools
                        .iter()
                        .map(|t| t.tool.estimated_tokens())
                        .sum::<usize>()
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

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
