use thiserror::Error;

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("HTTP错误: {0}")]
    HttpError(String),

    #[error("M3U8解析失败: {0}")]
    ParseError(String),

    #[error("文件操作失败: {0}")]
    FileError(String),

    #[error("解密失败: {0}")]
    DecryptionError(String),

    #[error("网络请求超时")]
    Timeout,

    #[error("任务失败: {0}")]
    TaskError(String),

    #[error("未知错误: {0}")]
    Unknown(String),
}

// 定义Result类型别名
pub type Result<T> = std::result::Result<T, DownloadError>;
