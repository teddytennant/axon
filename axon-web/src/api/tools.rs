use crate::state::SharedWebState;
use axon_core::{SchemaDetail, ToolFilter};
use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Deserialize)]
pub struct ToolSearchParams {
    pub q: Option<String>,
    pub server: Option<String>,
    pub limit: Option<usize>,
    pub budget: Option<usize>,
}

#[derive(Serialize)]
pub struct ToolResponse {
    pub name: String,
    pub server: String,
    pub description: String,
    pub peer_id: String,
    pub score: f64,
}

pub async fn get_tools(State(state): State<Arc<SharedWebState>>) -> Json<Vec<ToolResponse>> {
    let reg = state.tool_registry.read().await;
    let filter = ToolFilter {
        query: None,
        server_filter: None,
        limit: 100,
        max_tokens: None,
        detail: SchemaDetail::Full,
    };
    let results = reg.search(&filter);
    let tools: Vec<ToolResponse> = results
        .into_iter()
        .map(|r| ToolResponse {
            name: r.tool.name.clone(),
            server: r.tool.server_name.clone(),
            description: r.tool.description.clone(),
            peer_id: r.peer_id_hex.clone(),
            score: r.score,
        })
        .collect();
    Json(tools)
}

pub async fn search_tools(
    State(state): State<Arc<SharedWebState>>,
    Query(params): Query<ToolSearchParams>,
) -> Json<Vec<ToolResponse>> {
    let reg = state.tool_registry.read().await;
    let filter = ToolFilter {
        query: params.q,
        server_filter: params.server,
        limit: params.limit.unwrap_or(20),
        max_tokens: params.budget,
        detail: SchemaDetail::Full,
    };
    let results = reg.search(&filter);
    let tools: Vec<ToolResponse> = results
        .into_iter()
        .map(|r| ToolResponse {
            name: r.tool.name.clone(),
            server: r.tool.server_name.clone(),
            description: r.tool.description.clone(),
            peer_id: r.peer_id_hex.clone(),
            score: r.score,
        })
        .collect();
    Json(tools)
}
