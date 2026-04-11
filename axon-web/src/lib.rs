pub mod api;
pub mod embed;
pub mod state;
pub mod ws;

pub use state::{AgentInfo, SharedWebState, TaskLogEntry, WebState};

use axum::Router;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

/// Start the web UI server. This is spawned as a background task from `run_node()`.
pub async fn start_web_server(state: Arc<SharedWebState>, port: u16) {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .merge(api::api_router())
        .route("/api/ws/live", axum::routing::get(ws::ws_live))
        .fallback(embed::static_handler)
        .layer(cors)
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Web UI listening on http://localhost:{}", port);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Failed to bind web UI on port {}: {}", port, e);
            return;
        }
    };

    if let Err(e) = axum::serve(listener, app).await {
        tracing::error!("Web UI server error: {}", e);
    }
}
