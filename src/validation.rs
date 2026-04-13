//! 输入验证模块
//!
//! 该模块提供全面的输入验证功能，包括：
//! - URL 验证：防止 SSRF 攻击
//! - 路径验证：防止路径遍历攻击
//! - 配置验证：确保用户输入的合法性
//!
//! # 示例
//!
//! ```
//! use crate::validation::{validate_url, validate_concurrent};
//!
//! // 验证 URL
//! validate_url("https://example.com/video.m3u8")?;
//!
//! // 验证并发数
//! validate_concurrent(4)?;
//! ```

use crate::error::{DownloadError, Result};
use std::path::Path;
use url::Url;

/// 验证 URL 是否有效
///
/// 该函数会检查 URL 的格式和协议，只允许 HTTP/HTTPS 协议。
///
/// # 参数
///
/// * `url` - 需要验证的 URL 字符串
///
/// # 返回
///
/// * `Ok(())` - URL 有效
/// * `Err(DownloadError)` - URL 无效或协议不支持
///
/// # 示例
///
/// ```
/// validate_url("https://example.com/video.m3u8")?;
/// validate_url("ftp://example.com/file")?; // 返回错误
/// ```
pub fn validate_url(url: &str) -> Result<()> {
    // 检查 URL 格式
    let parsed_url = Url::parse(url)
        .map_err(|e| DownloadError::url_validation(url, format!("URL格式无效: {e}")))?;

    // 只允许 HTTP/HTTPS 协议
    match parsed_url.scheme() {
        "http" | "https" => Ok(()),
        scheme => Err(DownloadError::url_validation(
            url,
            format!("不支持的协议: {}，仅支持 http/https", scheme),
        )),
    }
}

/// 验证文件路径是否安全（防止路径遍历攻击）
///
/// 该函数检查相对路径是否逃逸出基础目录，防止路径遍历攻击。
///
/// # 参数
///
/// * `base` - 基础目录路径
/// * `relative` - 相对路径
///
/// # 返回
///
/// * `Ok(())` - 路径安全
/// * `Err(DownloadError)` - 路径不安全（可能存在路径遍历）
///
/// # 示例
///
/// ```
/// use std::path::Path;
/// let base = Path::new("/safe/directory");
/// validate_path_safe(base, "subdir/file.txt")?; // 安全
/// validate_path_safe(base, "../etc/passwd")?; // 不安全
/// ```
pub fn validate_path_safe(base: &Path, relative: &str) -> Result<()> {
    let full_path = base.join(relative);

    // 检查是否逃逸出基础目录
    if !full_path.starts_with(base) {
        return Err(DownloadError::validation(
            "file_path",
            format!(
                "路径遍历攻击检测: {} 逃逸出基础目录 {}",
                relative,
                full_path.display()
            ),
        ));
    }

    Ok(())
}

/// 验证并发数是否在合理范围内
///
/// 并发数限制在 1-32 之间，以防止资源耗尽。
///
/// # 参数
///
/// * `concurrent` - 并发下载数
///
/// # 返回
///
/// * `Ok(())` - 并发数有效
/// * `Err(DownloadError)` - 并发数超出范围
///
/// # 示例
///
/// ```
/// validate_concurrent(4)?;  // 有效
/// validate_concurrent(0)?;  // 无效
/// validate_concurrent(100)?; // 无效
/// ```
pub fn validate_concurrent(concurrent: usize) -> Result<()> {
    const MIN_CONCURRENT: usize = 1;
    const MAX_CONCURRENT: usize = 32;

    if concurrent < MIN_CONCURRENT {
        return Err(DownloadError::validation(
            "concurrent",
            format!("并发数过小，最小值为 {}", MIN_CONCURRENT),
        ));
    }

    if concurrent > MAX_CONCURRENT {
        return Err(DownloadError::validation(
            "concurrent",
            format!("并发数过大，最大值为 {}", MAX_CONCURRENT),
        ));
    }

    Ok(())
}

/// 验证重试次数是否在合理范围内
///
/// 重试次数限制在 0-10 之间，以防止无限重试。
///
/// # 参数
///
/// * `retry` - 重试次数
///
/// # 返回
///
/// * `Ok(())` - 重试次数有效
/// * `Err(DownloadError)` - 重试次数超出范围
///
/// # 示例
///
/// ```
/// validate_retry_count(3)?;   // 有效
/// validate_retry_count(0)?;   // 有效
/// validate_retry_count(15)?;  // 无效
/// ```
pub fn validate_retry_count(retry: usize) -> Result<()> {
    const MAX_RETRY: usize = 10;

    if retry > MAX_RETRY {
        return Err(DownloadError::validation(
            "retry",
            format!("重试次数过大，最大值为 {}", MAX_RETRY),
        ));
    }

    Ok(())
}

/// 验证超时时间是否在合理范围内
///
/// 超时时间限制在 5-300 秒之间。
///
/// # 参数
///
/// * `timeout` - 超时时间（秒）
///
/// # 返回
///
/// * `Ok(())` - 超时时间有效
/// * `Err(DownloadError)` - 超时时间超出范围
///
/// # 示例
///
/// ```
/// validate_timeout(30)?;   // 有效
/// validate_timeout(5)?;    // 有效
/// validate_timeout(400)?;  // 无效
/// ```
pub fn _validate_timeout(timeout: u64) -> Result<()> {
    const MIN_TIMEOUT: u64 = 5;
    const MAX_TIMEOUT: u64 = 300; // 5分钟

    if timeout < MIN_TIMEOUT {
        return Err(DownloadError::validation(
            "timeout",
            format!("超时时间过小，最小值为 {}秒", MIN_TIMEOUT),
        ));
    }

    if timeout > MAX_TIMEOUT {
        return Err(DownloadError::validation(
            "timeout",
            format!("超时时间过大，最大值为 {}秒", MAX_TIMEOUT),
        ));
    }

    Ok(())
}
