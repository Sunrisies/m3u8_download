//! 错误处理模块
//!
//! 该模块定义了项目中使用的所有错误类型，提供了统一的错误处理机制。
//! 所有错误都实现了 `std::error::Error` 和 `std::fmt::Display` trait。
//!
//! # 错误类型
//!
//! - `HttpError`: HTTP 请求错误
//! - `ParseError`: M3U8 解析错误
//! - `FileError`: 文件操作错误
//! - `DecryptionError`: 解密错误
//! - `Timeout`: 请求超时
//! - `SegmentError`: 片段下载错误
//! - `TaskError`: 任务执行错误
//! - `KeyError`: 密钥错误
//! - `FfmpegError`: `FFmpeg` 执行错误
//! - `Unknown`: 未知错误
//! - `UrlValidationError`: URL 验证错误
//! - `ValidationError`: 配置验证错误
//!
//! # 示例
//!
//! ```
//! use crate::error::{DownloadError, Result};
//!
//! fn download_file(url: &str) -> Result<Vec<u8>> {
//!     // 验证 URL
//!     validation::validate_url(url)?;
//!     
//!     // 下载文件
//!     let response = reqwest::get(url).await?;
//!     Ok(response.bytes().await?.to_vec())
//! }
//! ```

use thiserror::Error;

/// 下载错误类型
///
/// 该枚举包含了所有可能发生的下载相关错误，每个错误都提供了详细的错误信息。
#[derive(Error, Debug)]
pub enum DownloadError {
    /// HTTP 请求错误
    ///
    /// 当 HTTP 请求失败或返回错误状态码时产生。
    #[error("HTTP错误: {status} - {url}")]
    HttpError { status: u16, url: String },

    /// M3U8 播放列表解析错误
    ///
    /// 当解析 M3U8 文件失败时产生。
    #[error("M3U8解析失败: {reason}")]
    ParseError { reason: String },

    /// 文件操作错误
    ///
    /// 当文件读写或目录操作失败时产生。
    #[error("文件操作失败: {path} - {error}")]
    FileError { path: String, error: String },

    /// 解密错误
    ///
    /// 当解密视频片段失败时产生。
    #[error("解密失败: {reason}")]
    DecryptionError { reason: String },

    /// 请求超时错误
    ///
    /// 当网络请求超过指定时间未响应时产生。
    #[error("网络请求超时: {url} - 超时时间: {timeout}秒")]
    Timeout { url: String, timeout: u64 },

    /// 片段下载错误
    ///
    /// 当下载视频片段失败且达到最大重试次数时产生。
    #[error("片段下载失败: 片段{index} - 已重试{retry_count}次 - {error}")]
    SegmentError {
        index: usize,
        retry_count: usize,
        error: String,
    },

    /// 任务执行错误
    ///
    /// 当执行下载任务失败时产生。
    #[error("任务失败: {task_name} - {error}")]
    TaskError { task_name: String, error: String },

    /// 密钥错误
    ///
    /// 当获取或使用加密密钥失败时产生。
    #[error("加密密钥错误: {reason}")]
    KeyError { reason: String },

    /// `FFmpeg` 执行错误
    ///
    /// 当使用 `FFmpeg` 转换视频格式失败时产生。
    #[error("FFmpeg执行失败: {error}")]
    FfmpegError { error: String },

    /// 未知错误
    ///
    /// 当发生未预期的错误时产生。
    #[error("未知错误: {0}")]
    Unknown(String),

    /// URL 验证错误
    ///
    /// 当 URL 格式无效或不安全时产生。
    #[error("URL验证失败: {url} - {reason}")]
    UrlValidationError { url: String, reason: String },

    /// 配置验证错误
    ///
    /// 当配置值无效或超出范围时产生。
    #[error("配置验证失败: {field} - {error}")]
    ValidationError { field: String, error: String },
}

/// Result 类型别名
///
/// 用于简化返回类型的声明，统一使用 `DownloadError` 作为错误类型。
pub type Result<T> = std::result::Result<T, DownloadError>;

// 实现从常见错误类型的转换
impl From<std::io::Error> for DownloadError {
    fn from(err: std::io::Error) -> Self {
        Self::file("unknown", err.to_string())
    }
}

impl From<reqwest::Error> for DownloadError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            Self::timeout("unknown", 30)
        } else if err.is_status() {
            Self::http(
                err.status().map_or(0, |s| s.as_u16()),
                "unknown".to_string(),
            )
        } else {
            Self::parse(err.to_string())
        }
    }
}

impl From<url::ParseError> for DownloadError {
    fn from(err: url::ParseError) -> Self {
        Self::parse(err.to_string())
    }
}

impl From<anyhow::Error> for DownloadError {
    fn from(err: anyhow::Error) -> Self {
        Self::Unknown(err.to_string())
    }
}

impl From<serde_json::Error> for DownloadError {
    fn from(err: serde_json::Error) -> Self {
        Self::parse(format!("JSON解析失败: {err}"))
    }
}

impl DownloadError {
    /// 创建 HTTP 错误
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

    /// 创建 `FFmpeg` 错误
    pub fn ffmpeg(error: impl Into<String>) -> Self {
        Self::FfmpegError {
            error: error.into(),
        }
    }

    /// 创建 URL 验证错误
    pub fn url_validation(url: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::UrlValidationError {
            url: url.into(),
            reason: reason.into(),
        }
    }

    /// 创建配置验证错误
    pub fn validation(field: impl Into<String>, error: impl Into<String>) -> Self {
        Self::ValidationError {
            field: field.into(),
            error: error.into(),
        }
    }
}
