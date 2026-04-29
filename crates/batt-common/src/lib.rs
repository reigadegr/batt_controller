use std::fs::OpenOptions;
use std::io::Write as _;
use std::time::SystemTime;

pub const LOG_PATH: &str = "/data/opbatt/battchg.log";

/// 返回格式化时间戳字符串 "[YYYY-MM-DD-HH:MM:SS]"
pub fn get_timestamp() -> String {
    let epoch = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as libc::time_t;

    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&epoch, &mut tm) };

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
}
