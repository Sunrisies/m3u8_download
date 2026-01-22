use crate::downloader::{
    Args, DownloadStats, decrypt_segment, extract_encryption_key, merge_segments,
    process_download_tasks,
};
use anyhow::{Result, anyhow};
use futures::future::join_all;
use indicatif::{ProgressBar, ProgressStyle};
use log::{error, info};
use m3u8_rs::{MediaPlaylist, MediaSegment};
use reqwest::Client;
use std::fs::{self};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::time::sleep;
use url::Url;
// AES解密相关
use crate::utils::json_loader::load_download_tasks_from_json;
use crate::utils::{is_valid_ts_file, resolve_url};

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

pub struct M3u8Downloader {
    pub client: Client,
    pub base_url: Url,
    pub download_dir: PathBuf,
    pub concurrent: usize,
    pub retry: usize,
    pub stats: Arc<tokio::sync::Mutex<DownloadStats>>,
    pub progress_bar: ProgressBar,
    pub output_filename: String,
    pub output_dir: PathBuf,
}

impl M3u8Downloader {
    pub fn new(args: Args) -> Result<Self> {
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

    pub async fn download(&self) -> Result<()> {
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
        let key_data = extract_encryption_key(&m3u8_content, &self.client, &self.base_url).await?;

        if key_data.is_some() {
            info!("检测到加密流，已获取密钥");
        }

        // 并行下载片段
        let segments = Arc::new(playlist.segments);
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.concurrent));
        info!("开始下载片段...{}", segments.len());
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

    async fn download_text(&self, url: &str) -> Result<String> {
        let full_url = resolve_url(&self.base_url, url)?;
        let response = self.client.get(full_url).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!("HTTP 错误: {}", response.status()));
        }

        Ok(response.text().await?)
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

        let segment_url = resolve_url(&self.base_url, &segment.uri)?;
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
            data = decrypt_segment(data, key_data, index)?;
        }

        // 保存到下载目录
        let mut file = tokio::fs::File::create(&segment_path).await?;
        file.write_all(&data).await?;
        // info!("片段 {} 已保存到 {}", index, segment_path.display());

        Ok(())
    }
    async fn merge_segments(&self, segments: &[m3u8_rs::MediaSegment]) -> Result<()> {
        let output_path = self
            .output_dir
            .join(format!("{}.mp4", self.output_filename));
        merge_segments(&self.download_dir, segments, &output_path).await?;

        // 清理下载目录
        if let Err(e) = std::fs::remove_dir_all(&self.download_dir) {
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
