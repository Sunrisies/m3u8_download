use reqwest::blocking::get;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use url::Url;

use crate::utils::{parse_m3u8_from_source, PlaylistExt};

/// 下载单个媒体片段
pub fn download_segment(
    segment_uri: &str,
    base_url: Option<&str>,
    output_dir: &str,
) -> Result<String, String> {
    println!("开始下载:{}", segment_uri);
    // 构建完整的URL
    let url = if let Some(base) = base_url {
        // 如果提供了基础URL，则组合基础URL和片段URI
        if Url::parse(segment_uri).is_ok() {
            // 如果片段URI已经是完整URL，则直接使用
            segment_uri.to_string()
        } else {
            // 否则，组合基础URL和片段URI
            let base_url = base.trim_end_matches('/');
            let segment_uri = segment_uri.trim_start_matches('/');
            format!("{}/{}", base_url, segment_uri)
        }
    } else {
        // 如果没有提供基础URL，则假设片段URI是完整URL
        segment_uri.to_string()
    };

    // 从URL获取文件名
    let filename = if let Ok(parsed_url) = Url::parse(&url) {
        parsed_url
            .path_segments()
            .and_then(|segments| segments.last())
            .unwrap_or("segment.ts")
            .to_string()
    } else {
        "segment.ts".to_string()
    };

    // 构建输出文件路径
    let output_path = Path::new(output_dir).join(&filename);
    // 检查文件是否已存在
    if output_path.exists() {
        println!("当前片段存在: {}", output_path.display());
        return Ok(output_path.to_string_lossy().to_string());
    }

    // 创建输出目录（如果不存在）
    if let Some(parent) = output_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {}", e))?;
        }
    }
    println!("Downloading segment from: {}", url);
    // 发送HTTP请求下载文件
    let response = get(&url).map_err(|e| format!("Failed to download segment: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "HTTP request failed with status: {}",
            response.status()
        ));
    }

    // 获取响应内容
    let content = response
        .bytes()
        .map_err(|e| format!("Failed to read response: {}", e))?;

    // 写入文件
    let mut file =
        File::create(&output_path).map_err(|e| format!("Failed to create file: {}", e))?;
    file.write_all(&content)
        .map_err(|e| format!("Failed to write file: {}", e))?;

    Ok(output_path.to_string_lossy().to_string())
}

/// 下载播放列表中的所有片段
pub fn download_playlist_segments(
    playlist_source: &str,
    output_dir: &str,
) -> Result<Vec<String>, String> {
    // 解析M3U8播放列表
    let playlist = parse_m3u8_from_source(playlist_source)?;
    // 获取基础URL（用于解析相对路径）
    let base_url = if let Ok(parsed_url) = Url::parse(playlist_source) {
        // 获取播放列表的目录路径作为基础URL
        let path = parsed_url.path();
        if let Some(last_slash_pos) = path.rfind('/') {
            let dir_path = &path[..=last_slash_pos];
            let mut base_url = parsed_url.clone();
            base_url.set_path(dir_path);
            Some(base_url.to_string())
        } else {
            None
        }
    } else {
        None
    };

    // 获取所有片段
    let segments = playlist
        .get_segments()
        .ok_or("Failed to get segments from playlist")?;

    println!("下载 {} 片段 {}", segments.len(), output_dir);

    // 下载所有片段
    let mut downloaded_files = Vec::new();
    for (i, segment) in segments.iter().enumerate() {
        println!("下载片段 {}/{}: {}", i + 1, segments.len(), segment.uri);

        match download_segment(&segment.uri, base_url.as_deref(), output_dir) {
            Ok(path) => {
                println!("Successfully downloaded to: {}", path);
                downloaded_files.push(path);
            }
            Err(e) => {
                eprintln!("Failed to download segment: {}", e);
                // 继续下载下一个片段，而不是中断整个过程
            }
        }
    }

    Ok(downloaded_files)
}

/// 合并下载的片段为单个文件
pub fn merge_segments(segment_files: &[String], output_file: &str) -> Result<(), String> {
    // 创建输出文件
    let mut output =
        File::create(output_file).map_err(|e| format!("Failed to create output file: {}", e))?;

    // 逐个读取并写入片段文件
    for segment_file in segment_files {
        println!("Merging segment: {}", segment_file);

        // 读取片段文件
        let mut segment =
            File::open(segment_file).map_err(|e| format!("Failed to open segment file: {}", e))?;

        // 将片段内容写入输出文件
        std::io::copy(&mut segment, &mut output)
            .map_err(|e| format!("Failed to copy segment data: {}", e))?;
    }

    Ok(())
}

/// 下载并合并M3U8播放列表中的所有片段
pub fn download_and_merge_playlist(
    playlist_source: &str,
    output_file: &str,
    temp_dir: &str,
) -> Result<(), String> {
    // 下载所有片段
    let segment_files = download_playlist_segments(playlist_source, temp_dir)?;

    // // 合并片段
    // merge_segments(&segment_files, output_file)?;

    // // 可选：删除临时文件
    // for segment_file in &segment_files {
    //     if let Err(e) = fs::remove_file(segment_file) {
    //         eprintln!("Failed to remove temporary file {}: {}", segment_file, e);
    //     }
    // }

    Ok(())
}
