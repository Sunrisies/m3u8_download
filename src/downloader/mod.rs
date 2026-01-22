mod encryption;
mod segment;
pub use encryption::{decrypt_segment, extract_encryption_key};
pub use segment::merge_segments;

use futures::stream::{self, StreamExt};
use std::path::{Path, PathBuf};

use clap::Parser;
use log::{error, info};
use tokio::{fs, time::Instant};

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

    pub fn get_speed(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.downloaded_bytes as f64 / elapsed
        } else {
            0.0
        }
    }

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
) -> Result<(), String> {
    // 确定输出目录
    let output_dir = if task.output_dir.is_empty() {
        format!("./output")
    } else {
        format!("{}/{}", task.output_dir, task.name)
    };
    // 创建输出目录
    if !Path::new(&output_dir).exists() {
        fs::create_dir_all(&output_dir)
            .await
            .map_err(|e| format!("Failed to create output directory: {}", e))?;
    }

    // 确定下载目录（用于存储分段文件）
    let download_dir = format!("./downloads/{}", task.name);
    if !Path::new(&download_dir).exists() {
        fs::create_dir_all(&download_dir)
            .await
            .map_err(|e| format!("Failed to create download directory: {}", e))?;
    }

    let args = Args {
        url: task.url.clone(),
        output_name: task.name.clone(),
        download_dir: download_dir,
        concurrent: max_concurrent,
        retry: 4,
        output_dir: output_dir,
        index: index,
    };
    match M3u8Downloader::new(args) {
        Ok(downloader) => match downloader.download().await {
            Ok(_) => {
                info!("✅ 下载成功完成！");
                Ok(())
            }
            Err(e) => {
                error!("❌ 下载失败: {}", e);
                Err(format!("下载失败: {}", e))
            }
        },
        Err(e) => {
            error!("❌ 创建下载器失败: {}", e);
            Err(format!("创建下载器失败: {}", e))
        }
    }

    // Ok(())
}

// /// 处理多个下载任务（并发版）
pub async fn process_download_tasks(
    tasks: &[DownloadTask],
    max_concurrent: usize,
) -> Result<(), String> {
    info!(
        "正在处理具有{}个并发下载的{}个下载任务",
        tasks.len(),
        max_concurrent
    );
    let mut failed_tasks = Vec::new();
    let mut successful_tasks = Vec::new();
    let download_dir = PathBuf::from("./output");
    if !download_dir.exists() {
        // 注意：fs::create_dir_all 接受 AsRef<Path>，PathBuf 实现了它
        // 如果是在 async 上下文中，且使用 tokio::fs，请确保引用正确
        // 这里假设是 tokio::fs，因为前面有 .await
        tokio::fs::create_dir_all(&download_dir)
            .await
            .map_err(|e| format!("Failed to create download directory: {}", e))?;
    }
    let mut skipped_tasks = Vec::new();
    for (i, task) in tasks.iter().enumerate() {
        // ← 新增：检查是否已存在
        if is_already_downloaded(task, &download_dir) {
            info!(
                "⏭️ 任务 {}/{}: {} 已存在，跳过",
                i + 1,
                tasks.len(),
                task.name
            );
            skipped_tasks.push(task.name.clone());
            continue; // ← 关键：跳过下载
        }
        info!(
            "正在启动任务 {}/{},当前任务是:{}",
            i + 1,
            tasks.len(),
            task.name
        );
        match process_download_task(task, max_concurrent, i + 1).await {
            Ok(_) => {
                successful_tasks.push(task.name.clone());
                info!("✅ 任务 {} 处理成功", task.name);
            }
            Err(e) => {
                let error_info = format!("任务 '{}' 失败: {}", task.name, e);
                error!("❌ {}", error_info);
                failed_tasks.push((task.name.clone(), error_info));
            }
        }
    }
    // info!(
    //     "正在处理{}个下载任务，最大并发数: {}",
    //     tasks.len(),
    //     max_concurrent
    // );

    // let mut failed_tasks = Vec::new();
    // let mut successful_tasks = Vec::new();

    // // 创建并发流：同时启动最多 max_concurrent 个任务
    // let mut stream = stream::iter(tasks.iter().enumerate())
    //     .map(|(i, task)| {
    //         let name = task.name.clone();
    //         async move {
    //             // 这里才是真正的并发执行
    //             let result = process_download_task(task, max_concurrent).await;
    //             (i, name, result)
    //         }
    //     })
    //     .buffer_unordered(max_concurrent);

    // // 收集结果
    // while let Some((i, name, result)) = stream.next().await {
    //     match result {
    //         Ok(_) => {
    //             successful_tasks.push(name.clone());
    //             info!("✅ 任务 {} 处理成功", name);
    //         }
    //         Err(e) => {
    //             let error_info = format!("任务 '{}' 失败: {}", name, e);
    //             error!("❌ {}", error_info);
    //             failed_tasks.push((name, error_info));
    //         }
    //     }
    // }

    // 输出处理结果统计
    info!("\n===== 处理结果统计 =====");
    info!("总任务数: {}", tasks.len());
    info!("成功任务数: {}", successful_tasks.len());
    info!("失败任务数: {}", failed_tasks.len());
    info!("跳过任务数: {}", skipped_tasks.len()); // ← 新增

    // 新增跳过列表
    if !skipped_tasks.is_empty() {
        info!("\n===== 跳过任务列表 =====");
        for name in &skipped_tasks {
            info!("⏭️ {} (文件已存在)", name);
        }
    }

    if !failed_tasks.is_empty() {
        info!("\n===== 失败任务列表 =====");
        for (name, error) in &failed_tasks {
            info!("❌ {}: {}", name, error);
        }
    }

    if !successful_tasks.is_empty() {
        info!("\n===== 成功任务列表 =====");
        for name in &successful_tasks {
            info!("✅ {}", name);
        }
    }

    // 如果所有任务都失败，返回错误
    if failed_tasks.len() == tasks.len() {
        return Err("所有任务都失败了".to_string());
    }

    Ok(())
}
