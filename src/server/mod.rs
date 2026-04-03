mod handlers;
pub mod state;

use axum::{
    Router,
    routing::{delete, get, post, put},
};
use std::path::PathBuf;
use tower_http::cors::CorsLayer;

use state::AppState;

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(handlers::index))
        .route("/static/*path", get(handlers::static_handler))
        .route("/settings.html", get(handlers::settings_page))
        .route("/api/download", post(handlers::start_download))
        .route("/api/tasks", get(handlers::get_all_tasks))
        .route("/api/tasks/stats", get(handlers::get_stats))
        .route("/api/tasks/pending", get(handlers::get_pending_tasks))
        .route("/api/tasks/completed", get(handlers::get_completed_tasks))
        .route("/api/tasks/failed", get(handlers::get_failed_tasks))
        .route("/api/tasks/:id", get(handlers::get_task))
        .route("/api/tasks/:id", delete(handlers::delete_task))
        .route("/api/tasks/:id/ws", get(handlers::websocket_handler))
        .route("/api/settings", get(handlers::get_settings))
        .route("/api/settings", put(handlers::update_settings))
        .route("/api/browse", get(handlers::browse_directories))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

pub async fn start_server(host: &str, port: u16) -> anyhow::Result<()> {
    let data_file = PathBuf::from("./data/tasks.json");
    let settings_file = PathBuf::from("./data/settings.json");
    let state = AppState::new(data_file, settings_file);

    if let Err(e) = state.load().await {
        log::warn!("加载数据失败: {e}");
    }

    let app = create_router(state);

    let addr = format!("{host}:{port}");
    log::info!("🚀 服务器启动在 http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
