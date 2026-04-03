use anyhow::Result;
use clap::{Parser, Subcommand};

mod downloader;
mod server;
mod utils;

#[derive(Parser)]
#[command(name = "m3u8-downloader")]
#[command(about = "M3U8 多线程下载器", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// 启动Web服务模式
    Serve {
        /// 监听主机
        #[arg(short = 'H', long, default_value = "0.0.0.0")]
        host: String,

        /// 监听端口
        #[arg(short, long, default_value = "8080")]
        port: u16,

        /// 最大并发下载数
        #[arg(short, long, default_value = "8")]
        concurrent: usize,
    },

    /// 从JSON文件批量下载
    Batch {
        /// JSON任务文件路径
        #[arg(short, long, default_value = "./examples/download_tasks.json")]
        file: String,

        /// 最大并发下载数
        #[arg(short, long, default_value = "8")]
        concurrent: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    utils::init_logger();

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Serve {
            host,
            port,
            concurrent,
        }) => {
            log::info!("🚀 启动Web服务模式...");
            log::info!("📡 主机: {host}:{port}");
            log::info!("⚡ 最大并发数: {concurrent} (可通过设置页面修改)");
            log::info!("💡 提示: 并发数可在运行后通过设置页面调整");

            server::start_server(&host, port).await?;
        }
        Some(Commands::Batch { file, concurrent }) => {
            log::info!("📦 启动批量下载模式...");
            log::info!("📄 任务文件: {file}");
            log::info!("⚡ 最大并发数: {concurrent}");

            match utils::download_segment::load_and_process_download_tasks(&file, concurrent).await
            {
                Ok(()) => log::info!("✅ 所有任务已完成"),
                Err(e) => log::error!("❌ 批量下载失败: {e}"),
            }
        }
        None => {
            log::info!("🚀 启动Web服务模式（默认）...");
            log::info!("📡 主机: 0.0.0.0:8080");
            log::info!("💡 提示: 访问 http://0.0.0.0:8080 使用Web界面");

            server::start_server("0.0.0.0", 8080).await?;
        }
    }

    Ok(())
}
