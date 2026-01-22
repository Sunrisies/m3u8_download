// use anyhow::{Result, anyhow};
// use clap::Parser;
// use log::{error, info};

mod downloader;
// // mod error;
// mod utils;

// use downloader::{Args, M3u8Downloader};
// use utils::{DownloadTask, load_download_tasks_from_json};

// #[tokio::main]
// async fn main() -> Result<()> {
//     // 初始化日志
//     utils::init_logger();
//     //
//     // 处理JSON任务文件
//     process_json_tasks("./examples/download_tasks.json", 8).await?;

//     Ok(())
// }

// async fn process_json_tasks(json_path: &str, max_concurrent: usize) -> Result<()> {
//     info!("从JSON文件加载任务: {}", json_path);

//     let tasks =
//         load_download_tasks_from_json(json_path).map_err(|e| anyhow!("加载任务失败: {}", e))?;

//     info!("找到 {} 个任务", tasks.len());

//     for (i, task) in tasks.iter().enumerate() {
//         info!("正在处理任务 {}/{}: {}", i + 1, tasks.len(), task.name);

//         match process_single_task_from_task(task, max_concurrent).await {
//             Ok(_) => info!("✅ 任务 {} 完成", task.name),
//             Err(e) => error!("❌ 任务 {} 失败: {}", task.name, e),
//         }
//     }

//     Ok(())
// }

// async fn process_single_task_from_task(task: &DownloadTask, max_concurrent: usize) -> Result<()> {
//     // 确定输出目录
//     let output_dir = if task.output_dir.is_empty() {
//         format!("./output")
//     } else {
//         format!("{}/{}", task.output_dir, task.name)
//     };

//     // 创建输出目录
//     std::fs::create_dir_all(&output_dir)?;

//     // 确定下载目录
//     let download_dir = format!("./downloads/{}", task.name);
//     std::fs::create_dir_all(&download_dir)?;

//     let args = Args {
//         url: task.url.clone(),
//         output_name: task.name.clone(),
//         download_dir,
//         concurrent: max_concurrent,
//         retry: 4,
//         output_dir,
//     };

//     match M3u8Downloader::new(args) {
//         Ok(downloader) => downloader.download().await,
//         Err(e) => Err(anyhow!("创建下载器失败: {}", e)),
//     }
// }

use anyhow::Result;

use crate::utils::{download_segment::load_and_process_download_tasks, init_logger};
mod utils;
#[tokio::main]
async fn main() -> Result<()> {
    init_logger(); // 初始化日志
    match load_and_process_download_tasks("./examples/download_tasks.json", 8).await {
        Ok(tasks) => println!("Successfully processed all download tasks:{:?}", tasks),
        Err(e) => eprintln!("Failed to process download tasks: {}", e),
    }
    Ok(())
}
