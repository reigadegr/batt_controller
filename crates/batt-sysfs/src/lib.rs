use std::ffi::CString;
use std::os::unix::io::RawFd;

use libc::{
    O_CLOEXEC, O_RDONLY, O_TRUNC, O_WRONLY, SEEK_SET, atoi, close, lseek, open, read, write,
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
            return Err(-1);
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
        lseek(fd, 0, SEEK_SET);
        let mut buf = [0u8; 16];
        let n = read(fd, buf.as_mut_ptr().cast(), buf.len() - 1);
        if n <= 0 {
            return None;
        }
        buf[n.cast_unsigned()] = 0;
        let v = atoi(buf.as_ptr().cast());
        Some(v)
    }
}

/// 读取字符串（lseek 到开头再读）
#[must_use]
pub fn read_str(fd: RawFd) -> Option<String> {
    if fd < 0 {
        return None;
    }
    unsafe {
        lseek(fd, 0, SEEK_SET);
        let mut buf = [0u8; 512];
        let n = read(fd, buf.as_mut_ptr().cast(), buf.len() - 1);
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

/// 写入整数（仅用于 sysfs fd）
pub fn write_int(fd: RawFd, value: i32) -> Result<(), i32> {
    if fd < 0 {
        return Err(-1);
    }
    let s = c_string_from(format!("{value}"));
    unsafe {
        lseek(fd, 0, SEEK_SET);
        let n = write(fd, s.as_ptr().cast(), s.to_bytes().len());
        if n < 0 { Err(-1) } else { Ok(()) }
    }
}

/// 写入字符串（仅用于 sysfs fd）
pub fn write_str(fd: RawFd, val: &str) -> Result<(), i32> {
    if fd < 0 {
        return Err(-1);
    }
    let s = c_string_from(val);
    unsafe {
        lseek(fd, 0, SEEK_SET);
        let n = write(fd, s.as_ptr().cast(), s.to_bytes().len());
        if n < 0 { Err(-1) } else { Ok(()) }
    }
}

/* ------------------------------------------------------------------ */
/* proc 文件 open-write-close 模式                                     */
/* ------------------------------------------------------------------ */

/// 向 proc 文件写入整数（open-write-close）
pub fn write_proc_int(path: &str, value: i32) -> Result<(), i32> {
    let data = c_string_from(format!("{value}"));
    write_proc_raw(path, data.to_bytes())
}

/// 向 proc 文件写入字符串（open-write-close）
pub fn write_proc_str(path: &str, val: &str) -> Result<(), i32> {
    let data = c_string_from(val);
    write_proc_raw(path, data.to_bytes())
}

fn write_proc_raw(path: &str, data: &[u8]) -> Result<(), i32> {
    let c_path = c_string_from(path);
    unsafe {
        let fd = open(c_path.as_ptr(), O_WRONLY | O_TRUNC | O_CLOEXEC);
        if fd < 0 {
            return Err(-1);
        }
        let n = write(fd, data.as_ptr().cast(), data.len());
        close(fd);
        if n < 0 { Err(-1) } else { Ok(()) }
    }
}

/* ------------------------------------------------------------------ */
/* votable 重置                                                        */
/* ------------------------------------------------------------------ */

/// 重置 4 个 votable 节点为 "0"
pub fn reset_votables() {
    let _ = write_proc_str(PROC_PPS_FORCE_VAL, "0");
    let _ = write_proc_str(PROC_PPS_FORCE_ACTIVE, "0");
    let _ = write_proc_str(PROC_UFCS_FORCE_VAL, "0");
    let _ = write_proc_str(PROC_UFCS_FORCE_ACTIVE, "0");
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
    let c_path = c_string_from(PATH_USB_ONLINE);
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
        Some(atoi(buf.as_ptr().cast()) > 0)
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

/// 构造 `CString`，路径常量不含 null 字节，调用处可保证安全
#[allow(clippy::unwrap_used, clippy::expect_used)]
fn c_string_from(s: impl Into<Vec<u8>>) -> CString {
    CString::new(s).expect("unexpected null byte in sysfs/proc path")
}

fn open_ro(path: &str) -> RawFd {
    let c = c_string_from(path);
    unsafe { open(c.as_ptr(), O_RDONLY | O_CLOEXEC) }
}

fn open_wo(path: &str) -> RawFd {
    let c = c_string_from(path);
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

fn read_temp_file(path: &str) -> Option<String> {
    let c_path = c_string_from(path);
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
