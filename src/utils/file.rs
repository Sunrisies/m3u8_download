use anyhow::{Result, anyhow};
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// 检查TS文件是否有效（通过检查文件头）
pub fn is_valid_ts_file(path: &Path) -> bool {
    match File::open(path) {
        Ok(mut file) => {
            let mut buffer = [0u8; 4];
            if file.read_exact(&mut buffer).is_ok() {
                // TS文件通常以0x47开头（MPEG-TS同步字节）
                buffer[0] == 0x47
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

/// 解析URL
pub fn resolve_url(base_url: &url::Url, url: &str) -> Result<String> {
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(url.to_string())
    } else {
        base_url
            .join(url)
            .map(|u| u.to_string())
            .map_err(|e| anyhow!(format!("URL解析失败: {}", e)))
    }
}

/// 从segment URI中提取文件名
pub fn get_segment_filename(segment_uri: &str, index: usize) -> String {
    Path::new(segment_uri)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&format!("segment_{:06}.ts", index))
        .to_string()
}
