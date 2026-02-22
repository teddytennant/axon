mod agents;
mod providers;
mod tui;

use axon_core::{
    Identity, Message, PeerInfo, PeerTable, Runtime, Transport, Capability,
    TaskStatus, MdnsDiscovery, DiscoveryEvent,
};
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};
use uuid::Uuid;

use agents::{EchoAgent, LlmAgent, SystemInfoAgent};
use providers::ProviderKind;
use tui::{Dashboard, DashboardState, TaskLogEntry};

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
        } => {
            if !headless {
                tracing_subscriber::fmt()
                    .with_writer(std::io::sink)
                    .init();
            } else {
                tracing_subscriber::fmt::init();
            }

            // Resolve provider config
            let api_key = providers::resolve_api_key(&api_key, &provider);
            let model = if model.is_empty() {
                providers::default_model(&provider).to_string()
            } else {
                model
            };
            let endpoint = if llm_endpoint.is_empty() {
                providers::default_endpoint(&provider).to_string()
            } else {
                llm_endpoint
            };

            let llm_provider = providers::build_provider(&provider, &endpoint, &api_key, &model)?;

            run_node(listen, bootstrap_peers, headless, llm_provider).await?;
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
            println!("Querying node at {}...", node);
            let path = Identity::default_path();
            if path.exists() {
                let id = Identity::load_or_generate(&path)?;
                println!("Local Peer ID: {}", id.peer_id_hex());
            }
            println!("(Peer discovery query not yet implemented over network)");
        }
        Commands::Identity => {
            let path = Identity::default_path();
            let id = Identity::load_or_generate(&path)?;
            println!("Identity file: {}", path.display());
            println!("Peer ID: {}", id.peer_id_hex());
            println!("Short ID: {}", id.peer_id_short());
        }
    }

    Ok(())
}

