use serde::{Deserialize, Serialize};
use std::fs;

use crate::error::{Result, DownloadError};

/// 定义下载任务结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadTask {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub output_dir: String,
}

/// 从JSON文件加载下载任务
pub fn load_download_tasks_from_json(json_path: &str) -> Result<Vec<DownloadTask>> {
    // 读取JSON文件
    let json_content = fs::read_to_string(json_path)
        .map_err(|e| DownloadError::file(json_path, format!("读取JSON文件失败: {e}")))?;

    // 解析JSON内容
    serde_json::from_str(&json_content)
        .map_err(|e| DownloadError::parse(format!("解析JSON失败: {e}")))
}
