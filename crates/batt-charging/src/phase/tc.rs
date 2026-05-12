use crate::charging::write_current;
use crate::loop_::LoopCtx;

/// 默认涓流充电电流 (mA)
const DEFAULT_TC_FULL_MA: i32 = 500;
/// 默认电流递减步长 (mA)
const DEFAULT_DEC_STEP: i32 = 100;

/// TC 涓流阶段
pub fn exec_tc(c: &mut LoopCtx<'_>) {
    let mut cap = if c.cfg.tc_full_ma > 0 {
        c.cfg.tc_full_ma
    } else {
        DEFAULT_TC_FULL_MA
    };
    if cap > c.effective_max {
        cap = c.effective_max;
    }
    if c.current_ma > cap {
        let step = if c.cfg.dec_step > 0 {
            c.cfg.dec_step
        } else {
            DEFAULT_DEC_STEP
        };
        c.current_ma -= step;
        if c.current_ma < cap {
            c.current_ma = cap;
        }
        let _ = write_current(c.fds, c.use_ufcs, c.current_ma);
    }
}
