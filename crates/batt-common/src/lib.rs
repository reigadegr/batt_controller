use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::sync::Mutex;
use std::time::SystemTime;

pub const LOG_PATH: &str = "/data/opbatt/battchg.log";

static LOG_FILE: Mutex<Option<File>> = Mutex::new(None);

fn get_log_file() -> Option<std::sync::MutexGuard<'static, Option<File>>> {
    let mut guard = LOG_FILE.lock().ok()?;
    if guard.is_none() {
        *guard = Some(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(LOG_PATH)
                .ok()?,
        );
    }
    Some(guard)
}

/// 返回格式化时间戳字符串 "[YYYY-MM-DD-HH:MM:SS]"
#[must_use]
pub fn get_timestamp() -> String {
    let epoch = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_secs().cast_signed(),
        Err(_) => 0,
    };

    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    unsafe { libc::localtime_r(&raw const epoch, tm.as_mut_ptr()) };
    let tm = unsafe { tm.assume_init() };

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

    if let Some(mut guard) = get_log_file()
        && let Some(ref mut file) = *guard
    {
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
