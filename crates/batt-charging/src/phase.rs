use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use batt_common::{get_timestamp, log_write};
use batt_config::CV_STEP_MAX;

use crate::ChargePhase;
use crate::charging::{
    choose_protocol, clamp_max_ma, dumpsys_reset, get_default_cv_steps, get_temp_curr_offset,
    read_voters_3x, write_current,
};
use crate::loop_::LoopCtx;

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
        log_write(&format!(
            "{ts} ==== Reset limit reached ({}/{}) delay {}s ====\n",
            c.restart_count,
            c.cfg.max_ufcs_chg_reset_cc,
            if c.cfg.ufcs_reset_delay > 0 {
                c.cfg.ufcs_reset_delay
            } else {
                10
            }
        ));
        let delay = if c.cfg.ufcs_reset_delay > 0 {
            c.cfg.ufcs_reset_delay
        } else {
            10
        };
        let delay_ms = if c.cfg.loop_interval_ms > 0 {
            c.cfg.loop_interval_ms
        } else {
            450
        };
        for _ in 0..delay * (1000 / delay_ms) {
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

/// 计算 `effective_max`（温控 + `thermal_hi` + 所有 Voter 取最小值）
pub fn calc_effective_max(c: &mut LoopCtx<'_>) {
    let temp_offset = get_temp_curr_offset(c.cfg, c.parms.temp_01c);
    c.effective_max = c.max_ma;
    if temp_offset > 0 && c.effective_max > temp_offset {
        c.effective_max = temp_offset;
    }

    // 默认温控保护: temp_range 未配置时生效
    // temp_01c 单位 0.1°C, 300 = 30°C
    // >45°C 暂停, >40°C 降50%, <10°C 暂停, <15°C 降50%
    if c.cfg.temp_range_count == 0 {
        let t = c.parms.temp_01c;
        if !(100..=450).contains(&t) {
            c.effective_max = 0;
        } else if !(150..=400).contains(&t) {
            c.effective_max /= 2;
        }
    }

    // thermal_hi 限流: strace 确认 thermal_hi 阶梯 91→85→80 限制电流上限
    if c.parms.thermal_hi > 0 {
        let thermal_cap = c.parms.thermal_hi * 100;
        if thermal_cap > 0 && thermal_cap < c.effective_max {
            c.effective_max = thermal_cap;
        }
    }

    // STEP_VOTER 限流: strace 确认 STEP_VOTER 从 9100 变为 8000
    if c.voters.step_ma > 0 && c.voters.step_ma < c.effective_max {
        c.effective_max = c.voters.step_ma;
    }

    // 所有 Voter 取最小值: LIMIT_FCL / IMP / ADAPTER_IMAX / BASE_MAX / IC / BATT_TEMP / COOL_DOWN
    for &voter in &[
        c.voters.limit_fcl_ma,
        c.voters.imp_ma,
        c.voters.adapter_imax_ma,
        c.voters.base_max_ma,
        c.voters.ic_ma,
        c.voters.batt_temp_ma,
        c.voters.cool_down_ma,
    ] {
        if voter > 0 && voter < c.effective_max {
            c.effective_max = voter;
        }
    }
}

/// RISE / `RESTART_RISE` 阶段
pub fn exec_rise(c: &mut LoopCtx<'_>) {
    let phase_max = c.effective_max;

    if c.phase == ChargePhase::RestartRise {
        // 重启 RISE: +50mA 线性爬升, 无 quickstart
        // strace 确认 (2026-04-28 完整周期):
        // 550→600→650→...→3500, 每步 +50mA, ~480ms 间隔
        let step = if c.cfg.restart_rise_step > 0 {
            c.cfg.restart_rise_step
        } else {
            50
        };
        if c.current_ma < phase_max {
            c.current_ma += step;
            if c.current_ma > phase_max {
                c.current_ma = phase_max;
            }
            write_current(c.fds, c.use_ufcs, c.current_ma);
        } else {
            // strace 确认: 到达 phase_max 后停止写 force_val
            c.rise_max_reached = 1;
        }
        return;
    }

    // 以下为首次充电的 quickstart 三段式 RISE

    if c.current_ma == 500 && c.ramp_idx == 0 {
        // Quickstart: 写 500 后立即写高值
        // vbat >= rise_quickstep_thr: 直接跳到 cable_max-750
        // vbat < rise_quickstep_thr:  直接使用 ufcs_max_ma (bcc_parms[14])
        // 系数验证: 8000 * 14 / 80 = 1400 = ufcs_max_ma
        write_current(c.fds, c.use_ufcs, 500);
        if c.cfg.rise_quickstep_thr_mv > 0 && c.parms.vbat_mv >= c.cfg.rise_quickstep_thr_mv {
            // 高电压 quickstep: 一步逼近 cable_max
            let mut margin = (c.cable_max * 3) / 32;
            margin = ((margin + 25) / 50) * 50;
            c.current_ma = c.cable_max - margin;
            c.ramp_idx = 99; // 跳过 ramp, 直接全速步进
        } else {
            // 低电压 quickstart: 直接使用 ufcs_max_ma
            // strace 确认: 500 → 1400 (ufcs_max_ma)，系数14
            c.current_ma = if c.parms.ufcs_max_ma > 0 {
                c.parms.ufcs_max_ma
            } else {
                (c.cable_max * 14) / 80
            };
            c.current_ma = ((c.current_ma + 25) / 50) * 50;
            c.ramp_idx = 1;
        }
        // quickstart 目标不能超过 phase_max，否则后续 ramp 逻辑会跳过
        if c.current_ma > phase_max {
            c.current_ma = phase_max;
        }
        write_current(c.fds, c.use_ufcs, c.current_ma);
        return;
    }

    if c.current_ma >= phase_max {
        // strace 确认: 到达 phase_max 后停止写 force_val，静默维持
        c.rise_max_reached = 1;
        return;
    }

    let step;
    if (1..=4).contains(&c.ramp_idx) {
        // 斜坡阶段: 剩余距离除法
        let remaining = c.cable_max - c.current_ma;
        let divisor = if c.ramp_idx == 1 { 17 } else { 11 };
        step = (remaining + divisor / 2) / divisor;
        let mut step = ((step + 25) / 50) * 50;
        if step > c.inc_step {
            step = c.inc_step;
        }
        c.current_ma += step;
    } else if c.cfg.adjust_step > 0 && (5..=6).contains(&c.ramp_idx) {
        // 微调过渡: adjust_step (strace 确认 2 步)
        step = c.cfg.adjust_step;
        c.current_ma += step;
    } else {
        // 全速步进: cable_max / 10
        step = if c.cable_max > 0 {
            c.cable_max / 10
        } else {
            c.inc_step
        };
        c.current_ma += step;
    }

    if c.current_ma > phase_max {
        c.current_ma = phase_max;
    }

    write_current(c.fds, c.use_ufcs, c.current_ma);
    c.ramp_idx += 1;
}

/// CV 恒压阶段: 阶梯降流
pub fn exec_cv(c: &mut LoopCtx<'_>) {
    // 确定 CV 阶梯表: 有配置用配置, 无配置用内置默认
    if c.cfg.cv_step_count > 0 {
        let count = c.cfg.cv_step_count.min(CV_STEP_MAX);
        let step_mv = &c.cfg.cv_step_mv[..count];
        let step_ma = &c.cfg.cv_step_ma[..count];
        exec_cv_inner(c, step_mv, step_ma, count);
    } else {
        let default_steps = get_default_cv_steps(c.effective_max);
        let def_mv: Vec<i32> = default_steps.iter().map(|s| s.mv).collect();
        let def_ma: Vec<i32> = default_steps.iter().map(|s| s.ma).collect();
        exec_cv_inner(c, &def_mv, &def_ma, default_steps.len());
    }
}

#[allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]
fn exec_cv_inner(c: &mut LoopCtx<'_>, step_mv: &[i32], step_ma: &[i32], step_count: usize) {
    if c.cv_holding != 0 {
        // 阶梯已走完, 静默维持。
        // strace 确认 (2026-04-28): CV 阶段有振荡回升
        // (3000↔2950↔3000), vbat 回落时可回到较低阶梯。
        for i in 0..c.cv_step_idx as usize {
            if c.parms.vbat_mv < step_mv[i] {
                c.current_ma = if i > 0 { step_ma[i - 1] } else { c.current_ma };
                c.cv_step_idx = i as i32;
                c.cv_holding = 0;
                write_current(c.fds, c.use_ufcs, c.current_ma);
                return;
            }
        }
        return;
    }

    let mut dropped = false;
    for i in c.cv_step_idx as usize..step_count {
        if c.parms.vbat_mv >= step_mv[i] {
            c.current_ma = step_ma[i];
            c.cv_step_idx = i as i32 + 1;
            dropped = true;
        }
    }

    if dropped {
        write_current(c.fds, c.use_ufcs, c.current_ma);
        let ts = get_timestamp();
        log_write(&format!(
            "{ts} ==== CV step-down to {}mA (vbat={}mV, step={}) ====\n",
            c.current_ma, c.parms.vbat_mv, c.cv_step_idx
        ));
    }

    // 所有阶梯走完, 进入静默维持
    if step_count > 0 && c.cv_step_idx as usize >= step_count {
        c.cv_holding = 1;
        let ts = get_timestamp();
        log_write(&format!(
            "{ts} ==== CV holding at {}mA (vbat={}mV) ====\n",
            c.current_ma, c.parms.vbat_mv
        ));
    }
}

fn sleep_or_stop(c: &LoopCtx<'_>, ms: u64) -> bool {
    if !c.running.load(Ordering::Relaxed) {
        return false;
    }
    thread::sleep(Duration::from_millis(ms));
    true
}
pub fn exec_tc(c: &mut LoopCtx<'_>) {
    let mut cap = if c.cfg.tc_full_ma > 0 {
        c.cfg.tc_full_ma
    } else {
        500
    };
    if cap > c.effective_max {
        cap = c.effective_max;
    }
    if c.current_ma > cap {
        let step = if c.cfg.dec_step > 0 {
            c.cfg.dec_step
        } else {
            100
        };
        c.current_ma -= step;
        if c.current_ma < cap {
            c.current_ma = cap;
        }
        write_current(c.fds, c.use_ufcs, c.current_ma);
    }
}

/// DEPOL 去极化阶段
pub fn exec_depol(c: &mut LoopCtx<'_>) {
    // 去极化阶段 (strace 2026-04-28 完整周期确认):
    // 完整序列: 50→-100→500→300→250→50→0→-50→-200→-350→500→300→250→50→1000
    // 两轮脉冲+负值去极化。force_val 确实写入负值。
    let pulse = if c.cfg.depol_pulse_ma > 0 {
        c.cfg.depol_pulse_ma
    } else {
        500
    };
    let neg_step = if c.cfg.depol_neg_step > 0 {
        c.cfg.depol_neg_step
    } else {
        150
    };

    // Round 1: 50 → 初始负值 → 脉冲下降至 0
    write_current(c.fds, c.use_ufcs, 50);
    if !sleep_or_stop(c, 500) { return; }
    write_current(c.fds, c.use_ufcs, -(neg_step * 2 / 3));
    if !sleep_or_stop(c, 500) { return; }

    write_current(c.fds, c.use_ufcs, pulse);
    if !sleep_or_stop(c, 500) { return; }
    write_current(c.fds, c.use_ufcs, 300);
    if !sleep_or_stop(c, 500) { return; }
    write_current(c.fds, c.use_ufcs, 250);
    if !sleep_or_stop(c, 500) { return; }
    write_current(c.fds, c.use_ufcs, 50);
    if !sleep_or_stop(c, 500) { return; }
    write_current(c.fds, c.use_ufcs, 0);
    if !sleep_or_stop(c, 500) { return; }

    // Round 2: 负值递减 + 脉冲下降
    let mut neg = -50;
    for _ in 0..3 {
        if !sleep_or_stop(c, 500) { return; }
        write_current(c.fds, c.use_ufcs, neg);
        neg -= neg_step;
    }

    write_current(c.fds, c.use_ufcs, pulse);
    if !sleep_or_stop(c, 500) { return; }
    write_current(c.fds, c.use_ufcs, 300);
    if !sleep_or_stop(c, 500) { return; }
    write_current(c.fds, c.use_ufcs, 250);
    if !sleep_or_stop(c, 500) { return; }
    write_current(c.fds, c.use_ufcs, 50);
    if !sleep_or_stop(c, 500) { return; }

    // strace 确认: DEPOL 结束后写 1000 进入 FULL
    write_current(c.fds, c.use_ufcs, 1000);
    if !sleep_or_stop(c, 500) { return; }

    let ts = get_timestamp();
    log_write(&format!(
        "{ts} ==== DEPOL complete, preparing restart ====\n"
    ));

    c.current_ma = 1000;
    c.ramp_idx = 0;
    c.cv_step_idx = 0;
    c.cv_holding = 0;
    c.rise_max_reached = 0;
}
