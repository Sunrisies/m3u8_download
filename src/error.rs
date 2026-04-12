use thiserror::Error;

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("HTTP错误: {status} - {url}")]
    HttpError { status: u16, url: String },

    #[error("M3U8解析失败: {reason}")]
    ParseError { reason: String },

    #[error("文件操作失败: {path} - {error}")]
    FileError { path: String, error: String },

    #[error("解密失败: {reason}")]
    DecryptionError { reason: String },

    #[error("网络请求超时: {url} - 超时时间: {timeout}秒")]
    Timeout { url: String, timeout: u64 },

    #[error("片段下载失败: 片段{index} - 已重试{retry_count}次 - {error}")]
    SegmentError {
        index: usize,
        retry_count: usize,
        error: String,
    },

    #[error("任务失败: {task_name} - {error}")]
    TaskError { task_name: String, error: String },

    #[error("加密密钥错误: {reason}")]
    KeyError { reason: String },

    #[error("FFmpeg执行失败: {error}")]
    FfmpegError { error: String },
    #[error("未知错误: {0}")]
    Unknown(String),
}

// 定义Result类型别名
pub type Result<T> = std::result::Result<T, DownloadError>;

// 实现从常见错误类型的转换
impl From<std::io::Error> for DownloadError {
    fn from(err: std::io::Error) -> Self {
        DownloadError::file("unknown", err.to_string())
    }
}

impl From<reqwest::Error> for DownloadError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            DownloadError::timeout("unknown", 30)
        } else if err.is_status() {
            DownloadError::http(
                err.status().map_or(0, |s| s.as_u16()),
                "unknown".to_string(),
            )
        } else {
            DownloadError::parse(err.to_string())
        }
    }
}

impl From<url::ParseError> for DownloadError {
    fn from(err: url::ParseError) -> Self {
        DownloadError::parse(err.to_string())
    }
}

impl From<anyhow::Error> for DownloadError {
    fn from(err: anyhow::Error) -> Self {
        DownloadError::Unknown(err.to_string())
    }
}

impl DownloadError {
    /// 创建HTTP错误
    pub fn http(status: u16, url: impl Into<String>) -> Self {
        Self::HttpError {
            status,
            url: url.into(),
        }
    }

    /// 创建解析错误
    pub fn parse(reason: impl Into<String>) -> Self {
        Self::ParseError {
            reason: reason.into(),
        }
    }

    /// 创建文件错误
    pub fn file(path: impl AsRef<std::path::Path>, error: impl Into<String>) -> Self {
        Self::FileError {
            path: path.as_ref().to_string_lossy().to_string(),
            error: error.into(),
        }
    }

    /// 创建解密错误
    pub fn decryption(reason: impl Into<String>) -> Self {
        Self::DecryptionError {
            reason: reason.into(),
        }
    }

    /// 创建超时错误
    pub fn timeout(url: impl Into<String>, timeout: u64) -> Self {
        Self::Timeout {
            url: url.into(),
            timeout,
        }
    }

    /// 创建片段错误
    pub fn segment(index: usize, retry_count: usize, error: impl Into<String>) -> Self {
        Self::SegmentError {
            index,
            retry_count,
            error: error.into(),
        }
    }

    /// 创建任务错误
    pub fn task(task_name: impl Into<String>, error: impl Into<String>) -> Self {
        Self::TaskError {
            task_name: task_name.into(),
            error: error.into(),
        }
    }

    /// 创建密钥错误
    pub fn key(reason: impl Into<String>) -> Self {
        Self::KeyError {
            reason: reason.into(),
        }
    }

    /// 创建FFmpeg错误
    pub fn ffmpeg(error: impl Into<String>) -> Self {
        Self::FfmpegError {
            error: error.into(),
        }
    }
}
