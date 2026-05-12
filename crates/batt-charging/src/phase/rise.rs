use crate::ChargePhase;
use crate::charging::write_current;
use crate::loop_::LoopCtx;

/// 默认重启爬升步长 (mA)
const DEFAULT_RESTART_RISE_STEP: i32 = 50;

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
            DEFAULT_RESTART_RISE_STEP
        };
        if c.current_ma < phase_max {
            c.current_ma += step;
            if c.current_ma > phase_max {
                c.current_ma = phase_max;
            }
            let _ = write_current(c.fds, c.use_ufcs, c.current_ma);
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
        let _ = write_current(c.fds, c.use_ufcs, 500);
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
        let _ = write_current(c.fds, c.use_ufcs, c.current_ma);
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

    let _ = write_current(c.fds, c.use_ufcs, c.current_ma);
    c.ramp_idx += 1;
}
