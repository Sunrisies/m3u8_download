use crate::utils::get_segment_filename;
use anyhow::{Result, anyhow};
use std::path::PathBuf;
use tokio::fs;

/// 合并所有视频片段
pub async fn merge_segments(
    download_dir: &PathBuf,
    segments: &[m3u8_rs::MediaSegment],
    output_path: &PathBuf,
) -> Result<()> {
    use std::fs::File;
    use std::io::{Read, Write};

    // 先合并为临时TS文件
    let temp_ts_path = download_dir.join("temp.ts");
    let mut temp_file = File::create(&temp_ts_path)?;

    for (index, segment) in segments.iter().enumerate() {
        let segment_filename = get_segment_filename(&segment.uri, index);
        let segment_path = download_dir.join(&segment_filename);

        if !segment_path.exists() {
            return Err(anyhow!(format!("片段文件不存在: {:?}", segment_path)));
        }

        let mut segment_file = File::open(&segment_path)?;
        let mut buffer = Vec::new();
        segment_file.read_to_end(&mut buffer)?;
        temp_file.write_all(&buffer)?;
    }

    temp_file.flush()?;

    // 使用FFmpeg将TS转换为MP4
    use std::process::Command;

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
        .map_err(|e| anyhow!(format!("执行FFmpeg失败: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(format!("FFmpeg转换失败: {}", stderr)));
    }

    // 清理临时文件
    let _ = fs::remove_file(&temp_ts_path).await;

    Ok(())
}
