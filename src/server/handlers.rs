use axum::{
    Json,
    extract::{
        Path as AxumPath, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use std::{env::current_dir, path::Path};
use tokio::time::interval;

use futures::stream;
use tokio::sync::mpsc;
use bytes::Bytes;

use crate::downloader::M3u8Downloader;
use crate::downloader::Args as DownloadArgs;

use crate::config::WS_UPDATE_INTERVAL_MS;
use crate::server::state::{AppSettings, AppState, DownloadRequest, TaskStatus};

#[derive(Serialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

#[derive(Deserialize)]
pub struct BrowseQuery {
    pub path: Option<String>,
}

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

pub async fn settings_page() -> impl IntoResponse {
    let html = StaticFiles::get("settings.html").map_or_else(
        || b"<!DOCTYPE html><html><body><h1>Settings</h1></body></html>".to_vec(),
        |file| file.data.to_vec(),
    );
    Html(String::from_utf8_lossy(&html).to_string())
}

pub async fn static_handler(AxumPath(path): AxumPath<String>) -> impl IntoResponse {
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

pub async fn init_stream_download(
    State(state): State<AppState>,
    Json(request): Json<DownloadRequest>,
) -> impl IntoResponse {
    match state.add_task(request).await {
        Ok(task_id) => {
            log::info!("📝 直传任务已创建: {task_id}");
            (
                StatusCode::CREATED,
                Json(json!({
                    "id": task_id,
                    "status": "pending",
                    "download_url": format!("/api/download/stream/{}", task_id),
                    "message": "直传下载任务已创建"
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": format!("创建直传任务失败: {}", e)
            })),
        )
            .into_response(),
    }
}

fn create_task_callbacks(
    state: &AppState,
    task_id: &str,
) -> (
    crate::utils::download_segment::ProgressCallback,
    crate::utils::download_segment::StatusCallback,
) {
    let state_clone = state.clone();
    let task_id_clone = task_id.to_string();
    let callback: crate::utils::download_segment::ProgressCallback =
        Arc::new(move |progress: f64| {
            let state = state_clone.clone();
            let task_id = task_id_clone.clone();
            let normalized_progress = progress.clamp(0.0, 99.0);
            tokio::spawn(async move {
                let _ = state.update_task_progress(&task_id, normalized_progress).await;
            });
        });

    let state_clone2 = state.clone();
    let task_id_clone2 = task_id.to_string();
    let status_callback: crate::utils::download_segment::StatusCallback =
        Arc::new(move |status: &str| {
            let state = state_clone2.clone();
            let task_id = task_id_clone2.clone();
            let status = status.to_ascii_lowercase();
            tokio::spawn(async move {
                if status == "merging" {
                    let _ = state.update_task_progress(&task_id, 99.0).await;
                    let _ = state
                        .update_task_status(&task_id, TaskStatus::Merging, None)
                        .await;
                }
            });
        });

    (callback, status_callback)
}

async fn run_download_task(
    state: AppState,
    task_id: String,
    request: DownloadRequest,
) -> Result<(), String> {
    let _ = state
        .update_task_status(&task_id, TaskStatus::Downloading, None)
        .await;

    let settings = state.get_settings().await;
    log::info!(
        "📋 任务使用设置 - 下载目录: {}, 临时目录: {}, 并发: {}, 重试: {}",
        settings.download_dir,
        settings.temp_dir,
        settings.concurrent,
        settings.retry
    );

    let output_dir = request
        .output_dir
        .unwrap_or_else(|| settings.download_dir.clone());
    let download_dir = format!("{}/{}", settings.temp_dir, request.name);

    tokio::fs::create_dir_all(&output_dir)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::create_dir_all(&download_dir)
        .await
        .map_err(|e| e.to_string())?;

    log::info!("📁 输出目录: {output_dir}, 临时目录: {download_dir}");

    let output_name = request.name.clone();
    let args = crate::downloader::Args {
        url: request.url,
        output_name: request.name,
        concurrent: settings.concurrent,
        retry: settings.retry,
        download_dir,
        output_dir: output_dir.clone(),
        index: 1,
    };

    let (callback, status_callback) = create_task_callbacks(&state, &task_id);

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

async fn build_stream_download_response(
    state: AppState,
    task_id: String,
    request: DownloadRequest,
) -> Response {
    log::info!("🚀 直传任务开始响应: {task_id}");
    let settings = state.get_settings().await;
    let download_dir = format!("{}/stream_{}", settings.temp_dir, task_id);
    let output_dir = format!("{}/stream_{}_out", settings.temp_dir, task_id);

    let _ = state
        .update_task_status(&task_id, TaskStatus::Downloading, None)
        .await;

    if let Err(e) = tokio::fs::create_dir_all(&download_dir).await {
        let message = format!("创建临时目录失败: {}", e);
        let _ = state
            .update_task_status(&task_id, TaskStatus::Failed, Some(message.clone()))
            .await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": message })),
        )
            .into_response();
    }

    if let Err(e) = tokio::fs::create_dir_all(&output_dir).await {
        let message = format!("创建输出目录失败: {}", e);
        let _ = std::fs::remove_dir_all(&download_dir);
        let _ = state
            .update_task_status(&task_id, TaskStatus::Failed, Some(message.clone()))
            .await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": message })),
        )
            .into_response();
    }

    let (tx, rx) = mpsc::channel::<std::result::Result<Bytes, String>>(32);

    let args = DownloadArgs {
        url: request.url.clone(),
        output_name: request.name.clone(),
        concurrent: settings.concurrent,
        retry: settings.retry,
        download_dir: download_dir.clone(),
        output_dir: output_dir.clone(),
        index: 1,
    };

    let (callback, status_callback) = create_task_callbacks(&state, &task_id);

    match M3u8Downloader::new(args) {
        Ok(downloader) => {
            let state_clone = state.clone();
            let task_id_clone = task_id.clone();
            let download_dir_clone = download_dir.clone();
            let output_dir_clone = output_dir.clone();
            let error_tx = tx.clone();

            tokio::spawn(async move {
                log::info!("▶️ 直传任务开始后台下载: {task_id_clone}");
                let downloader = downloader
                    .with_progress_callback(callback)
                    .with_status_callback(status_callback)
                    .with_stream_output(tx);

                match downloader.download().await {
                    Ok(()) => {
                        let _ = state_clone.update_task_progress(&task_id_clone, 100.0).await;
                        let _ = state_clone
                            .update_task_status(&task_id_clone, TaskStatus::Completed, None)
                            .await;
                        log::info!("✅ 直传任务 {task_id_clone} 下载完成");
                    }
                    Err(e) => {
                        let message = e.to_string();
                        let _ = state_clone
                            .update_task_status(
                                &task_id_clone,
                                TaskStatus::Failed,
                                Some(message.clone()),
                            )
                            .await;
                        let _ = error_tx.send(Err(message.clone())).await;
                        log::error!("❌ 直传任务 {task_id_clone} 下载失败: {message}");
                    }
                }

                let _ = std::fs::remove_dir_all(&download_dir_clone);
                let _ = std::fs::remove_dir_all(&output_dir_clone);
            });

            let stream = stream::unfold(rx, |mut rx| async {
                rx.recv().await.map(|item| (item, rx))
            });

            let filename = format!("{}.mp4", request.name);
            Response::builder()
                .header("Content-Type", "video/mp4")
                .header(
                    "Content-Disposition",
                    format!("attachment; filename=\"{}\"", filename),
                )
                .header("X-Task-Id", &task_id)
                .body(axum::body::Body::from_stream(stream))
                .unwrap()
        }
        Err(e) => {
            let message = format!("创建下载器失败: {}", e);
            let _ = std::fs::remove_dir_all(&download_dir);
            let _ = std::fs::remove_dir_all(&output_dir);
            let _ = state
                .update_task_status(&task_id, TaskStatus::Failed, Some(message.clone()))
                .await;
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": message })),
            )
                .into_response()
        }
    }
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

pub async fn get_task(
    AxumPath(id): AxumPath<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
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
    AxumPath(id): AxumPath<String>,
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

pub async fn download_task_file(
    AxumPath(id): AxumPath<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.get_task(&id).await {
        Some(task_info) => {
            match &task_info.output_file {
                Some(output_path) => {
                    let path = std::path::Path::new(output_path);
                    if path.exists() {
                        match tokio::fs::read(path).await {
                            Ok(data) => {
                                let filename = path.file_name()
.and_then(|n| n.to_str())
.unwrap_or("download.mp4");
                                let content_type = mime_guess::from_path(path).first_or_octet_stream();
                                Response::builder()
.header("Content-Type", content_type.as_ref())
.header("Content-Disposition", format!("attachment; filename=\"{}\"", filename))
.header("Content-Length", data.len().to_string())
.body(axum::body::Body::from(data))
.unwrap()
                            }
                            Err(e) => {
                                (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("读取文件失败: {}", e)}))).into_response()
                            }
                        }
                    } else {
                        (StatusCode::NOT_FOUND, Json(json!({"error": "文件不存在"}))).into_response()
                    }
                }
                None => {
                    (StatusCode::BAD_REQUEST, Json(json!({"error": "任务尚未完成，无输出文件"}))).into_response()
                }
            }
        }
        None => {
            (StatusCode::NOT_FOUND, Json(json!({"error": "任务不存在"}))).into_response()
        }
    }
}

pub async fn stream_download_by_task(
    AxumPath(id): AxumPath<String>,
    State(state): State<AppState>,
) -> Response {
    let task = match state.get_task(&id).await {
        Some(task) => task,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "任务不存在"
                })),
            )
                .into_response();
        }
    };

    if task.status != TaskStatus::Pending {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "直传任务已启动或已结束，请重新创建任务"
            })),
        )
            .into_response();
    }

    let request = DownloadRequest {
        name: task.name,
        url: task.url,
        output_dir: None,
    };

    build_stream_download_response(state, id, request).await
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
    AxumPath(id): AxumPath<String>,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_websocket(socket, id, state))
}

