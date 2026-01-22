use anyhow::{Result, anyhow};
use clap::Parser;
use futures::future::join_all;
use indicatif::{ProgressBar, ProgressStyle};
use log::{error, info};
use m3u8_rs::{MediaPlaylist, MediaSegment};
use reqwest::Client;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::time::sleep;
use url::Url;

// AES解密相关
use aes::Aes128;
use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};

use crate::utils::is_valid_ts_file;
use crate::utils::json_loader::{DownloadTask, load_download_tasks_from_json};

type Aes128CbcDec = cbc::Decryptor<Aes128>;
/// 处理单个下载任务（并发版）
pub async fn process_download_task(
    task: &DownloadTask,
    max_concurrent: usize,
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
            .map_err(|e| format!("Failed to create output directory: {}", e))?;
    }

    // 确定下载目录（用于存储分段文件）
    let download_dir = format!("./downloads/{}", task.name);
    if !Path::new(&download_dir).exists() {
        fs::create_dir_all(&download_dir)
            .map_err(|e| format!("Failed to create download directory: {}", e))?;
    }

    let args = Args {
        url: task.url.clone(),
        output_name: task.name.clone(),
        download_dir: download_dir,
        concurrent: max_concurrent,
        retry: 4,
        output_dir: output_dir,
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

    for (i, task) in tasks.iter().enumerate() {
        info!(
            "正在启动任务 {}/{},当前任务是:{}",
            i + 1,
            tasks.len(),
            task.name
        );
        match process_download_task(task, max_concurrent).await {
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

    // 输出处理结果统计
    info!("\n===== 处理结果统计 =====");
    info!("总任务数: {}", tasks.len());
    info!("成功任务数: {}", successful_tasks.len());
    info!("失败任务数: {}", failed_tasks.len());

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

/// 从JSON文件加载并处理下载任务（并发版）
pub async fn load_and_process_download_tasks(
    json_path: &str,
    max_concurrent: usize,
) -> Result<(), String> {
    // 加载下载任务
    let tasks = load_download_tasks_from_json(json_path)?;
    // 处理下载任务
    process_download_tasks(&tasks, max_concurrent).await
}

#[derive(Parser)]
struct Args {
    /// M3U8 播放列表 URL
    url: String,

    /// 输出文件名（不包含扩展名）
    output_name: String,

    /// 并发下载数
    concurrent: usize,

    /// 重试次数
    retry: usize,

    /// 下载目录
    download_dir: String,
    /// 输出目录
    output_dir: String,
}

#[derive(Clone)]
struct DownloadStats {
    total_segments: usize,
    completed_segments: usize,
    downloaded_bytes: u64,
    start_time: Instant,
}

impl DownloadStats {
    fn new(total_segments: usize) -> Self {
        Self {
            total_segments,
            completed_segments: 0,
            downloaded_bytes: 0,
            start_time: Instant::now(),
        }
    }

    fn get_speed(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.downloaded_bytes as f64 / elapsed
        } else {
            0.0
        }
    }

    fn get_progress_percentage(&self) -> f64 {
        if self.total_segments > 0 {
            (self.completed_segments as f64 / self.total_segments as f64) * 100.0
        } else {
            0.0
        }
    }
}

struct M3u8Downloader {
    client: Client,
    base_url: Url,
    download_dir: PathBuf,
    concurrent: usize,
    retry: usize,
    stats: Arc<tokio::sync::Mutex<DownloadStats>>,
    progress_bar: ProgressBar,
    output_filename: String,
    output_dir: PathBuf,
}

impl M3u8Downloader {
    fn new(args: Args) -> Result<Self> {
        let base_url = Url::parse(&args.url)?;
        let download_dir = PathBuf::from(&args.download_dir);
        let output_dir = PathBuf::from(&args.output_dir);
        // 创建下载目录
        if !download_dir.exists() {
            fs::create_dir_all(&download_dir)?;
        }
        if !output_dir.exists() {
            fs::create_dir_all(&output_dir)?;
        }

        let client = Client::builder().timeout(Duration::from_secs(30)).build()?;

        let progress_bar = ProgressBar::new(100);
        progress_bar.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) {msg}")
                .unwrap()
                .progress_chars("=>-"),
        );

        Ok(Self {
            client,
            base_url,
            download_dir,
            concurrent: args.concurrent,
            retry: args.retry,
            stats: Arc::new(tokio::sync::Mutex::new(DownloadStats::new(0))),
            progress_bar,
            output_filename: args.output_name,
            output_dir,
        })
    }

    async fn download(&self) -> Result<()> {
        info!("正在获取 M3U8 播放列表...");

        // 下载并解析 M3U8 文件
        let m3u8_content = self.download_text(&self.base_url.to_string()).await?;
        let playlist = self.parse_m3u8(&m3u8_content)?;

        info!("发现 {} 个视频片段", playlist.segments.len());

        // 更新统计信息
        {
            let mut stats = self.stats.lock().await;
            stats.total_segments = playlist.segments.len();
        }

        self.progress_bar.set_length(playlist.segments.len() as u64);

        // 检查加密 - 从播放列表内容中提取密钥信息
        let key_data = self.extract_encryption_key(&m3u8_content).await?;

        if key_data.is_some() {
            info!("检测到加密流，已获取密钥");
        }

        // 并行下载片段
        let segments = Arc::new(playlist.segments);
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.concurrent));

        let download_tasks: Vec<_> = (0..segments.len())
            .map(|i| {
                let downloader = self.clone();
                let segments = segments.clone();
                let key_data = key_data.clone();
                let semaphore = semaphore.clone();

                tokio::spawn(async move {
                    let _permit = semaphore.acquire().await.unwrap();
                    downloader
                        .download_segment(i, &segments[i], key_data.as_ref())
                        .await
                })
            })
            .collect();

        // 等待所有下载完成
        let results = join_all(download_tasks).await;

        // 检查是否有下载失败
        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => return Err(anyhow!("片段 {} 下载失败: {}", i, e)),
                Err(e) => return Err(anyhow!("片段 {} 任务失败: {}", i, e)),
            }
        }

        self.progress_bar.finish_with_message("所有片段下载完成");

        // 合并文件
        info!("正在合并视频文件...");
        self.merge_segments(&segments).await?;

        info!(
            "下载完成！输出文件: {}/{}.ts",
            self.download_dir.display(),
            self.output_filename
        );
        Ok(())
    }

    async fn extract_encryption_key(&self, m3u8_content: &str) -> Result<Option<Vec<u8>>> {
        // 查找 EXT-X-KEY 标签
        for line in m3u8_content.lines() {
            if line.starts_with("#EXT-X-KEY:") {
                if let Some(uri_start) = line.find("URI=\"") {
                    let uri_start = uri_start + 5; // "URI=\"的长度
                    if let Some(uri_end) = line[uri_start..].find("\"") {
                        let key_uri = &line[uri_start..uri_start + uri_end];
                        return Ok(Some(self.download_key(key_uri).await?));
                    }
                }
            }
        }
        Ok(None)
    }

    async fn download_text(&self, url: &str) -> Result<String> {
        let full_url = self.resolve_url(url)?;
        let response = self.client.get(full_url).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!("HTTP 错误: {}", response.status()));
        }

        Ok(response.text().await?)
    }

    async fn download_key(&self, key_uri: &str) -> Result<Vec<u8>> {
        let full_url = self.resolve_url(key_uri)?;
        let response = self.client.get(full_url).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!("密钥下载失败: {}", response.status()));
        }

        Ok(response.bytes().await?.to_vec())
    }

    async fn download_segment(
        &self,
        index: usize,
        segment: &MediaSegment,
        key: Option<&Vec<u8>>,
    ) -> Result<()> {
        let mut retry_count = 0;

        loop {
            match self.try_download_segment(index, segment, key).await {
                Ok(_) => {
                    // 更新统计信息
                    {
                        let mut stats = self.stats.lock().await;
                        stats.completed_segments += 1;

                        let speed = stats.get_speed();
                        let percentage = stats.get_progress_percentage();

                        self.progress_bar
                            .set_position(stats.completed_segments as u64);
                        self.progress_bar.set_message(format!(
                            "已下载: {}/{} ({:.1}%) 速度: {:.1} KB/s",
                            stats.completed_segments,
                            stats.total_segments,
                            percentage,
                            speed / 1024.0
                        ));
                    }
                    return Ok(());
                }
                Err(e) => {
                    retry_count += 1;
                    if retry_count > self.retry {
                        return Err(anyhow!(
                            "片段 {} 下载失败，已重试 {} 次: {}",
                            index,
                            self.retry,
                            e
                        ));
                    }
                    error!(
                        "片段 {} 下载失败，正在重试 ({}/{})...",
                        index, retry_count, self.retry
                    );
                    sleep(Duration::from_millis(1000 * retry_count as u64)).await;
                }
            }
        }
    }

    async fn try_download_segment(
        &self,
        index: usize,
        segment: &MediaSegment,
        key: Option<&Vec<u8>>,
    ) -> Result<()> {
        // 从segment.uri中提取文件名
        let segment_filename = Path::new(&segment.uri)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&format!("segment_{:06}.ts", index))
            .to_string();

        // 检查分段文件是否已存在
        let segment_path = self.download_dir.join(&segment_filename);
        if segment_path.exists() {
            // 校验已存在的文件是否有效
            if is_valid_ts_file(&segment_path) {
                info!(
                    "片段 {} ({}) 已存在且校验通过，跳过下载",
                    index, segment_filename
                );
                // 注意：这里不增加 completed_segments，由 download_segment 统一处理
                return Ok(());
            } else {
                error!(
                    "片段 {} ({}) 已存在但校验失败，将重新下载",
                    index, segment_filename
                );
                // 删除损坏的文件
                if let Err(e) = fs::remove_file(&segment_path) {
                    error!("删除损坏文件失败 {}: {}", segment_path.display(), e);
                }
            }
        }

        let segment_url = self.resolve_url(&segment.uri)?;
        // info!("正在下载片段 {}: {}", index, segment_url);
        let response = self.client.get(segment_url).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!("HTTP 错误: {}", response.status()));
        }

        let mut data = response.bytes().await?.to_vec();

        // 更新下载字节数
        {
            let mut stats = self.stats.lock().await;
            stats.downloaded_bytes += data.len() as u64;
        }

        // 如果有加密，进行解密
        if let Some(key_data) = key {
            data = self.decrypt_segment(data, key_data, index)?;
        }

        // 保存到下载目录
        let mut file = tokio::fs::File::create(&segment_path).await?;
        file.write_all(&data).await?;
        // info!("片段 {} 已保存到 {}", index, segment_path.display());

        Ok(())
    }

    fn decrypt_segment(&self, data: Vec<u8>, key: &[u8], segment_index: usize) -> Result<Vec<u8>> {
        if key.len() != 16 {
            return Err(anyhow!("AES 密钥长度必须为 16 字节"));
        }

        // 使用片段索引作为 IV（初始化向量）
        let mut iv = [0u8; 16];
        let iv_bytes = (segment_index as u128).to_be_bytes();
        iv.copy_from_slice(&iv_bytes);

        let cipher = Aes128CbcDec::new(key.into(), &iv.into());

        // 解密数据
        let mut decrypted = data.clone();
        let decrypted_data = cipher
            .decrypt_padded_mut::<Pkcs7>(&mut decrypted)
            .map_err(|e| anyhow!("解密失败: {:?}", e))?;

        Ok(decrypted_data.to_vec())
    }

    async fn merge_segments(&self, segments: &[MediaSegment]) -> Result<()> {
        let output_path = self
            .output_dir
            .join(format!("{}.mp4", self.output_filename));

        // 先合并为临时TS文件
        let temp_ts_path = self
            .download_dir
            .join(format!("{}_temp.ts", self.output_filename));
        let mut temp_file = File::create(&temp_ts_path)?;
        for (index, segment) in segments.iter().enumerate() {
            // 从segment.uri中提取文件名，与下载时保持一致
            let segment_filename = Path::new(&segment.uri)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&format!("segment_{:06}.ts", index))
                .to_string();
            let segment_path = self.download_dir.join(&segment_filename);

            if !segment_path.exists() {
                return Err(anyhow!("片段文件不存在: {:?}", segment_path));
            }

            let mut segment_file = File::open(&segment_path)?;
            let mut buffer = Vec::new();
            segment_file.read_to_end(&mut buffer)?;
            temp_file.write_all(&buffer)?;
        }

        temp_file.flush()?;

        // 使用FFmpeg将TS转换为MP4
        let output = Command::new("ffmpeg")
            .args([
                "-i",
                temp_ts_path.to_str().unwrap(),
                "-c",
                "copy", // 直接复制流，不重新编码
                "-bsf:a",
                "aac_adtstoasc", // 修复AAC音频流
                "-y",            // 覆盖输出文件
                output_path.to_str().unwrap(),
            ])
            .output()
            .map_err(|e| anyhow!("执行FFmpeg失败: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("FFmpeg转换失败: {}", stderr));
        }

        // 删除临时TS文件
        fs::remove_file(&temp_ts_path).map_err(|e| anyhow!("删除临时文件失败: {}", e))?;
        if let Err(e) = fs::remove_dir_all(&self.download_dir) {
            error!("删除下载目录失败 {}: {}", self.download_dir.display(), e);
        } else {
            info!("已删除下载目录: {}", self.download_dir.display());
        }
        info!("视频文件已转换为MP4: {}", output_path.display());
        Ok(())
    }

    fn parse_m3u8(&self, content: &str) -> Result<MediaPlaylist> {
        match m3u8_rs::parse_media_playlist(content.as_bytes()) {
            Ok((_, playlist)) => Ok(playlist),
            Err(e) => Err(anyhow!("M3U8 解析失败: {:?}", e)),
        }
    }

    fn resolve_url(&self, url: &str) -> Result<String> {
        if url.starts_with("http://") || url.starts_with("https://") {
            Ok(url.to_string())
        } else {
            Ok(self.base_url.join(url)?.to_string())
        }
    }

    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            base_url: self.base_url.clone(),
            download_dir: self.download_dir.clone(),
            concurrent: self.concurrent,
            retry: self.retry,
            stats: self.stats.clone(),
            progress_bar: self.progress_bar.clone(),
            output_filename: self.output_filename.clone(),
            output_dir: self.output_dir.clone(),
        }
    }
}
