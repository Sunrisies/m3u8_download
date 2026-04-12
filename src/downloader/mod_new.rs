mod encryption;
mod segment;
pub use encryption::{decrypt_segment, extract_encryption_key};
use futures::{StreamExt, stream};
pub use segment::merge_segments;

use std::path::{Path, PathBuf};

use clap::Parser;
use log::{error, info};
use tokio::{fs, time::Instant};

use crate::config::*;
use crate::error::{Result, DownloadError};
use crate::utils::{DownloadTask, download_segment::M3u8Downloader, is_already_downloaded};

#[derive(Parser)]
pub struct Args {
    /// M3U8 播放列表 URL
    pub url: String,

    /// 输出文件名（不包含扩展名）
    pub output_name: String,

    /// 并发下载数
    pub concurrent: usize,

    /// 重试次数
    pub retry: usize,

    /// 下载目录
    pub download_dir: String,
    /// 输出目录
    pub output_dir: String,
    /// 下载任务索引
    pub index: usize,
}
#[derive(Clone)]
pub struct DownloadStats {
    pub total_segments: usize,
    pub completed_segments: usize,
    pub downloaded_bytes: u64,
    pub start_time: Instant,
}

impl DownloadStats {
    pub fn new(total_segments: usize) -> Self {
        Self {
            total_segments,
            completed_segments: 0,
            downloaded_bytes: 0,
            start_time: Instant::now(),
        }
    }
    #[allow(clippy::cast_precision_loss)]
    pub fn get_speed(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.downloaded_bytes as f64 / elapsed
        } else {
            0.0
        }
    }
    #[allow(clippy::cast_precision_loss)]
    pub fn get_progress_percentage(&self) -> f64 {
        if self.total_segments > 0 {
            (self.completed_segments as f64 / self.total_segments as f64) * 100.0
        } else {
            0.0
        }
    }
}

/// 处理单个下载任务（并发版）
pub async fn process_download_task(
    task: &DownloadTask,
    max_concurrent: usize,
    index: usize,
) -> Result<()> {
    // 确定输出目录
    let output_dir = if task.output_dir.is_empty() {
        "./output".to_string()
    } else {
        format!("{}/{}", task.output_dir, task.name)
    };
    // 创建输出目录
    if !Path::new(&output_dir).exists() {
        fs::create_dir_all(&output_dir).await?;
    }

    // 确定下载目录（用于存储分段文件）
    let download_dir = format!("./downloads/{}", task.name);
    if !Path::new(&download_dir).exists() {
        fs::create_dir_all(&download_dir).await?;
    }

    let args = Args {
        url: task.url.clone(),
        output_name: task.name.clone(),
        download_dir,
        concurrent: max_concurrent,
        retry: DEFAULT_RETRY_COUNT,
        output_dir,
        index,
    };
    match M3u8Downloader::new(args) {
        Ok(downloader) => {
            downloader.download().await
                .map_err(|e| {
                    error!("❌ 下载失败: {e}");
                    e
                })?;
            info!("✅ 下载成功完成！");
            Ok(())
        }
        Err(e) => {
            error!("❌ 创建下载器失败: {e}");
            Err(e)
        }
    }
}

/// 处理多个下载任务（并发版）
pub async fn process_download_tasks(
    tasks: &[DownloadTask],
    max_concurrent: usize,
) -> Result<()> {
    info!(
        "正在处理{}个下载任务，最大并发数: {}",
        tasks.len(),
        max_concurrent
    );

    let mut failed_tasks = Vec::new();
    let mut successful_tasks = Vec::new();
    let mut skipped_tasks = Vec::new();

    let download_dir = PathBuf::from("./output");
    if !download_dir.exists() {
        tokio::fs::create_dir_all(&download_dir).await?;
    }

    // 预先检查哪些任务需要跳过
    let tasks_to_download: Vec<_> = tasks
        .iter()
        .enumerate()
        .filter(|(i, task)| {
            if is_already_downloaded(task, &download_dir) {
                info!(
                    "⏭️ 任务 {}/{}: {} 已存在，跳过",
                    i + 1,
                    tasks.len(),
                    task.name
                );
                skipped_tasks.push(task.name.clone());
                false
            } else {
                true
            }
        })
        .collect();

    if tasks_to_download.is_empty() {
        info!("所有任务都已存在，无需下载");
        return Ok(());
    }

    // 创建并发流：同时启动最多 max_concurrent 个任务
    let mut stream = stream::iter(tasks_to_download.into_iter())
        .map(|(i, task)| {
            let name = task.name.clone();
            async move {
                info!(
                    "正在启动任务 {}/{},当前任务是:{}",
                    i + 1,
                    tasks.len(),
                    task.name
                );
                let result = process_download_task(task, max_concurrent, i + 1).await;
                (i, name, result)
            }
        })
        .buffer_unordered(max_concurrent);

    // 收集结果
    while let Some((_i, name, result)) = stream.next().await {
        match result {
            Ok(()) => {
                successful_tasks.push(name.clone());
                info!("✅ 任务 {name} 处理成功");
            }
            Err(e) => {
                let error_info = format!("任务 '{name}' 失败: {e}");
                error!("❌ {error_info}");
                failed_tasks.push((name, error_info));
            }
        }
    }

    // 输出处理结果统计
    info!("
===== 处理结果统计 =====");
    info!("总任务数: {}", tasks.len());
    info!("成功任务数: {}", successful_tasks.len());
    info!("失败任务数: {}", failed_tasks.len());
    info!("跳过任务数: {}", skipped_tasks.len());

    // 新增跳过列表
    if !skipped_tasks.is_empty() {
        info!("
===== 跳过任务列表 =====");
        for name in &skipped_tasks {
            info!("⏭️ {name} (文件已存在)");
        }
    }

    if !failed_tasks.is_empty() {
        info!("
===== 失败任务列表 =====");
        for (name, error) in &failed_tasks {
            info!("❌ {name}: {error}");
        }
    }

    if !successful_tasks.is_empty() {
        info!("
===== 成功任务列表 =====");
        for name in &successful_tasks {
            info!("✅ {name}");
        }
    }

    // 如果所有任务都失败，返回错误
    if failed_tasks.len() == tasks.len() {
        return Err(DownloadError::task("批量任务", "所有任务都失败".to_string()));
    }

    Ok(())
}
