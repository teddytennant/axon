use crate::state::SharedWebState;
use axum::extract::State;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
pub struct StatusResponse {
    pub peer_id: String,
    pub listen_addr: String,
    pub uptime_secs: u64,
    pub peer_count: usize,
    pub agent_count: usize,
    pub tasks_total: u64,
    pub tasks_failed: u64,
    pub messages_received: u64,
    pub messages_sent: u64,
    pub provider: String,
    pub model: String,
    pub mcp_tool_count: usize,
    pub version: String,
}

pub async fn get_status(State(state): State<Arc<SharedWebState>>) -> Json<StatusResponse> {
    let ws = state.web_state.read().await;
    let peer_count = state.peer_table.read().await.all_peers_owned().len();
    let agent_count = state.runtime.agent_names().await.len();
    let mcp_tool_count = state.mcp_bridge.all_tools().await.len();

    Json(StatusResponse {
        peer_id: ws.peer_id.clone(),
        listen_addr: ws.listen_addr.clone(),
        uptime_secs: ws.uptime_secs,
        peer_count,
        agent_count,
        tasks_total: ws.tasks_total,
        tasks_failed: ws.tasks_failed,
        messages_received: ws.messages_received,
        messages_sent: ws.messages_sent,
        provider: ws.provider_name.clone(),
        model: ws.model_name.clone(),
        mcp_tool_count,
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}
