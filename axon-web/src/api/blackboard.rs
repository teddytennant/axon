use crate::state::{BlackboardEntry, SharedWebState};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use std::sync::Arc;

pub async fn list_entries(
    State(state): State<Arc<SharedWebState>>,
) -> Json<Vec<BlackboardEntry>> {
    let ws = state.web_state.read().await;
    Json(ws.blackboard_entries.clone())
}

pub async fn get_entry(
    State(state): State<Arc<SharedWebState>>,
    axum::extract::Path(key): axum::extract::Path<String>,
) -> Result<Json<BlackboardEntry>, StatusCode> {
    let ws = state.web_state.read().await;
    ws.blackboard_entries
        .iter()
        .find(|e| e.key == key)
        .cloned()
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

pub async fn set_entry(
    State(state): State<Arc<SharedWebState>>,
    axum::extract::Path(key): axum::extract::Path<String>,
    Json(body): Json<serde_json::Value>,
) -> StatusCode {
    let value = match body.get("value").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => return StatusCode::BAD_REQUEST,
    };
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let mut ws = state.web_state.write().await;
    if let Some(entry) = ws.blackboard_entries.iter_mut().find(|e| e.key == key) {
        entry.value = value;
        entry.timestamp_ms = ts;
    } else {
        ws.blackboard_entries.push(BlackboardEntry {
            key,
            value,
            timestamp_ms: ts,
        });
    }
    StatusCode::OK
}
