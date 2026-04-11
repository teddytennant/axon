use crate::state::SharedWebState;
use axum::extract::State;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
pub struct PeerResponse {
    pub peer_id: String,
    pub addr: String,
    pub capabilities: Vec<String>,
    pub last_seen: u64,
    pub last_seen_ago: String,
}

pub async fn get_peers(State(state): State<Arc<SharedWebState>>) -> Json<Vec<PeerResponse>> {
    let peers = state.peer_table.read().await.all_peers_owned();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let result: Vec<PeerResponse> = peers
        .into_iter()
        .map(|p| {
            let diff = now.saturating_sub(p.last_seen);
            let ago = if diff < 60 {
                format!("{}s ago", diff)
            } else if diff < 3600 {
                format!("{}m ago", diff / 60)
            } else {
                format!("{}h ago", diff / 3600)
            };
            PeerResponse {
                peer_id: hex::encode(&p.peer_id),
                addr: p.addr,
                capabilities: p.capabilities.iter().map(|c| c.tag()).collect(),
                last_seen: p.last_seen,
                last_seen_ago: ago,
            }
        })
        .collect();

    Json(result)
}
