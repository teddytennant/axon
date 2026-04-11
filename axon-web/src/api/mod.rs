pub mod agents;
pub mod auth;
pub mod blackboard;
pub mod chat;
pub mod config;
pub mod mesh;
pub mod models;
pub mod status;
pub mod tasks;
pub mod tools;
pub mod trust;
pub mod workflows;

use crate::state::SharedWebState;
use axum::routing::{get, post, put};
use axum::Router;
use std::sync::Arc;

pub fn api_router() -> Router<Arc<SharedWebState>> {
    Router::new()
        .route("/api/status", get(status::get_status))
        .route("/api/mesh/peers", get(mesh::get_peers))
        .route("/api/agents", get(agents::get_agents))
        .route("/api/tasks/log", get(tasks::get_task_log))
        .route("/api/tasks/stats", get(tasks::get_task_stats))
        .route("/api/trust", get(trust::get_trust))
        .route("/api/trust/{peer_id}", get(trust::get_peer_trust))
        .route("/api/tools", get(tools::get_tools))
        .route("/api/tools/search", get(tools::search_tools))
        .route("/api/models/{provider}", get(models::get_models))
        .route(
            "/api/config",
            get(config::get_config).put(config::put_config),
        )
        .route("/api/config/llm", put(config::put_llm_config))
        .route("/api/auth/validate", post(auth::validate_key))
        .route("/api/auth/key/{provider}", put(auth::put_key))
        .route("/api/chat/completions", post(chat::completions))
        .route("/api/workflows", get(workflows::list_workflows))
        .route("/api/workflows/{id}", get(workflows::get_workflow))
        .route("/api/blackboard", get(blackboard::list_entries))
        .route(
            "/api/blackboard/{key}",
            get(blackboard::get_entry).put(blackboard::set_entry),
        )
}
