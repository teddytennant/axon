use crate::state::{SharedWebState, WorkflowSnapshot};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowsResponse {
    pub active: Vec<WorkflowSnapshot>,
    pub completed: Vec<WorkflowSnapshot>,
}

pub async fn list_workflows(State(state): State<Arc<SharedWebState>>) -> Json<WorkflowsResponse> {
    let ws = state.web_state.read().await;
    Json(WorkflowsResponse {
        active: ws.active_workflows.clone(),
        completed: ws.completed_workflows.iter().cloned().collect(),
    })
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
