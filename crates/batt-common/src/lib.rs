use std::fs::OpenOptions;
use std::io::Write as _;
use std::time::SystemTime;

pub mod error;

pub use error::BattError;

pub const LOG_PATH: &str = "/data/opbatt/battchg.log";

/// 返回格式化时间戳字符串 "[YYYY-MM-DD-HH:MM:SS]"
#[must_use]
pub fn get_timestamp() -> String {
    let epoch = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .cast_signed();

    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&raw const epoch, &raw mut tm) };

    format!(
        "[{:04}-{:02}-{:02}-{:02}:{:02}:{:02}]",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
    )
}

/// 同时写入 stdout 和日志文件；文件写入失败时仅忽略错误
pub fn log_write(msg: &str) {
    print!("{msg}");
    // 忽略 stdout flush 错误
    let _ = std::io::stdout().flush();

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(LOG_PATH) {
        let _ = file.write_all(msg.as_bytes());
    }
}

// ---------------------------------------------------------------------------
// 日志宏: 带级别的日志输出，替代直接 log_write() 调用
// ---------------------------------------------------------------------------

/// 日志级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Debug => f.write_str("DEBUG"),
            Self::Info => f.write_str("INFO"),
            Self::Warn => f.write_str("WARN"),
            Self::Error => f.write_str("ERROR"),
        }
    }
}

/// 内部日志写入函数
pub fn log_with_level(level: LogLevel, msg: &str) {
    let ts = get_timestamp();
    let formatted = format!("{ts} [{level}] {msg}\n");
    log_write(&formatted);
}

/// Debug 级别日志
#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        $crate::log_with_level($crate::LogLevel::Debug, &format!($($arg)*))
    };
}

/// Info 级别日志
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        $crate::log_with_level($crate::LogLevel::Info, &format!($($arg)*))
    };
}

/// Warn 级别日志
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        $crate::log_with_level($crate::LogLevel::Warn, &format!($($arg)*))
    };
}

/// Error 级别日志
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        $crate::log_with_level($crate::LogLevel::Error, &format!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_timestamp_format() {
        let ts = get_timestamp();
        // 格式: [YYYY-MM-DD-HH:MM:SS]
        assert!(ts.starts_with('['));
        assert!(ts.ends_with(']'));
        assert_eq!(ts.len(), 21); // [ + 19 chars + ]
    }

    #[test]
    fn test_log_level_ordering() {
        assert!(LogLevel::Debug < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Error);
    }
}
