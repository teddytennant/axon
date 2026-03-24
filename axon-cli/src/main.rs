mod agents;
mod config;
mod providers;
mod tui;

use axon_core::{
    Identity, Message, PeerInfo, PeerTable, Runtime, Transport, Capability,
    TaskStatus, MdnsDiscovery, DiscoveryEvent, TaskQueue, TaskQueueConfig,
};
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
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
                tracing_subscriber::fmt()
                    .with_writer(std::io::sink)
                    .init();
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

            run_node(effective_listen, effective_peers, effective_headless, llm_provider).await?;
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

    let local_peer = PeerInfo {
        peer_id: identity.public_key_bytes(),
        addr: local_addr.to_string(),
        capabilities: runtime.all_capabilities().await,
        last_seen: now_secs(),
    };

    let peer_table = Arc::new(RwLock::new(PeerTable::new(local_peer)));
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
            state.add_log(format!("Recovered {} tasks from previous session", recovered));
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
                                    handle_message(msg, &conn, &rt, &pt, &ds, remote, &m, &tq).await;
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
    for peer_addr in &bootstrap_peers {
        let t = transport.clone();
        let addr = *peer_addr;
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

    if headless {
        info!("Running in headless mode. Press Ctrl+C to stop.");
        // Wait for SIGINT or SIGTERM
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
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

    // 3. Shut down mDNS (stops advertising)
    if let Some(mdns) = _mdns {
        mdns.shutdown();
        info!("mDNS discovery stopped");
    }

    // 4. Close transport (sends close frames to all peers)
    transport.shutdown().await;
    info!("Node shut down cleanly.");
    Ok(())
}

async fn handle_message(
    msg: Message,
    conn: &quinn::Connection,
    runtime: &Arc<Runtime>,
    peer_table: &Arc<RwLock<PeerTable>>,
    dashboard: &Arc<RwLock<DashboardState>>,
    remote: SocketAddr,
    metrics: &Arc<NodeMetrics>,
    task_queue: &Arc<TaskQueue>,
) {
    match msg {
        Message::Ping { nonce } => {
            let pong = Message::Pong { nonce };
            let _ = Transport::send(conn, &pong).await;
            metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
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
            let task_id = req.id;
            let req_id = req.id.to_string();
            let cap_tag = req.capability.tag();

            // Write-ahead: persist task before dispatch
            if let Err(e) = task_queue.enqueue(req.clone()) {
                tracing::warn!("Failed to enqueue task {}: {}", task_id, e);
            }

            let resp = runtime.dispatch(req).await;

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
        _ => {
            tracing::warn!("Unhandled message type from {}", remote);
        }
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
