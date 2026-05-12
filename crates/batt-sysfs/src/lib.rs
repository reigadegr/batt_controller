use std::ffi::{CStr, CString};
use std::os::unix::io::RawFd;

use libc::{
    O_CLOEXEC, O_RDONLY, O_TRUNC, O_WRONLY, SEEK_SET, close, lseek, open, read, write,
};

/* ------------------------------------------------------------------ */
/* sysfs/proc 路径常量                                                 */
/* ------------------------------------------------------------------ */

pub const PATH_USB_ONLINE: &str = "/sys/class/power_supply/usb/online";
pub const PATH_BATTERY_TEMP: &str = "/sys/class/power_supply/battery/temp";
pub const PATH_CHIP_SOC: &str = "/sys/class/oplus_chg/battery/chip_soc";
pub const PATH_UFCS_STATUS: &str = "/proc/oplus-votable/UFCS_CURR/status";
pub const PATH_ADAPTER_POWER: &str = "/sys/class/oplus_chg/common/adapter_power";
pub const PATH_BCC_CURRENT: &str = "/sys/class/oplus_chg/battery/bcc_current";
pub const PATH_MMI_CHARGING: &str = "/sys/class/oplus_chg/battery/mmi_charging_enable";
pub const PATH_BCC_PARMS: &str = "/sys/class/oplus_chg/battery/bcc_parms";
pub const PATH_BATTERY_LOG: &str = "/sys/class/oplus_chg/battery/battery_log_content";

pub const PROC_PPS_FORCE_VAL: &str = "/proc/oplus-votable/PPS_CURR/force_val";
pub const PROC_PPS_FORCE_ACTIVE: &str = "/proc/oplus-votable/PPS_CURR/force_active";
pub const PROC_UFCS_FORCE_VAL: &str = "/proc/oplus-votable/UFCS_CURR/force_val";
pub const PROC_UFCS_FORCE_ACTIVE: &str = "/proc/oplus-votable/UFCS_CURR/force_active";

/* ------------------------------------------------------------------ */
/* SysfsFds: 持久持有的 sysfs 文件描述符集合                             */
/* ------------------------------------------------------------------ */

#[derive(Debug)]
pub struct SysfsFds {
    pub usb_online: RawFd,
    pub battery_temp: RawFd,
    pub chip_soc: RawFd,
    pub adapter_power: RawFd,
    pub bcc_current: RawFd,
    pub mmi_charging_enable: RawFd,
}

impl SysfsFds {
    /// 打开所有 sysfs 节点，`usb_online` 打开失败时返回错误
    pub fn open_all() -> Result<Self, i32> {
        let fds = Self {
            usb_online: open_ro(PATH_USB_ONLINE),
            battery_temp: open_ro(PATH_BATTERY_TEMP),
            chip_soc: open_ro(PATH_CHIP_SOC),
            adapter_power: open_ro(PATH_ADAPTER_POWER),
            bcc_current: open_wo(PATH_BCC_CURRENT),
            mmi_charging_enable: open_wo(PATH_MMI_CHARGING),
        };
        if fds.usb_online < 0 {
            eprintln!("warn: failed to open {PATH_USB_ONLINE}");
            return Err(-1);
        }
        // 非关键 fd 打开失败时记录警告
        for (fd, path) in [
            (fds.battery_temp, PATH_BATTERY_TEMP),
            (fds.chip_soc, PATH_CHIP_SOC),
            (fds.adapter_power, PATH_ADAPTER_POWER),
            (fds.bcc_current, PATH_BCC_CURRENT),
            (fds.mmi_charging_enable, PATH_MMI_CHARGING),
        ] {
            if fd < 0 {
                eprintln!("warn: failed to open {path}");
            }
        }
        Ok(fds)
    }

    /// 关闭所有 fd 并置为 -1
    pub fn close_all(&mut self) {
        close_fd(&mut self.usb_online);
        close_fd(&mut self.battery_temp);
        close_fd(&mut self.chip_soc);
        close_fd(&mut self.adapter_power);
        close_fd(&mut self.bcc_current);
        close_fd(&mut self.mmi_charging_enable);
    }
}

impl Drop for SysfsFds {
    fn drop(&mut self) {
        self.close_all();
    }
}

/* ------------------------------------------------------------------ */
/* 读写原语                                                            */
/* ------------------------------------------------------------------ */

/// 读取整数（lseek 到开头再读）
#[must_use]
pub fn read_int(fd: RawFd) -> Option<i32> {
    if fd < 0 {
        return None;
    }
    unsafe {
        if lseek(fd, 0, SEEK_SET) < 0 {
            return None;
        }
        let mut buf = [0u8; 16];
        let n = read(fd, buf.as_mut_ptr().cast(), buf.len() - 1);
        if n <= 0 {
            return None;
        }
        buf[n.cast_unsigned()] = 0;
        parse_int_from_buf(&buf)
    }
}

/// 写入整数（仅用于 sysfs fd）
pub fn write_int(fd: RawFd, value: i32) -> Result<(), i32> {
    if fd < 0 {
        return Err(-1);
    }
    let Ok(s) = c_string_from(format!("{value}")) else {
        eprintln!("write_int: invalid value: {value}");
        return Err(-1);
    };
    unsafe {
        if lseek(fd, 0, SEEK_SET) < 0 {
            return Err(-1);
        }
        let n = write(fd, s.as_ptr().cast(), s.to_bytes().len());
        if n < 0 || n.cast_unsigned() < s.to_bytes().len() {
            Err(-1)
        } else {
            Ok(())
        }
    }
}

/* ------------------------------------------------------------------ */
/* proc 文件 open-write-close 模式                                     */
/* ------------------------------------------------------------------ */

