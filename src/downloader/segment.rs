use crate::config::WRITE_BUFFER_SIZE;
use crate::error::{DownloadError, Result};
use crate::utils::get_segment_filename;
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
    let temp_file = fs::File::create(&temp_ts_path)
        .await
        .map_err(|e| DownloadError::file(&temp_ts_path, e.to_string()))?;
    let mut writer = tokio::io::BufWriter::with_capacity(WRITE_BUFFER_SIZE, temp_file);

    for (index, segment) in segments.iter().enumerate() {
        let segment_filename = get_segment_filename(&segment.uri, index);
        let segment_path = download_dir.join(&segment_filename);

        if !segment_path.exists() {
            return Err(DownloadError::file(
                &segment_path,
                format!("片段文件不存在: {}", segment_path.display()),
            ));
        }

        let mut segment_file = fs::File::open(&segment_path)
            .await
            .map_err(|e| DownloadError::file(&segment_path, e.to_string()))?;
        let mut buffer = Vec::new();
        segment_file
            .read_to_end(&mut buffer)
            .await
            .map_err(|e| DownloadError::file(&segment_path, e.to_string()))?;
        writer
            .write_all(&buffer)
            .await
            .map_err(|e| DownloadError::file(&temp_ts_path, e.to_string()))?;
    }

    writer
        .flush()
        .await
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

/// 合并所有TS片段到临时文件
pub async fn merge_segments_to_temp_ts(
    download_dir: &Path,
    segments: &[m3u8_rs::MediaSegment],
    temp_ts_path: &Path,
) -> Result<()> {
    let temp_file = fs::File::create(temp_ts_path)
        .await
        .map_err(|e| DownloadError::file(temp_ts_path, e.to_string()))?;
    let mut writer = tokio::io::BufWriter::with_capacity(WRITE_BUFFER_SIZE, temp_file);

    for (index, segment) in segments.iter().enumerate() {
        let segment_filename = get_segment_filename(&segment.uri, index);
        let segment_path = download_dir.join(&segment_filename);

        if !segment_path.exists() {
            return Err(DownloadError::file(
                &segment_path,
                format!("片段文件不存在: {}", segment_path.display()),
            ));
        }

        let mut segment_file = fs::File::open(&segment_path)
            .await
            .map_err(|e| DownloadError::file(&segment_path, e.to_string()))?;
        let mut buffer = Vec::new();
        segment_file
            .read_to_end(&mut buffer)
            .await
            .map_err(|e| DownloadError::file(&segment_path, e.to_string()))?;
        writer
            .write_all(&buffer)
            .await
            .map_err(|e| DownloadError::file(temp_ts_path, e.to_string()))?;
    }

    writer
        .flush()
        .await
        .map_err(|e| DownloadError::file(temp_ts_path, e.to_string()))?;

    Ok(())
}

/// 通过ffmpeg pipe将TS转为MP4并流式输出
pub async fn merge_to_mp4_stream(
    temp_ts_path: &Path,
    tx: &tokio::sync::mpsc::Sender<std::result::Result<bytes::Bytes, String>>,
) -> Result<()> {
    use std::process::Stdio;
    use tokio::io::AsyncReadExt;
    use tokio::process::Command;

    let mut child = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-i",
            temp_ts_path.to_str().unwrap(),
            "-c",
            "copy",
            "-bsf:a",
            "aac_adtstoasc",
            "-movflags",
            "frag_keyframe+empty_moov+default_base_moof",
            "-f",
            "mp4",
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| DownloadError::ffmpeg(format!("启动FFmpeg失败: {e}")))?;

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| DownloadError::ffmpeg("无法获取FFmpeg标准输出".to_string()))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| DownloadError::ffmpeg("无法获取FFmpeg标准错误输出".to_string()))?;
    let mut buf = vec![0u8; 65536];
    let stderr_task = tokio::spawn(async move {
        let mut stderr_buf = Vec::new();
        let _ = stderr.read_to_end(&mut stderr_buf).await;
        stderr_buf
    });

    loop {
        let n = stdout
            .read(&mut buf)
            .await
            .map_err(|e| DownloadError::ffmpeg(format!("读取FFmpeg输出失败: {e}")))?;
        if n == 0 {
            break;
        }
        let _ = tx.send(Ok(bytes::Bytes::copy_from_slice(&buf[..n]))).await;
    }

    let status = child
        .wait()
        .await
        .map_err(|e| DownloadError::ffmpeg(format!("等待FFmpeg失败: {e}")))?;
    let stderr_output = stderr_task.await.unwrap_or_default();

    if !status.success() {
        let stderr_text = String::from_utf8_lossy(&stderr_output).trim().to_string();
        if stderr_text.is_empty() {
            return Err(DownloadError::ffmpeg("FFmpeg转换失败".to_string()));
        }
        return Err(DownloadError::ffmpeg(format!("FFmpeg转换失败: {stderr_text}")));
    }

    Ok(())
}
