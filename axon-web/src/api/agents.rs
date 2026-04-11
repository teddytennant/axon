use crate::state::SharedWebState;
use axum::extract::State;
use axum::Json;
use std::sync::Arc;

pub async fn get_agents(
    State(state): State<Arc<SharedWebState>>,
) -> Json<Vec<crate::state::AgentInfo>> {
    let ws = state.web_state.read().await;
    Json(ws.agent_info.clone())
}