async fn handle_websocket(mut socket: WebSocket, task_id: String, state: AppState) {
    let mut interval = interval(Duration::from_millis(WS_UPDATE_INTERVAL_MS));

    loop {
        tokio::select! {
            _ = interval.tick() => {
                if let Some(task) = state.get_task(&task_id).await {
                    let msg = serde_json::to_string(&task).unwrap();
                    if socket.send(Message::Text(msg)).await.is_err() {
                        break;
                    }

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

pub async fn get_settings(State(state): State<AppState>) -> impl IntoResponse {
    let settings = state.get_settings().await;
    log::info!(
        "🔧 获取设置: download_dir={}, concurrent={}",
        settings.download_dir,
        settings.concurrent
    );
    Json(settings)
}

pub async fn update_settings(
    State(state): State<AppState>,
    Json(new_settings): Json<AppSettings>,
) -> impl IntoResponse {
    log::info!(
        "🔧 收到设置更新请求: download_dir={}, concurrent={}, retry={}",
        new_settings.download_dir,
        new_settings.concurrent,
        new_settings.retry
    );

    match state.update_settings(new_settings.clone()).await {
        Ok(()) => {
            let saved = state.get_settings().await;
            log::info!(
                "✅ 设置更新成功: download_dir={}, concurrent={}",
                saved.download_dir,
                saved.concurrent
            );
            (StatusCode::OK, Json(json!({"message": "设置已保存"})))
        }
        Err(e) => {
            log::error!("❌ 设置更新失败: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("保存失败: {e}")})),
            )
        }
    }
}

pub async fn browse_directories(Query(query): Query<BrowseQuery>) -> impl IntoResponse {
    let current_path = query.path.unwrap_or_else(|| ".".to_string());
    let path = Path::new(&current_path);

    let mut entries = Vec::new();

    if path.is_relative() {
        if let Ok(absolute) = std::env::current_dir() {
            let full_path = absolute.join(path);
            let full_path = full_path.as_path();

            if let Ok(read_dir) = std::fs::read_dir(full_path) {
                let mut dirs: Vec<DirEntry> = Vec::new();
                let mut files: Vec<DirEntry> = Vec::new();

                for entry in read_dir.flatten() {
                    let entry_path = entry.path();
                    let name = entry.file_name().to_string_lossy().to_string();

                    if name.starts_with('.') {
                        continue;
                    }

                    let is_dir = entry_path.is_dir();
                    let full_path_str = entry_path.to_string_lossy().to_string();

                    let dir_entry = DirEntry {
                        name,
                        path: full_path_str,
                        is_dir,
                    };

                    if is_dir {
                        dirs.push(dir_entry);
                    } else {
                        files.push(dir_entry);
                    }
                }

                dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

                entries.extend(dirs);
                entries.extend(files);
            }
        }
    } else if path.is_absolute()
        && path.exists()
        && path.is_dir()
        && let Ok(read_dir) = std::fs::read_dir(path)
    {
        let mut dirs: Vec<DirEntry> = Vec::new();
        let mut files: Vec<DirEntry> = Vec::new();

        for entry in read_dir.flatten() {
            let entry_path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            if name.starts_with('.') {
                continue;
            }

            let is_dir = entry_path.is_dir();
            let full_path_str = entry_path.to_string_lossy().to_string();

            let dir_entry = DirEntry {
                name,
                path: full_path_str,
                is_dir,
            };

            if is_dir {
                dirs.push(dir_entry);
            } else {
                files.push(dir_entry);
            }
        }

        dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        entries.extend(dirs);
        entries.extend(files);
    }

    let parent_path = if path.is_relative() {
        current_dir().map_or_else(
            |_| String::new(),
            |absolute| {
                let full_path = absolute.join(path);
                full_path
                    .parent()
                    .map_or_else(String::new, |parent| parent.to_string_lossy().to_string())
            },
        )
    } else if path.is_absolute() {
        path.parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    } else {
        String::new()
    };

    let current_path_str = if path.is_relative() {
        current_dir().map_or_else(
            |_| current_path.clone(),
            |absolute| absolute.join(path).to_string_lossy().to_string(),
        )
    } else {
        current_path.clone()
    };

    Json(json!({
        "current_path": current_path_str,
        "parent_path": parent_path,
        "entries": entries
    }))
}

pub async fn stream_download(
    State(state): State<AppState>,
    Json(request): Json<DownloadRequest>,
) -> impl IntoResponse {
    match state.add_task(request.clone()).await {
        Ok(task_id) => build_stream_download_response(state, task_id, request).await,
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": format!("创建直传任务失败: {}", e)
            })),
        )
            .into_response(),
    }
}


