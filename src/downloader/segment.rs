use crate::utils::get_segment_filename;
use crate::config::*;
use crate::error::{Result, DownloadError};
use std::path::Path;
// 使用FFmpeg将TS转换为MP4
use std::process::Command;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// 合并所有视频片段
pub async fn merge_segments(
    download_dir: &Path,
    segments: &[m3u8_rs::MediaSegment],
    output_path: &Path,
) -> Result<()> {
    // 先合并为临时TS文件
    let temp_ts_path = download_dir.join("temp.ts");
    let temp_file = fs::File::create(&temp_ts_path).await
        .map_err(|e| DownloadError::file(&temp_ts_path, e.to_string()))?;
    let mut writer = tokio::io::BufWriter::with_capacity(WRITE_BUFFER_SIZE, temp_file);

    for (index, segment) in segments.iter().enumerate() {
        let segment_filename = get_segment_filename(&segment.uri, index);
        let segment_path = download_dir.join(&segment_filename);

        if !segment_path.exists() {
            return Err(DownloadError::file(
                &segment_path,
                format!("片段文件不存在: {}", segment_path.display())
            ));
        }

        let mut segment_file = fs::File::open(&segment_path).await
            .map_err(|e| DownloadError::file(&segment_path, e.to_string()))?;
        let mut buffer = Vec::new();
        segment_file.read_to_end(&mut buffer).await
            .map_err(|e| DownloadError::file(&segment_path, e.to_string()))?;
        writer.write_all(&buffer).await
            .map_err(|e| DownloadError::file(&temp_ts_path, e.to_string()))?;
    }

    writer.flush().await
        .map_err(|e| DownloadError::file(&temp_ts_path, e.to_string()))?;

    let output = Command::new("ffmpeg")
        .args([
            "-i",
            temp_ts_path.to_str().unwrap(),
            "-c",
            "copy",
            "-bsf:a",
            "aac_adtstoasc",
            "-y",
            output_path.to_str().unwrap(),
        ])
        .output()
        .map_err(|e| DownloadError::ffmpeg(format!("执行FFmpeg失败: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DownloadError::ffmpeg(format!("FFmpeg转换失败: {stderr}")));
    }

    // 清理临时文件
    let _ = fs::remove_file(&temp_ts_path).await;

    Ok(())
}
