use std::{
    fs::{self, File},
    io::Read,
    path::Path,
};

/// 校验 TS 文件是否有效（简单的包头检查）
pub fn is_valid_ts_file(path: &Path) -> bool {
    // 1. 检查文件是否存在
    if !path.exists() {
        return false;
    }

    // 2. 检查文件大小，TS 包通常为 188 字节，有效文件至少应该有一个包
    let metadata = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };

    if metadata.len() < 188 {
        return false;
    }

    // 3. 读取文件头部的几个字节进行校验
    // TS 包的 Sync Byte 是 0x47
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };

    let mut buffer = [0u8; 188 * 2]; // 读取前两个包的大小作为样本
    if let Err(_) = file.read_exact(&mut buffer) {
        return false;
    }

    // 检查前几个包的起始字节是否为 0x47
    // 检查第1个包
    if buffer[0] != 0x47 {
        return false;
    }
    // 检查第2个包（如果文件够大）
    if buffer[188] != 0x47 {
        return false;
    }

    true
}
