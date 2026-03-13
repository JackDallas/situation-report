use std::path::PathBuf;

use axum::Router;
use tower_http::services::{ServeDir, ServeFile};

use crate::state::AppState;

/// Create a router that serves the built SvelteKit SPA from a directory.
/// Falls back to index.html for client-side routing.
pub fn static_file_router(static_dir: PathBuf) -> Router<AppState> {
    let index = static_dir.join("index.html");
    Router::new().fallback_service(
        ServeDir::new(&static_dir).fallback(ServeFile::new(index)),
    )
}
