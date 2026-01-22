use anyhow::{anyhow, Result};
use clap::Parser;
use futures::future::join_all;
use indicatif::{ProgressBar, ProgressStyle};
use m3u8_rs::{MediaPlaylist, MediaSegment};
use reqwest::Client;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::io::AsyncWriteExt;
use tokio::time::sleep;
use url::Url;

// AES解密相关
use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use aes::Aes128;

use crate::utils::json_loader::{load_download_tasks_from_json, DownloadTask};

type Aes128CbcDec = cbc::Decryptor<Aes128>;
/// 处理单个下载任务（并发版）
pub async fn process_download_task(
    task: &DownloadTask,
    max_concurrent: usize,
) -> Result<(), String> {
    println!("Processing download task: {}", task.name);

    // 确定输出目录
    let output_dir = if task.output_dir.is_empty() {
        format!("./output")
    } else {
        format!("{}/{}", task.output_dir, task.name)
    };
    println!("输出目录是:{}", output_dir);
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
    // 分离目录和文件名
    let output_path = PathBuf::from(&output_dir);
    // let download_dir = output_path.clone();
    let output_filename = task.name.clone();
    println!(
        "&task.url:{:?},download_dir{:?},111:{:?}",
        output_path, download_dir, output_filename
    );
    let args = Args {
        url: task.url.clone(),
        output: output_filename,
        download_dir: download_dir,
        concurrent: max_concurrent,
        retry: 4,
        output_dir: output_dir,
    };
    let downloader = M3u8Downloader::new(args).map_err(|e| e.to_string())?;
    match downloader.download().await {
        Ok(_) => {
            println!("✅ 下载成功完成！");
        }
        Err(e) => {
            eprintln!("❌ 下载失败: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

// /// 处理多个下载任务（并发版）
pub async fn process_download_tasks(
    tasks: &[DownloadTask],
    max_concurrent: usize,
) -> Result<(), String> {
    println!(
        "Processing {} download tasks with {} concurrent downloads",
        tasks.len(),
        max_concurrent
    );

    for (i, task) in tasks.iter().enumerate() {
        println!("Starting task {}/{}", i + 1, tasks.len());

        if let Err(e) = process_download_task(task, max_concurrent).await {
            eprintln!("Failed to process task '{}': {}", task.name, e);
            // 继续处理下一个任务，而不是中断整个过程
        }
    }

    println!("Completed processing {} tasks", tasks.len());
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
// #[command(author, version, about, long_about = None)]
struct Args {
    /// M3U8 播放列表 URL
    // #[arg(short, long)]
    url: String,

    /// 输出文件名（不包含扩展名）
    // #[arg(short, long, default_value = "video")]
    output: String,

    /// 并发下载数
    // #[arg(short, long, default_value = "8")]
    concurrent: usize,

    /// 重试次数
    // #[arg(short, long, default_value = "3")]
    retry: usize,

    /// 下载目录
    // #[arg(short, long, default_value = "download")]
    download_dir: String,
    /// 输出目录
    output_dir: String,
}

#[derive(Clone)]
struct DownloadStats {
    total_segments: usize,
    completed_segments: usize,
    total_bytes: u64,
    downloaded_bytes: u64,
    start_time: Instant,
}

impl DownloadStats {
    fn new(total_segments: usize) -> Self {
        Self {
            total_segments,
            completed_segments: 0,
            total_bytes: 0,
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
            output_filename: args.output,
            output_dir,
        })
    }

    async fn download(&self) -> Result<()> {
        println!("正在获取 M3U8 播放列表...");

        // 下载并解析 M3U8 文件
        let m3u8_content = self.download_text(&self.base_url.to_string()).await?;
        let playlist = self.parse_m3u8(&m3u8_content)?;

        println!("发现 {} 个视频片段", playlist.segments.len());

        // 更新统计信息
        {
            let mut stats = self.stats.lock().await;
            stats.total_segments = playlist.segments.len();
        }

        self.progress_bar.set_length(playlist.segments.len() as u64);

        // 检查加密 - 从播放列表内容中提取密钥信息
        let key_data = self.extract_encryption_key(&m3u8_content).await?;

        if key_data.is_some() {
            println!("检测到加密流，已获取密钥");
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
        println!("正在合并视频文件...");
        self.merge_segments(&segments).await?;

        println!(
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
                    println!(
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
            println!("片段 {} ({}) 已存在，跳过下载", index, segment_filename);
            // 更新统计信息（已下载字节数）
            {
                let mut stats = self.stats.lock().await;
                stats.completed_segments += 1;
                stats.downloaded_bytes += segment_path.metadata()?.len();
            }
            return Ok(());
        }

        let segment_url = self.resolve_url(&segment.uri)?;
        println!("正在下载片段 {}: {}", index, segment_url);
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
        println!("片段 {} 已保存到 {}", index, segment_path.display());

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

        println!("正在合并片段到: {}", output_path.display());
        // 先合并为临时TS文件
        let temp_ts_path = self
            .download_dir
            .join(format!("{}_temp.ts", self.output_filename));
        println!("正在合并片段到临时文件: {}", temp_ts_path.display());
        let mut temp_file = File::create(&temp_ts_path)?;
        let mut output_file = File::create(&output_path)?;
        for (index, segment) in segments.iter().enumerate() {
            // 从segment.uri中提取文件名，与下载时保持一致
            let segment_filename = Path::new(&segment.uri)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&format!("segment_{:06}.ts", index))
                .to_string();
            println!("正在合并片段: {}", segment_filename);
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
        // let output_path = self
        //     .download_dir
        //     .join(format!("{}.mp4", self.output_filename));
        println!("正在转换为MP4格式: {}", output_path.display());

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

        println!("视频文件已转换为MP4: {}", output_path.display());
        // println!("视频文件已合并到: {}", output_path.display());
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
