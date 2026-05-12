use batt_common::{get_timestamp, log_write};

use crate::charging::write_current;
use crate::loop_::LoopCtx;

use super::sleep_or_stop;

/// 去极化阶段各步骤间隔 (ms)
const DEPOL_STEP_INTERVAL_MS: u64 = 500;
/// 默认脉冲电流 (mA)
const DEFAULT_DEPOL_PULSE_MA: i32 = 500;
/// 默认负步进 (mA)
const DEFAULT_DEPOL_NEG_STEP: i32 = 150;
/// DEPOL 结束后写入的电流值 (mA)
const DEPOL_EXIT_CURRENT_MA: i32 = 1000;

/// DEPOL 去极化阶段
pub fn exec_depol(c: &mut LoopCtx<'_>) {
    // 去极化阶段 (strace 2026-04-28 完整周期确认):
    // 完整序列: 50→-100→500→300→250→50→0→-50→-200→-350→500→300→250→50→1000
    // 两轮脉冲+负值去极化。force_val 确实写入负值。
    let pulse = if c.cfg.depol_pulse_ma > 0 {
        c.cfg.depol_pulse_ma
    } else {
        DEFAULT_DEPOL_PULSE_MA
    };
    let neg_step = if c.cfg.depol_neg_step > 0 {
        c.cfg.depol_neg_step
    } else {
        DEFAULT_DEPOL_NEG_STEP
    };

    // Round 1: 50 → 初始负值 → 脉冲下降至 0
    let _ = write_current(c.use_ufcs, 50);
    if !sleep_or_stop(c, DEPOL_STEP_INTERVAL_MS) {
        return;
    }
    let _ = write_current(c.use_ufcs, -(neg_step * 2 / 3));
    if !sleep_or_stop(c, DEPOL_STEP_INTERVAL_MS) {
        return;
    }

    let _ = write_current(c.use_ufcs, pulse);
    if !sleep_or_stop(c, DEPOL_STEP_INTERVAL_MS) {
        return;
    }
    let _ = write_current(c.use_ufcs, 300);
    if !sleep_or_stop(c, DEPOL_STEP_INTERVAL_MS) {
        return;
    }
    let _ = write_current(c.use_ufcs, 250);
    if !sleep_or_stop(c, DEPOL_STEP_INTERVAL_MS) {
        return;
    }
    let _ = write_current(c.use_ufcs, 50);
    if !sleep_or_stop(c, DEPOL_STEP_INTERVAL_MS) {
        return;
    }
    let _ = write_current(c.use_ufcs, c.cfg.depol_zero_ma);
    if !sleep_or_stop(c, DEPOL_STEP_INTERVAL_MS) {
        return;
    }

    // Round 2: 负值递减 + 脉冲下降
    let mut neg = -50;
    for _ in 0..3 {
        if !sleep_or_stop(c, DEPOL_STEP_INTERVAL_MS) {
            return;
        }
        let _ = write_current(c.use_ufcs, neg);
        neg -= neg_step;
    }

    let _ = write_current(c.use_ufcs, pulse);
    if !sleep_or_stop(c, DEPOL_STEP_INTERVAL_MS) {
        return;
    }
    let _ = write_current(c.use_ufcs, 300);
    if !sleep_or_stop(c, DEPOL_STEP_INTERVAL_MS) {
        return;
    }
    let _ = write_current(c.use_ufcs, 250);
    if !sleep_or_stop(c, DEPOL_STEP_INTERVAL_MS) {
        return;
    }
    let _ = write_current(c.use_ufcs, 50);
    if !sleep_or_stop(c, DEPOL_STEP_INTERVAL_MS) {
        return;
    }

    // strace 确认: DEPOL 结束后写 1000 进入 FULL
    let _ = write_current(c.use_ufcs, DEPOL_EXIT_CURRENT_MA);
    if !sleep_or_stop(c, DEPOL_STEP_INTERVAL_MS) {
        return;
    }

    let ts = get_timestamp();
    log_write(&format!(
        "{ts} ==== DEPOL complete, preparing restart ====\n"
    ));

    c.current_ma = DEPOL_EXIT_CURRENT_MA;
    c.ramp_idx = 0;
    c.cv_step_idx = 0;
    c.cv_holding = 0;
    c.rise_max_reached = 0;
}
