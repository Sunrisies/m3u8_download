use serde::{Deserialize, Serialize};
use std::fs;

// use crate::utils::download_segment::process_download_tasks;
/// 定义下载任务结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadTask {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub output_dir: String,
}

/// 从JSON文件加载下载任务
pub fn load_download_tasks_from_json(json_path: &str) -> Result<Vec<DownloadTask>, String> {
    // 读取JSON文件
    let json_content =
        fs::read_to_string(json_path).map_err(|e| format!("Failed to read JSON file: {}", e))?;

    // 解析JSON内容
    let tasks: Vec<DownloadTask> =
        serde_json::from_str(&json_content).map_err(|e| format!("Failed to parse JSON: {}", e))?;

    Ok(tasks)
}

// /// 从JSON文件加载并处理下载任务
// pub fn load_and_process_download_tasks(json_path: &str) -> Result<(), String> {
//     // 加载下载任务
//     let tasks = load_download_tasks_from_json(json_path)?;

//     // 处理下载任务
//     process_download_tasks(&tasks)
// }
