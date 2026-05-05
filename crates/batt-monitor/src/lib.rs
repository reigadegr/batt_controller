use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use batt_config::BattConfig;
use batt_sysfs::read_battery_log;

/* ------------------------------------------------------------------ */
/* BatteryLog: battery_log_content 解析结果                             */
/* ------------------------------------------------------------------ */

#[derive(Debug, Clone, Default)]
pub struct BatteryLog {
    pub temp_raw: i32, // [1] battery/temp 原始值 (0.1°C)
    pub temp_01c: i32, // [2] 温度 0.1°C
    pub vbat_mv: i32,  // [3] 电池电压 mV
    pub vbus_mv: i32,  // [4] 总线电压 mV
    pub ibat_ma: i32,  // [5] 电池电流 mA (负值=充电)
    pub chip_soc: i32, // [6] 芯片 SoC %
    pub ui_soc: i32,   // [7] UI SoC %
    pub chg_sts: i32,  // [8] 充电状态
    pub fcc_mah: i32,  // [12] 累积充电量 mAh
}

/* ------------------------------------------------------------------ */
/* SharedState: 全局共享状态                                            */
/* ------------------------------------------------------------------ */

pub struct SharedState {
    pub usb_online: AtomicBool,
    pub charging_active: AtomicBool,
    pub running: AtomicBool,
    pub config: Mutex<BattConfig>,
    pub blog: Mutex<BatteryLog>,
}

impl SharedState {
    #[must_use]
    pub fn new(config: BattConfig) -> Self {
        Self {
            usb_online: AtomicBool::new(false),
            charging_active: AtomicBool::new(false),
            running: AtomicBool::new(true),
            config: Mutex::new(config),
            blog: Mutex::new(BatteryLog::default()),
        }
    }
}

/* ------------------------------------------------------------------ */
/* parse_battery_log: 解析 battery_log_content 字段                     */
/* 格式: ,[f1],[f2],[f3],...,[f12],...                                 */
/* 前导逗号跳过，字段从索引 0 开始                                       */
/* ------------------------------------------------------------------ */

#[must_use]
pub fn parse_battery_log(buf: &str) -> BatteryLog {
    let mut fields = [0i32; 20];
    let mut count = 0;

    // 跳过前导逗号
    let s = buf.strip_prefix(',').unwrap_or(buf);

    for token in s.split(',').take(20) {
        // 截断到换行符
        let token = token.find('\n').map_or(token, |pos| &token[..pos]);
        fields[count] = token.trim().parse::<i32>().unwrap_or(0);
        count += 1;
    }

    let mut blog = BatteryLog::default();

    if count >= 8 {
        blog.temp_raw = fields[0]; // [1] battery_temp raw
        blog.temp_01c = fields[1]; // [2] temp_01c
        blog.vbat_mv = fields[2]; // [3] 电池电压 mV
        blog.vbus_mv = fields[3]; // [4] 总线电压 mV
        blog.ibat_ma = fields[4]; // [5] 电池电流 mA
        blog.chip_soc = fields[5]; // [6] chip_soc %
        blog.ui_soc = fields[6]; // [7] ui_soc %
        blog.chg_sts = fields[7]; // [8] 充电状态
    }
    if count >= 12 {
        blog.fcc_mah = fields[11]; // [12] 累积充电量 mAh
    }

    blog
}

/* ------------------------------------------------------------------ */
/* monitor_usb_thread: USB 在线监控线程                                  */
/* 每 2s 轮询 usb/online，设置 usb_online 和 charging_active           */
/* ------------------------------------------------------------------ */

pub fn monitor_usb_thread(state: &Arc<SharedState>) {
    let mut prev_online = false;

    while state.running.load(Ordering::Relaxed) {
        let online = batt_sysfs::read_usb_online().unwrap_or(false);

        if online && !prev_online {
            state.usb_online.store(true, Ordering::Relaxed);
            state.charging_active.store(true, Ordering::Relaxed);
        } else if !online && prev_online {
            state.usb_online.store(false, Ordering::Relaxed);
            state.charging_active.store(false, Ordering::Relaxed);
        }
        prev_online = online;

        thread::sleep(Duration::from_secs(2));
    }
}

/* ------------------------------------------------------------------ */
/* monitor_battery_log_thread: 电池日志监控线程                          */
/* 每 5s 读取 battery_log_content 并解析                                */
/* ------------------------------------------------------------------ */

pub fn monitor_battery_log_thread(state: &Arc<SharedState>) {
    while state.running.load(Ordering::Relaxed) {
        if state.usb_online.load(Ordering::Relaxed)
            && let Some(buf) = read_battery_log()
        {
            let blog = parse_battery_log(&buf);
            *state
                .blog
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = blog;
        }

        thread::sleep(Duration::from_secs(5));
    }
}
