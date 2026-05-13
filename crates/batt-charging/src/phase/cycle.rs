use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use batt_common::{get_timestamp, log_write};

use crate::ChargePhase;
use crate::charging::{choose_protocol, clamp_max_ma, dumpsys_reset, read_voters_3x};
use crate::loop_::LoopCtx;

/// 默认 UFCS 重置延迟秒数
const DEFAULT_UFCS_RESET_DELAY: i32 = 10;
/// 默认循环间隔毫秒数
const DEFAULT_LOOP_INTERVAL_MS: i32 = 450;

/// 充电周期结束处理（含 dumpsys reset + 协议切换）
/// 返回 true 表示执行了周期结束逻辑（调用方应 continue）
pub fn handle_cycle_end(c: &mut LoopCtx<'_>) -> bool {
    // strace 确认: thermal_hi 降到 <=20 且已进入充电周期时重置
    if c.parms.thermal_hi > 0 {
        c.in_charge_cycle = 1;
    }

    if c.parms.thermal_hi > 20 || c.in_charge_cycle == 0 {
        return false;
    }

    // 重置所有 votable
    batt_sysfs::reset_votables();

    // dumpsys 电池控制序列
    dumpsys_reset();

    // 重置计数限制: 超过 max_ufcs_chg_reset_cc 后等待 ufcs_reset_delay
    c.restart_count += 1;
    if c.cfg.max_ufcs_chg_reset_cc > 0 && c.restart_count > c.cfg.max_ufcs_chg_reset_cc {
        let ts = get_timestamp();
        let delay = if c.cfg.ufcs_reset_delay > 0 {
            c.cfg.ufcs_reset_delay
        } else {
            DEFAULT_UFCS_RESET_DELAY
        };
        log_write(&format!(
            "{ts} ==== Reset limit reached ({}/{}) delay {}s ====\n",
            c.restart_count,
            c.cfg.max_ufcs_chg_reset_cc,
            delay
        ));
        let delay_ms = if c.cfg.loop_interval_ms > 0 {
            c.cfg.loop_interval_ms
        } else {
            DEFAULT_LOOP_INTERVAL_MS
        };
        // SAFETY: .max(0) 保证非负, 充电配置值不会超出 u32 范围
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let iterations = (i64::from(delay) * 1000 / i64::from(delay_ms)).max(0) as u32;
        for _ in 0..iterations {
            if !c.running.load(Ordering::Relaxed) {
                break;
            }
            thread::sleep(Duration::from_millis(delay_ms.unsigned_abs().into()));
        }
        c.restart_count = 0;
    }

    // 根据 bcc_parms 决定下一周期的协议
    c.use_ufcs = choose_protocol(c.cfg, &c.parms);

    // 重新读取 voter 信息确定新的最大电流和步长
    read_voters_3x(&mut c.voters);

    c.cable_max = c.voters.cable_max_ma;
    c.max_ma = clamp_max_ma(
        if c.use_ufcs != 0 {
            c.cfg.ufcs_max
        } else {
            c.cfg.pps_max
        },
        if c.use_ufcs != 0 {
            c.parms.ufcs_max_ma
        } else {
            c.parms.pps_max_ma
        },
        c.cable_max,
    );

    // strace 确认: inc_step = effective_max / 10 (非 step_ma / 10)
    c.inc_step = if c.max_ma > 0 {
        c.max_ma / 10
    } else {
        c.cfg.inc_step
    };

    let ts = get_timestamp();
    log_write(&format!(
        "{ts} ==== Charger type {}, set max current {}mA (restart #{}) ====\n",
        if c.use_ufcs != 0 { "UFCS" } else { "PPS" },
        c.max_ma,
        c.restart_count
    ));

    c.current_ma = 500;
    c.ramp_idx = 0;
    c.cv_step_idx = 0;
    c.cv_holding = 0;
    c.rise_max_reached = 0;
    c.in_charge_cycle = 0;
    c.phase = ChargePhase::RestartRise;
    true
}
