//! 配置常量模块
//! 统一管理项目中的硬编码值

/// 默认重试次数
pub const DEFAULT_RETRY_COUNT: usize = 4;

/// HTTP请求超时时间（秒）
pub const HTTP_TIMEOUT_SECONDS: u64 = 30;

/// HTTP连接超时时间（秒）
pub const HTTP_CONNECT_TIMEOUT_SECONDS: u64 = 10;

/// 连接池空闲超时时间（秒）
pub const POOL_IDLE_TIMEOUT_SECONDS: u64 = 90;

/// TCP保活时间（秒）
pub const TCP_KEEPALIVE_SECONDS: u64 = 60;

/// 默认并发下载数
pub const DEFAULT_CONCURRENT_DOWNLOADS: usize = 8;

/// 连接池最大空闲连接数
pub const POOL_MAX_IDLE_PER_HOST: usize = 32;

/// 文件写入缓冲区大小（字节）
pub const WRITE_BUFFER_SIZE: usize = 64 * 1024; // 64KB

/// WebSocket更新间隔（毫秒）
pub const WS_UPDATE_INTERVAL_MS: u64 = 500;

/// TS文件同步字节
pub const TS_SYNC_BYTE: u8 = 0x47;

/// AES密钥长度（字节）
pub const AES_KEY_LENGTH: usize = 16;
