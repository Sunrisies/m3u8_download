mod handlers;
pub mod state;

use axum::{
    Router,
    routing::{delete, get, post},
};
use std::path::PathBuf;
use tower_http::cors::CorsLayer;

use state::AppState;

pub fn create_router(state: AppState) -> Router {
    Router::new()
        // 静态文件
        .route("/", get(handlers::index))
        .route("/static/*path", get(handlers::static_handler))
        // API 路由
        .route("/api/download", post(handlers::start_download))
        .route("/api/tasks", get(handlers::get_all_tasks))
        .route("/api/tasks/stats", get(handlers::get_stats))
        .route("/api/tasks/pending", get(handlers::get_pending_tasks))
        .route("/api/tasks/completed", get(handlers::get_completed_tasks))
        .route("/api/tasks/failed", get(handlers::get_failed_tasks))
        .route("/api/tasks/:id", get(handlers::get_task))
        .route("/api/tasks/:id", delete(handlers::delete_task))
        .route("/api/tasks/:id/ws", get(handlers::websocket_handler))
        // 中间件
        .layer(CorsLayer::permissive())
        .with_state(state)
}

pub async fn start_server(host: &str, port: u16, max_concurrent: usize) -> anyhow::Result<()> {
    let data_file = PathBuf::from("./data/tasks.json");
    let state = AppState::new(max_concurrent, data_file);

    // 加载历史数据
    if let Err(e) = state.load().await {
        log::warn!("加载历史数据失败: {e}");
    }

    let app = create_router(state);

    let addr = format!("{host}:{port}");
    log::info!("🚀 服务器启动在 http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
