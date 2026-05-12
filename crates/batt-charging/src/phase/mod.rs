use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use crate::loop_::LoopCtx;

/// 等待指定毫秒，期间检测运行标志；返回 false 表示应退出
pub(super) fn sleep_or_stop(c: &LoopCtx<'_>, ms: u64) -> bool {
    if !c.running.load(Ordering::Relaxed) {
        return false;
    }
    thread::sleep(Duration::from_millis(ms));
    true
}

mod cycle;
mod cv;
mod depol;
mod rise;
mod tc;

pub use cycle::handle_cycle_end;
pub use cv::exec_cv;
pub use depol::exec_depol;
pub use rise::exec_rise;
pub use tc::exec_tc;

use crate::charging::get_temp_curr_offset;

/// 计算 `effective_max`（温控 + `thermal_hi` + 所有 Voter 取最小值）
pub fn calc_effective_max(c: &mut LoopCtx<'_>) {
    let temp_offset = get_temp_curr_offset(c.cfg, c.parms.temp_01c);
    c.effective_max = c.max_ma;
    if temp_offset > 0 && c.effective_max > temp_offset {
        c.effective_max = temp_offset;
    }

    // 默认温控保护: temp_range 未配置时生效
    // temp_01c 单位 0.1°C, 300 = 30.0°C
    // >45°C 暂停, >40°C 降50%, <10°C 暂停, <15°C 降50%
    if c.cfg.temp_range_count == 0 {
        let t = c.parms.temp_01c;
        // 100 = 10.0°C, 150 = 15.0°C, 400 = 40.0°C, 450 = 45.0°C
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