/// 向 proc 文件写入整数（open-write-close）
pub fn write_proc_int(path: &str, value: i32) -> Result<(), i32> {
    let Ok(data) = c_string_from(format!("{value}")) else {
        eprintln!("write_proc_int: invalid value: {value}");
        return Err(-1);
    };
    write_proc_raw(path, data.to_bytes())
}

/// 向 proc 文件写入字符串（open-write-close）
pub fn write_proc_str(path: &str, val: &str) -> Result<(), i32> {
    let Ok(data) = c_string_from(val) else {
        eprintln!("write_proc_str: invalid value: {val}");
        return Err(-1);
    };
    write_proc_raw(path, data.to_bytes())
}

fn write_proc_raw(path: &str, data: &[u8]) -> Result<(), i32> {
    let Ok(c_path) = c_string_from(path) else {
        eprintln!("write_proc_raw: invalid path: {path}");
        return Err(-1);
    };
    unsafe {
        let fd = open(c_path.as_ptr(), O_WRONLY | O_TRUNC | O_CLOEXEC);
        if fd < 0 {
            return Err(-1);
        }
        let n = write(fd, data.as_ptr().cast(), data.len());
        close(fd);
        if n < 0 || n.cast_unsigned() < data.len() {
            Err(-1)
        } else {
            Ok(())
        }
    }
}

/* ------------------------------------------------------------------ */
/* votable 重置                                                        */
/* ------------------------------------------------------------------ */

/// 重置 4 个 votable 节点为 "0"
pub fn reset_votables() {
    for (path, name) in [
        (PROC_PPS_FORCE_VAL, "PPS_CURR/force_val"),
        (PROC_PPS_FORCE_ACTIVE, "PPS_CURR/force_active"),
        (PROC_UFCS_FORCE_VAL, "UFCS_CURR/force_val"),
        (PROC_UFCS_FORCE_ACTIVE, "UFCS_CURR/force_active"),
    ] {
        if let Err(e) = write_proc_str(path, "0") {
            eprintln!("warn: failed to reset {name}: {e}");
        }
    }
}

/* ------------------------------------------------------------------ */
/* 临时打开读取（open-read-close）                                      */
/* ------------------------------------------------------------------ */

/// 临时读取 `bcc_parms`
#[must_use]
pub fn read_bcc_parms() -> Option<String> {
    read_temp_file(PATH_BCC_PARMS)
}

/// 临时读取 usb/online 状态 (open-read-close)
#[must_use]
pub fn read_usb_online() -> Option<bool> {
    let Ok(c_path) = c_string_from(PATH_USB_ONLINE) else {
        eprintln!("read_usb_online: invalid path");
        return None;
    };
    unsafe {
        let fd = open(c_path.as_ptr(), O_RDONLY | O_CLOEXEC);
        if fd < 0 {
            return None;
        }
        let mut buf = [0u8; 16];
        let n = read(fd, buf.as_mut_ptr().cast(), buf.len() - 1);
        close(fd);
        if n <= 0 {
            return None;
        }
        buf[n.cast_unsigned()] = 0;
        Some(parse_int_from_buf(&buf).is_some_and(|v| v > 0))
    }
}

/// 临时读取 `battery_log`
#[must_use]
pub fn read_battery_log() -> Option<String> {
    read_temp_file(PATH_BATTERY_LOG)
}

/// 临时读取 ufcs voters
#[must_use]
pub fn read_ufcs_voters() -> Option<String> {
    read_temp_file(PATH_UFCS_STATUS)
}

/* ------------------------------------------------------------------ */
/* 内部辅助函数                                                        */
/* ------------------------------------------------------------------ */

/// 构造 `CString`，路径常量不含 null 字节
///
/// # Errors
///
/// 当输入包含 null 字节时返回错误。
fn c_string_from(s: impl Into<Vec<u8>>) -> Result<CString, String> {
    CString::new(s).map_err(|e| format!("null byte in sysfs/proc path: {e}"))
}

fn open_ro(path: &str) -> RawFd {
    let Ok(c) = c_string_from(path) else {
        eprintln!("open_ro: invalid path: {path}");
        return -1;
    };
    unsafe { open(c.as_ptr(), O_RDONLY | O_CLOEXEC) }
}

fn open_wo(path: &str) -> RawFd {
    let Ok(c) = c_string_from(path) else {
        eprintln!("open_wo: invalid path: {path}");
        return -1;
    };
    unsafe { open(c.as_ptr(), O_WRONLY | O_CLOEXEC) }
}

fn close_fd(fd: &mut RawFd) {
    if *fd >= 0 {
        unsafe {
            close(*fd);
        }
    }
    *fd = -1;
}

/// 从 null 结尾的字节缓冲区解析整数，替代 `atoi` 以区分合法 0 与解析失败
unsafe fn parse_int_from_buf(buf: &[u8]) -> Option<i32> {
    let cstr = unsafe { CStr::from_ptr(buf.as_ptr().cast()) };
    let s = cstr.to_str().ok()?.trim();
    s.parse::<i32>().ok()
}

fn read_temp_file(path: &str) -> Option<String> {
    let Ok(c_path) = c_string_from(path) else {
        eprintln!("read_temp_file: invalid path: {path}");
        return None;
    };
    unsafe {
        let fd = open(c_path.as_ptr(), O_RDONLY | O_CLOEXEC);
        if fd < 0 {
            return None;
        }
        let mut buf = [0u8; 4096];
        let n = read(fd, buf.as_mut_ptr().cast(), buf.len() - 1);
        close(fd);
        if n <= 0 {
            return None;
        }
        buf[n.cast_unsigned()] = 0;
        Some(
            std::ffi::CStr::from_ptr(buf.as_ptr().cast())
                .to_string_lossy()
                .into_owned(),
        )
    }
}
