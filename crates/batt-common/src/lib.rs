use std::fs::OpenOptions;
use std::io::Write as _;
use std::time::SystemTime;

pub const LOG_PATH: &str = "/data/opbatt/battchg.log";

/// 返回格式化时间戳字符串 "[YYYY-MM-DD-HH:MM:SS]"
pub fn get_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // 秒数 -> 年月日时分秒（手动计算，避免外部依赖）
    let (year, month, day, hour, min, sec) = epoch_to_ymdhms(now);

    format!("[{year:04}-{month:02}-{day:02}-{hour:02}:{min:02}:{sec:02}]")
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

/// 将 Unix 秒数转换为 (年, 月, 日, 时, 分, 秒)，使用 UTC
fn epoch_to_ymdhms(mut secs: u64) -> (u16, u8, u8, u8, u8, u8) {
    const SECS_PER_DAY: u64 = 86400;

    let days = (secs / SECS_PER_DAY) as u32;
    secs %= SECS_PER_DAY;

    let hour = (secs / 3600) as u8;
    let min = ((secs % 3600) / 60) as u8;
    let sec = (secs % 60) as u8;

    let (year, month, day) = days_to_ymd(days);
    (year, month, day, hour, min, sec)
}

/// 从 1970-01-01 起的天数推算年月日
fn days_to_ymd(mut days: u32) -> (u16, u8, u8) {
    // 先估算年份
    let mut year = 1970u16;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let leap = is_leap(year);
    let mut month = 1u8;
    for &days_in_month in month_days(leap).iter() {
        if days < days_in_month {
            break;
        }
        days -= days_in_month;
        month += 1;
    }

    (year, month, (days + 1) as u8)
}

fn is_leap(year: u16) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn month_days(leap: bool) -> [u32; 12] {
    [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ]
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
    fn test_is_leap() {
        assert!(!is_leap(1900));
        assert!(is_leap(2000));
        assert!(is_leap(2024));
        assert!(!is_leap(2023));
    }
}
