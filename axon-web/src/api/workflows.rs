use crate::state::{SharedWebState, WorkflowSnapshot};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use std::sync::Arc;

pub async fn list_workflows(State(state): State<Arc<SharedWebState>>) -> Json<serde_json::Value> {
    let ws = state.web_state.read().await;
    Json(serde_json::json!({
        "active": ws.active_workflows,
        "completed": ws.completed_workflows.iter().collect::<Vec<&WorkflowSnapshot>>(),
    }))
}

pub async fn get_workflow(
    State(state): State<Arc<SharedWebState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<WorkflowSnapshot>, StatusCode> {
    let ws = state.web_state.read().await;
    if let Some(wf) = ws.active_workflows.iter().find(|w| w.id == id) {
        return Ok(Json(wf.clone()));
    }
    if let Some(wf) = ws.completed_workflows.iter().find(|w| w.id == id) {
        return Ok(Json(wf.clone()));
    }
    Err(StatusCode::NOT_FOUND)
}
