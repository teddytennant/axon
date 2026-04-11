use crate::state::SharedWebState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
pub struct TrustEntry {
    pub peer_id: String,
    pub reliability: f64,
    pub accuracy: f64,
    pub availability: f64,
    pub quality: f64,
    pub overall: f64,
    pub confidence: f64,
    pub observation_count: usize,
}

pub async fn get_trust(State(state): State<Arc<SharedWebState>>) -> Json<Vec<TrustEntry>> {
    let ts = state.trust_store.lock().await;
    let ranked = ts.ranked_peers();
    let entries: Vec<TrustEntry> = ranked
        .into_iter()
        .map(|(peer_id, score)| TrustEntry {
            peer_id: hex::encode(&peer_id),
            reliability: score.reliability,
            accuracy: score.accuracy,
            availability: score.availability,
            quality: score.quality,
            overall: score.overall,
            confidence: score.confidence,
            observation_count: score.observation_count,
        })
        .collect();
    Json(entries)
}

pub async fn get_peer_trust(
    State(state): State<Arc<SharedWebState>>,
    Path(peer_id): Path<String>,
) -> Result<Json<TrustEntry>, StatusCode> {
    let peer_bytes = hex::decode(&peer_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let ts = state.trust_store.lock().await;
    let score = ts.score(&peer_bytes);
    Ok(Json(TrustEntry {
        peer_id,
        reliability: score.reliability,
        accuracy: score.accuracy,
        availability: score.availability,
        quality: score.quality,
        overall: score.overall,
        confidence: score.confidence,
        observation_count: score.observation_count,
    }))
}
