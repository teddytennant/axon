use crate::state::{BlackboardEntry, SharedWebState};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Clone, Deserialize)]
pub struct SetBlackboardEntryRequest {
    pub value: String,
}

pub async fn list_entries(State(state): State<Arc<SharedWebState>>) -> Json<Vec<BlackboardEntry>> {
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
    Json(req): Json<SetBlackboardEntryRequest>,
) -> StatusCode {
    let ts = axon_core::util::now_ms();

    let mut ws = state.web_state.write().await;
    if let Some(entry) = ws.blackboard_entries.iter_mut().find(|e| e.key == key) {
        entry.value = req.value;
        entry.timestamp_ms = ts;
    } else {
        ws.blackboard_entries.push(BlackboardEntry {
            key,
            value: req.value,
            timestamp_ms: ts,
        });
    }
    StatusCode::OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_request_deserializes() {
        let req: SetBlackboardEntryRequest = serde_json::from_str(r#"{"value":"hello"}"#).unwrap();
        assert_eq!(req.value, "hello");
    }

    #[test]
    fn set_request_rejects_missing_value() {
        let res = serde_json::from_str::<SetBlackboardEntryRequest>(r#"{"other":1}"#);
        assert!(res.is_err());
    }
}
