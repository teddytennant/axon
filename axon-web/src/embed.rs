use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "frontend/dist"]
struct FrontendAssets;

/// Serve embedded frontend static files, falling back to index.html for SPA routing.
pub async fn static_handler(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Try exact file match first
    if let Some(file) = FrontendAssets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime.as_ref())],
            file.data.into_owned(),
        )
            .into_response()
    } else if let Some(file) = FrontendAssets::get("index.html") {
        // SPA fallback
        Html(String::from_utf8_lossy(&file.data).into_owned()).into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}
