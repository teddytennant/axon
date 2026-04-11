use crate::state::SharedWebState;
use axum::extract::State;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
pub struct TaskStatsResponse {
    pub pending: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub timed_out: usize,
    pub total: usize,
}

pub async fn get_task_stats(State(state): State<Arc<SharedWebState>>) -> Json<TaskStatsResponse> {
    let stats = state.task_queue.stats().unwrap_or_default();
    Json(TaskStatsResponse {
        pending: stats.pending,
        running: stats.running,
        completed: stats.completed,
        failed: stats.failed,
        timed_out: stats.timed_out,
        total: stats.total(),
    })
}

pub async fn get_task_log(
    State(state): State<Arc<SharedWebState>>,
) -> Json<Vec<crate::state::TaskLogEntry>> {
    let ws = state.web_state.read().await;
    Json(ws.task_log.clone())
}
