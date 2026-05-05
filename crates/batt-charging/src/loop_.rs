use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use batt_common::{get_timestamp, log_write};
use batt_config::BattConfig;
use batt_sysfs::{self, SysfsFds};

use crate::ChargePhase;
use crate::charging::{
    clamp_max_ma, dumpsys_set_ac, dumpsys_set_status, next_phase, parse_bcc_parms, phase_name,
    read_voters_3x, write_current,
};
use crate::phase::{calc_effective_max, exec_cv, exec_depol, exec_rise, exec_tc, handle_cycle_end};

/// 充电主循环上下文: 避免在函数间传递过多参数
pub struct LoopCtx<'a> {
    // 配置和硬件
    pub fds: &'a mut SysfsFds,
    pub cfg: &'a BattConfig,
    pub running: &'a AtomicBool,

    // 运行时状态
    pub current_ma: i32,
    pub max_ma: i32,
    pub cable_max: i32,
    pub use_ufcs: i32,
    pub inc_step: i32,
    pub ramp_idx: i32,
    pub restart_count: i32,
    pub in_charge_cycle: i32,
    pub soc: i32,
    pub phase: ChargePhase,
    pub cv_step_idx: i32,
    pub cv_holding: i32,
    pub effective_max: i32,
    /// RISE 阶段到达 `phase_max` 后置 1, 静默维持
    pub rise_max_reached: i32,

    // 临时数据
    pub parms: crate::BccParms,
    pub voters: crate::UfcsVoters,
}

/// 充电控制主循环（在子线程中运行）
/// - `fds`: 已打开的 sysfs fd 集合
/// - `cfg`: 配置
/// - `running`: 运行标志（设为 false 时退出循环）
pub fn run(fds: &mut SysfsFds, cfg: &BattConfig, running: &AtomicBool) {
    // 缓存 chip_soc fd，避免后续通过 fds 引用
    let chip_soc_fd = fds.chip_soc;

    let mut c = LoopCtx {
        fds,
        cfg,
        running,
        current_ma: 500,
        max_ma: cfg.ufcs_max,
        cable_max: 0,
        use_ufcs: 1,
        inc_step: 0,
        ramp_idx: 0,
        restart_count: 0,
        in_charge_cycle: 0,
        soc: 0,
        phase: ChargePhase::Idle,
        cv_step_idx: 0,
        cv_holding: 0,
        effective_max: 0,
        rise_max_reached: 0,
        parms: crate::BccParms::default(),
        voters: crate::UfcsVoters::default(),
    };

    // ---- 阶段 1: 读取电池状态日志 ----
    let _ = batt_sysfs::read_battery_log();

    // ---- 阶段 2: 重置 votable ----
    batt_sysfs::reset_votables();

    // ---- 阶段 3: 设置电池状态为充电中 ----
    dumpsys_set_ac();
    dumpsys_set_status();

    read_voters_3x(&mut c.voters);

    c.cable_max = c.voters.cable_max_ma;
    c.max_ma = clamp_max_ma(c.max_ma, 0, c.cable_max);

    // strace 确认: inc_step = effective_max / 10
    c.inc_step = if c.max_ma > 0 {
        c.max_ma / 10
    } else {
        cfg.inc_step
    };

    // ---- 阶段 4: 输出充电信息日志 ----
    let ts = get_timestamp();
    log_write(&format!(
        "{ts} UFCS_CHG: AdpMAXma={}mA, CableMAXma={}mA, Maxallow={}mA, Maxset={}mA, OP_chg=1\n",
        c.voters.max_ma, c.voters.cable_max_ma, c.voters.max_ma, c.max_ma
    ));
    log_write(&format!(
        "{ts} ==== Charger type UFCS, set max current {}mA ====\n",
        c.max_ma
    ));

    // ---- 阶段 5: 充电控制主循环 ----
    while running.load(Ordering::Relaxed) {
        // 读取 bcc_parms
        if let Some(s) = batt_sysfs::read_bcc_parms() {
            let _ = parse_bcc_parms(&s, &mut c.parms);
        } else {
            thread::sleep(Duration::from_millis(500));
            continue;
        }

        // 读取 SoC
        c.soc = batt_sysfs::read_int(chip_soc_fd).unwrap_or(0);

        // 充电周期结束检测
        if handle_cycle_end(&mut c) {
            continue;
        }

        // 温控: 根据温度/thermal_hi/STEP_VOTER 调整最大电流
        calc_effective_max(&mut c);

        // ---- 充电阶段状态机 ----
        let new_phase = next_phase(c.phase, cfg, &c.parms, c.soc, c.current_ma);
        if new_phase != c.phase {
            let ts = get_timestamp();
            log_write(&format!(
                "{ts} ==== Phase {} -> {} (vbat={}mV, soc={}%) ====\n",
                phase_name(c.phase),
                phase_name(new_phase),
                c.parms.vbat_mv,
                c.soc
            ));
            c.phase = new_phase;
        }

        match c.phase {
            ChargePhase::Idle => {}
            ChargePhase::Rise | ChargePhase::RestartRise => {
                // strace 确认: 到达 phase_max 后静默维持，不写 force_val
                if c.rise_max_reached == 0 {
                    exec_rise(&mut c);
                }
            }
            ChargePhase::Cv => exec_cv(&mut c),
            ChargePhase::Tc => exec_tc(&mut c),
            ChargePhase::Depol => exec_depol(&mut c),
            ChargePhase::Full => {
                // strace 确认: FULL 阶段持续写 1000mA (非 500)
                write_current(c.fds, c.use_ufcs, 1000);
            }
        }

        // 满电判断: batt_full_thr_mv 额外检查
        if c.phase != ChargePhase::Full
            && cfg.batt_full_thr_mv > 0
            && c.parms.vbat_mv >= cfg.batt_full_thr_mv
        {
            let ts = get_timestamp();
            log_write(&format!(
                "{ts} ==== Battery full: vbat={}mV >= batt_full_thr_mv={}mV ====\n",
                c.parms.vbat_mv, cfg.batt_full_thr_mv
            ));
            c.phase = ChargePhase::Full;
        }

        thread::sleep(Duration::from_millis(500));
    }
}
