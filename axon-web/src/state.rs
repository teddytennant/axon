use axon_core::{McpBridge, PeerTable, PersistentTrustStore, Runtime, TaskQueue, ToolRegistry};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

// Re-export the shared dashboard types owned by `axon_core::dashboard` so
// existing `axon_web::state::{AgentInfo, TaskLogEntry, WorkflowSnapshot,
// StepSnapshot, BlackboardEntry}` imports keep resolving. These used to be
// duplicate definitions that silently drifted from the CLI's copies; they are
// now a single source of truth.
pub use axon_core::{AgentInfo, BlackboardEntry, StepSnapshot, TaskLogEntry, WorkflowSnapshot};

/// Shared state between the web server and the running axon node.
///
/// All core fields are `Arc`-wrapped handles to the same state objects that
/// `run_node()` creates. The `web_*` fields are populated by the sync loop
/// in main.rs alongside the TUI updates.
pub struct SharedWebState {
    // --- Core state (shared with TUI/node) ---
    pub peer_table: Arc<RwLock<PeerTable>>,
    pub tool_registry: Arc<RwLock<ToolRegistry>>,
    pub trust_store: Arc<Mutex<PersistentTrustStore>>,
    pub task_queue: Arc<TaskQueue>,
    pub runtime: Arc<Runtime>,
    pub mcp_bridge: Arc<McpBridge>,
    pub local_peer_id: Vec<u8>,

    // --- Web-specific shared state ---
    pub web_state: Arc<RwLock<WebState>>,
}

/// Extra state the web server needs that isn't in axon-core.
/// Populated by the sync loop in main.rs.
pub struct WebState {
    pub peer_id: String,
    pub listen_addr: String,
    pub uptime_secs: u64,
    pub tasks_total: u64,
    pub tasks_failed: u64,
    pub messages_received: u64,
    pub messages_sent: u64,
    pub provider_name: String,
    pub model_name: String,
    pub config_path: String,
    pub agent_info: Vec<AgentInfo>,
    pub task_log: Vec<TaskLogEntry>,
    pub logs: VecDeque<String>,
    pub throughput_history: VecDeque<u64>,
    pub crdt_counters: Vec<(String, u64)>,
    pub crdt_registers: Vec<(String, String)>,
    pub crdt_sets: Vec<(String, Vec<String>)>,
    // Orchestration state
    pub active_workflows: Vec<WorkflowSnapshot>,
    pub completed_workflows: VecDeque<WorkflowSnapshot>,
    pub blackboard_entries: Vec<BlackboardEntry>,
}

impl WebState {
    pub fn new(peer_id: String, listen_addr: String) -> Self {
        Self {
            peer_id,
            listen_addr,
            uptime_secs: 0,
            tasks_total: 0,
            tasks_failed: 0,
            messages_received: 0,
            messages_sent: 0,
            provider_name: String::new(),
            model_name: String::new(),
            config_path: String::new(),
            agent_info: Vec::new(),
            task_log: Vec::new(),
            logs: VecDeque::new(),
            throughput_history: VecDeque::new(),
            crdt_counters: Vec::new(),
            crdt_registers: Vec::new(),
            crdt_sets: Vec::new(),
            active_workflows: Vec::new(),
            completed_workflows: VecDeque::new(),
            blackboard_entries: Vec::new(),
        }
    }

    pub fn add_log(&mut self, msg: String) {
        self.logs.push_back(msg);
        if self.logs.len() > 1000 {
            self.logs.pop_front();
        }
    }

    pub fn add_task_log(&mut self, entry: TaskLogEntry) {
        self.task_log.push(entry);
        if self.task_log.len() > 500 {
            self.task_log.remove(0);
        }
    }
}
