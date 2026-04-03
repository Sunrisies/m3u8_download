use axum::{
    Json,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use rust_embed::RustEmbed;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

use crate::server::state::{AppState, DownloadRequest, TaskStatus};

#[derive(RustEmbed)]
#[folder = "static/"]
struct StaticFiles;

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
}

pub async fn index() -> impl IntoResponse {
    let html = StaticFiles::get("index.html").map_or_else(
        || b"<!DOCTYPE html><html><body><h1>M3U8 Downloader Service</h1></body></html>".to_vec(),
        |file| file.data.to_vec(),
    );
    Html(String::from_utf8_lossy(&html).to_string())
}

pub async fn static_handler(Path(path): Path<String>) -> impl IntoResponse {
    let mime = mime_guess::from_path(&path).first_or_octet_stream();

    if let Some(file) = StaticFiles::get(&path) {
        Response::builder()
            .header("Content-Type", mime.as_ref())
            .body(axum::body::Body::from(file.data))
            .unwrap()
    } else {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(axum::body::Body::from("Not Found"))
            .unwrap()
    }
}

pub async fn start_download(
    State(state): State<AppState>,
    Json(request): Json<DownloadRequest>,
) -> impl IntoResponse {
    match state.add_task(request.clone()).await {
        Ok(task_id) => {
            let state_clone = state.clone();
            let task_id_clone = task_id.clone();

            tokio::spawn(async move {
                if let Err(e) = run_download_task(state_clone, task_id_clone, request).await {
                    log::error!("下载任务执行失败: {e}");
                }
            });

            (
                StatusCode::CREATED,
                Json(json!({
                    "id": task_id,
                    "status": "pending",
                    "message": "下载任务已创建"
                })),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": format!("创建任务失败: {}", e)
            })),
        ),
    }
}

async fn run_download_task(
    state: AppState,
    task_id: String,
    request: DownloadRequest,
) -> Result<(), String> {
    let _ = state
        .update_task_status(&task_id, TaskStatus::Downloading, None)
        .await;

    let output_dir = request.output_dir.unwrap_or_else(|| "./output".to_string());
    let download_dir = format!("./downloads/{}", request.name);

    // 创建目录
    tokio::fs::create_dir_all(&output_dir)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::create_dir_all(&download_dir)
        .await
        .map_err(|e| e.to_string())?;

    let output_name = request.name.clone();
    let args = crate::downloader::Args {
        url: request.url,
        output_name: request.name,
        concurrent: state.max_concurrent,
        retry: 4,
        download_dir,
        output_dir: output_dir.clone(),
        index: 1,
    };

    // 创建进度回调
    let state_clone = state.clone();
    let task_id_clone = task_id.clone();
    let callback: crate::utils::download_segment::ProgressCallback =
        Arc::new(move |progress: f64| {
            let state = state_clone.clone();
            let task_id = task_id_clone.clone();
            tokio::spawn(async move {
                let _ = state.update_task_progress(&task_id, progress).await;
            });
        });

    // 创建状态回调
    let state_clone2 = state.clone();
    let task_id_clone2 = task_id.clone();
    let status_callback: crate::utils::download_segment::StatusCallback =
        Arc::new(move |status: &str| {
            let state = state_clone2.clone();
            let task_id = task_id_clone2.clone();
            let status = status.to_string();
            tokio::spawn(async move {
                if status.as_str() == "merging" {
                    let _ = state
                        .update_task_status(&task_id, TaskStatus::Merging, None)
                        .await;
                }
            });
        });

    match crate::utils::download_segment::M3u8Downloader::new(args) {
        Ok(downloader) => {
            let downloader = downloader
                .with_progress_callback(callback)
                .with_status_callback(status_callback);
            match downloader.download().await {
                Ok(()) => {
                    let _ = state
                        .update_task_status(&task_id, TaskStatus::Completed, None)
                        .await;
                    let _ = state.update_task_progress(&task_id, 100.0).await;

                    // 获取输出文件信息
                    let output_file = format!("{output_dir}/{output_name}.mp4");
                    if let Ok(metadata) = tokio::fs::metadata(&output_file).await {
                        let _ = state
                            .update_task_output(&task_id, output_file, metadata.len())
                            .await;
                    }

                    log::info!("✅ 任务 {task_id} 下载完成");
                }
                Err(e) => {
                    let _ = state
                        .update_task_status(&task_id, TaskStatus::Failed, Some(e.to_string()))
                        .await;
                    log::error!("❌ 任务 {task_id} 下载失败: {e}");
                }
            }
        }
        Err(e) => {
            let _ = state
                .update_task_status(&task_id, TaskStatus::Failed, Some(e.to_string()))
                .await;
            log::error!("❌ 任务 {task_id} 创建下载器失败: {e}");
        }
    }

    Ok(())
}

pub async fn get_all_tasks(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    let tasks = if let Some(q) = query.q {
        if q.is_empty() {
            state.get_all_tasks().await // 条件为真时执行（q 为空）
        } else {
            state.search_tasks(&q).await // 条件为假时执行（q 非空）
        }
    } else {
        state.get_all_tasks().await
    };
    Json(tasks)
}

pub async fn get_task(Path(id): Path<String>, State(state): State<AppState>) -> impl IntoResponse {
    state.get_task(&id).await.map_or_else(
        || {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "任务不存在"
                })),
            )
        },
        |task| (StatusCode::OK, Json(json!(task))),
    )
}

pub async fn delete_task(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.delete_task(&id).await {
        Ok(true) => (StatusCode::OK, Json(json!({"message": "任务已删除"}))),
        Ok(false) => (StatusCode::NOT_FOUND, Json(json!({"error": "任务不存在"}))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("删除失败: {}", e)})),
        ),
    }
}

pub async fn get_pending_tasks(State(state): State<AppState>) -> impl IntoResponse {
    let tasks = state.get_tasks_by_status(TaskStatus::Pending).await;
    Json(tasks)
}

pub async fn get_completed_tasks(State(state): State<AppState>) -> impl IntoResponse {
    let tasks = state.get_tasks_by_status(TaskStatus::Completed).await;
    Json(tasks)
}

pub async fn get_failed_tasks(State(state): State<AppState>) -> impl IntoResponse {
    let tasks = state.get_tasks_by_status(TaskStatus::Failed).await;
    Json(tasks)
}

pub async fn get_stats(State(app_state): State<AppState>) -> impl IntoResponse {
    let stats = app_state.get_stats().await;
    Json(stats)
}

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_websocket(socket, id, state))
}

async fn handle_websocket(mut socket: WebSocket, task_id: String, state: AppState) {
    let mut interval = interval(Duration::from_millis(500));

    loop {
        tokio::select! {
            _ = interval.tick() => {
                if let Some(task) = state.get_task(&task_id).await {
                    let msg = serde_json::to_string(&task).unwrap();
                    if socket.send(Message::Text(msg)).await.is_err() {
                        break;
                    }

                    // 如果任务完成或失败，关闭连接
                    if task.status == TaskStatus::Completed || task.status == TaskStatus::Failed {
                        break;
                    }
                } else {
                    break;
                }
            }
            Some(Ok(msg)) = socket.recv() => {
                if let Message::Close(_) = msg {
                    break;
                }
            }
        }
    }
}
