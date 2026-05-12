use batt_common::{get_timestamp, log_write};
use batt_config::CV_STEP_MAX;

use crate::charging::write_current;
use crate::loop_::LoopCtx;

/// 默认 CV 阶梯电压 (mV)
const DEFAULT_CV_STEP_MV: [i32; 4] = [4450, 4480, 4500, 4520];
/// 默认 CV 阶梯电流 (mA) 索引: [0]=half, [1]=1000, [2]=500, [3]=200
const DEFAULT_CV_STEP_MA_SUFFIX: [i32; 3] = [1000, 500, 200];

/// CV 恒压阶段: 阶梯降流
pub fn exec_cv(c: &mut LoopCtx<'_>) {
    if c.cfg.cv_step_count > 0 {
        let count = c.cfg.cv_step_count.min(CV_STEP_MAX);
        let step_mv = &c.cfg.cv_step_mv[..count];
        let step_ma = &c.cfg.cv_step_ma[..count];
        exec_cv_inner(c, step_mv, step_ma, count);
    } else {
        let half = (c.effective_max + 1) / 2;
        let half = ((half + 25) / 50) * 50;
        let def_ma = [
            half,
            DEFAULT_CV_STEP_MA_SUFFIX[0],
            DEFAULT_CV_STEP_MA_SUFFIX[1],
            DEFAULT_CV_STEP_MA_SUFFIX[2],
        ];
        exec_cv_inner(c, &DEFAULT_CV_STEP_MV, &def_ma, DEFAULT_CV_STEP_MV.len());
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
                let _ = write_current(c.fds, c.use_ufcs, c.current_ma);
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
        let _ = write_current(c.fds, c.use_ufcs, c.current_ma);
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