async fn run_node(
    listen: SocketAddr,
    bootstrap_peers: Vec<SocketAddr>,
    headless: bool,
    llm_provider: Box<dyn providers::LlmProvider>,
) -> anyhow::Result<()> {
    let identity = Identity::load_or_generate(&Identity::default_path())?;
    let peer_id_hex = identity.peer_id_hex();
    info!("Starting axon node with Peer ID: {}", peer_id_hex);

    let transport = Transport::bind(listen, &identity).await?;
    let local_addr = transport.local_addr()?;
    info!("Listening on {}", local_addr);

    let runtime = Arc::new(Runtime::new());
    runtime.register(Arc::new(EchoAgent)).await;
    runtime.register(Arc::new(SystemInfoAgent)).await;
    runtime
        .register(Arc::new(LlmAgent::new(Arc::from(llm_provider))))
        .await;

    let local_peer = PeerInfo {
        peer_id: identity.public_key_bytes(),
        addr: local_addr.to_string(),
        capabilities: runtime.all_capabilities().await,
        last_seen: now_secs(),
    };

    let peer_table = Arc::new(RwLock::new(PeerTable::new(local_peer)));

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
    }

    let transport = Arc::new(transport);
    let active_connections: Arc<RwLock<Vec<(String, quinn::Connection)>>> =
        Arc::new(RwLock::new(Vec::new()));

    // Spawn connection acceptor
    let accept_transport = transport.clone();
    let accept_runtime = runtime.clone();
    let accept_peer_table = peer_table.clone();
    let accept_dashboard = dashboard_state.clone();
    let accept_conns = active_connections.clone();

    tokio::spawn(async move {
        loop {
            if let Some(conn) = accept_transport.accept().await {
                let rt = accept_runtime.clone();
                let pt = accept_peer_table.clone();
                let ds = accept_dashboard.clone();
                let remote = conn.remote_address();

                {
                    let mut conns = accept_conns.write().await;
                    conns.push((remote.to_string(), conn.clone()));
                }

                tokio::spawn(async move {
                    loop {
                        match Transport::recv(&conn).await {
                            Ok(msg) => {
                                handle_message(msg, &conn, &rt, &pt, &ds, remote).await;
                            }
                            Err(e) => {
                                info!("Connection from {} closed: {}", remote, e);
                                break;
                            }
                        }
                    }
                });
            }
        }
    });

    // Connect to bootstrap peers
    for peer_addr in &bootstrap_peers {
        let t = transport.clone();
        let addr = *peer_addr;
        let _pt = peer_table.clone();
        let ds = dashboard_state.clone();
        let id = identity.public_key_bytes();
        let caps = runtime.all_capabilities().await;
        let la = local_addr.to_string();

        tokio::spawn(async move {
            match t.connect(addr).await {
                Ok(conn) => {
                    let announce = Message::Announce(PeerInfo {
                        peer_id: id,
                        addr: la,
                        capabilities: caps,
                        last_seen: now_secs(),
                    });
                    if let Err(e) = Transport::send(&conn, &announce).await {
                        error!("Failed to announce to {}: {}", addr, e);
                    } else {
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
    let mdns_result = MdnsDiscovery::new(
        peer_id_hex.clone(),
        local_addr.port(),
        all_caps,
    );

    let _mdns = if let Ok((mdns, mut mdns_rx)) = mdns_result {
        let mdns_pt = peer_table.clone();
        let mdns_ds = dashboard_state.clone();
        let mdns_transport = transport.clone();
        let mdns_id = identity.public_key_bytes();
        let mdns_caps = runtime.all_capabilities().await;
        let mdns_la = local_addr.to_string();

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
                                    }
                                    Err(e) => {
                                        let mut state = mdns_ds.write().await;
                                        state.add_log(format!("mDNS: failed to connect to {}: {}", addr, e));
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
        )
        .await;
    });

    // Spawn periodic peer table sync to dashboard
    let sync_pt = peer_table.clone();
    let sync_ds = dashboard_state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            let table = sync_pt.read().await;
            let peers = table.all_peers_owned();
            drop(table);
            let mut state = sync_ds.write().await;
            state.peers = peers;
        }
    });

    if headless {
        info!("Running in headless mode. Press Ctrl+C to stop.");
        tokio::signal::ctrl_c().await?;
    } else {
        let mut dashboard = Dashboard::new(dashboard_state.clone());
        dashboard.run().await?;
    }

    if let Some(mdns) = _mdns {
        mdns.shutdown();
    }
    transport.shutdown().await;
    info!("Node shut down.");
    Ok(())
}

async fn handle_message(
    msg: Message,
    conn: &quinn::Connection,
    runtime: &Arc<Runtime>,
    peer_table: &Arc<RwLock<PeerTable>>,
    dashboard: &Arc<RwLock<DashboardState>>,
    remote: SocketAddr,
) {
    match msg {
        Message::Ping { nonce } => {
            let pong = Message::Pong { nonce };
            let _ = Transport::send(conn, &pong).await;
        }
        Message::Announce(info) => {
            let mut pt = peer_table.write().await;
            pt.upsert(info.clone());
            drop(pt);
            let mut state = dashboard.write().await;
            state.add_log(format!("Peer announced: {} at {}", short_id(&info.peer_id), info.addr));
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
            let req_id = req.id.to_string();
            let cap_tag = req.capability.tag();
            let resp = runtime.dispatch(req).await;

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
            }

            let _ = Transport::send(conn, &Message::TaskResponse(resp)).await;
        }
        Message::Discover { capability } => {
            let pt = peer_table.read().await;
            let peers = pt.find_by_capability(&capability)
                .into_iter()
                .cloned()
                .collect();
            let _ = Transport::send(conn, &Message::DiscoverResponse { peers }).await;
        }
        _ => {}
    }
}

async fn send_task(peer_addr: SocketAddr, namespace: &str, name: &str, data: &[u8]) -> anyhow::Result<()> {
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

fn short_id(id: &[u8]) -> String {
    id.iter().take(4).map(|b| format!("{:02x}", b)).collect()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
