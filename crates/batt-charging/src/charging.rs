use std::process::Command;

use batt_config::BattConfig;
use batt_sysfs::{
    self, PROC_PPS_FORCE_ACTIVE, PROC_PPS_FORCE_VAL, PROC_UFCS_FORCE_ACTIVE, PROC_UFCS_FORCE_VAL,
    SysfsFds,
};

use crate::{BccParms, ChargePhase, UfcsVoters};

/// 解析 `bcc_parms` 字符串（19 个逗号分隔值）
/// 返回 Ok(()) 表示成功；至少需要 12 个字段
pub fn parse_bcc_parms(str_data: &str, parms: &mut BccParms) -> Result<(), ()> {
    *parms = BccParms::default();

    let mut fields = [0i32; 20];
    let mut count = 0usize;

    for token in str_data.split(',') {
        if count >= 20 {
            break;
        }
        let trimmed = token.trim();
        match trimmed.parse::<i32>() {
            Ok(v) => {
                fields[count] = v;
                count += 1;
            }
            Err(_) => break,
        }
    }

    if count < 12 {
        return Err(());
    }

    // strace 886 次读取 + battery_log_content 交叉验证确认的字段映射
    parms.fcc = fields[0]; // 满电容量 mAh (恒定 ~5896)
    parms.design_cap = fields[1]; // 设计容量 (恒定 ~5888)
    parms.ic_param_a = fields[2]; // 充电IC参数A (线性递减)
    parms.param_c = fields[3]; // 常量 ~2637
    parms.param_d = fields[4]; // 常量 ~2621
    parms.ic_param_b = fields[5]; // 充电IC参数B (= ic_param_a + 405)
    parms.vbat_mv = fields[6]; // 电池电压 mV (交叉验证确认)
    parms.temp_01c = fields[7]; // 温度 0.1°C (303=30.3°C)
    parms.ibat_ma = fields[8]; // 电池电流 mA (负值=充电)
    parms.thermal_hi = fields[9]; // 温控阈值上界 (91→85→80)
    parms.thermal_lo = fields[10]; // 温控阈值下界 (= thermal_hi - 11)
    parms.vbus_mv = fields[11]; // 总线电压 mV (精确匹配 battery_log)

    if count >= 19 {
        parms.field_12 = fields[12];
        parms.field_13 = fields[13];
        parms.ufcs_max_ma = fields[14];
        parms.ufcs_en = fields[15];
        parms.pps_max_ma = fields[16];
        parms.pps_en = fields[17];
        parms.cable_type = fields[18];
    }

    Ok(())
}

/// 从 voter status 字符串中提取指定 tag 的 `v=` 值
fn extract_voter_int(status: &str, tag: &str) -> i32 {
    if let Some(pos) = status.find(tag) {
        let rest = &status[pos..];
        if let Some(vpos) = rest.find("v=") {
            let val_str = &rest[vpos + 2..];
            // 解析到第一个非数字字符（跳过负号开头的数字）
            let end = val_str
                .char_indices()
                .skip_while(|&(_, c)| c == '-')
                .find(|&(_, c)| !c.is_ascii_digit())
                .map_or(val_str.len(), |(i, _)| i);
            if end > 0 {
                return val_str[..end].parse().unwrap_or(0);
            }
        }
    }
    0
}

/// 解析 UFCS voter 信息（22 个 voter）
pub fn parse_ufcs_voters(status: &str, voters: &mut UfcsVoters) {
    *voters = UfcsVoters {
        max_ma: extract_voter_int(status, "MAX_VOTER:"),
        cable_max_ma: extract_voter_int(status, "CABLE_MAX_VOTER:"),
        step_ma: extract_voter_int(status, "STEP_VOTER:"),
        bcc_ma: extract_voter_int(status, "BCC_VOTER:"),
        adapter_imax_ma: extract_voter_int(status, "ADAPTER_IMAX_VOTER:"),
        ic_ma: extract_voter_int(status, "IC_VOTER:"),
        base_max_ma: extract_voter_int(status, "BASE_MAX_VOTER:"),
        batt_temp_ma: extract_voter_int(status, "BATT_TEMP_VOTER:"),
        cool_down_ma: extract_voter_int(status, "COOL_DOWN_VOTER:"),
        imp_ma: extract_voter_int(status, "IMP_VOTER:"),
        limit_fcl_ma: extract_voter_int(status, "LIMIT_FCL_VOTER:"),
        batt_soc_ma: extract_voter_int(status, "BATT_SOC_VOTER:"),
        sale_mode_ma: extract_voter_int(status, "SALE_MODE_VOTER:"),
        hidl_ma: extract_voter_int(status, "HIDL_VOTER:"),
        bad_subboard_ma: extract_voter_int(status, "BAD_SUBBOARD_VOTER:"),
        eis_ma: extract_voter_int(status, "EIS_VOTER:"),
        batt_bal_ma: extract_voter_int(status, "BATT_BAL_VOTER:"),
        ibus_over_ma: extract_voter_int(status, "IBUS_OVER_VOTER:"),
        slow_chg_ma: extract_voter_int(status, "SLOW_CHG_VOTER:"),
        plc_ma: extract_voter_int(status, "PLC_VOTER:"),
        pr_ma: extract_voter_int(status, "PR_VOTER:"),
        bad_sub_btb_ma: extract_voter_int(status, "BAD_SUB_BTB_VOTER:"),
    };
}

/// 读取 voter 信息 3 次（strace 确认行为）
pub fn read_voters_3x(voters: &mut UfcsVoters) {
    for _ in 0..3 {
        if let Some(s) = batt_sysfs::read_ufcs_voters() {
            parse_ufcs_voters(&s, voters);
        }
    }
}

/// 执行 dumpsys battery reset
/// strace 确认: 充电重置序列仅含 dumpsys battery reset
pub fn dumpsys_reset() {
    let _ = Command::new("dumpsys").arg("battery").arg("reset").status();
}

/// 选择充电协议: 1=UFCS, 0=PPS
#[must_use]
pub const fn choose_protocol(cfg: &BattConfig, parms: &BccParms) -> i32 {
    if cfg.cable_override != 0 {
        return if cfg.cable_override > 0 { 1 } else { 0 };
    }
    if parms.ufcs_en != 0 && parms.ufcs_max_ma > 0 {
        return 1;
    }
    if parms.pps_en != 0 && parms.pps_max_ma > 0 {
        return 0;
    }
    1
}

/// 根据温度获取电流偏移量
#[must_use]
pub fn get_temp_curr_offset(cfg: &BattConfig, temp_01c: i32) -> i32 {
    let temp = temp_01c / 10;
    for i in (0..cfg.temp_range_count).rev() {
        if temp >= cfg.temp_range[i] {
            return if i < cfg.temp_curr_offset_count {
                cfg.temp_curr_offset[i]
            } else {
                0
            };
        }
    }
    0
}

/// 写入电流值到 votable
pub fn write_current(fds: &SysfsFds, use_ufcs: i32, ma: i32) {
    let _ = fds; // fds 参数保留以匹配调用签名，当前 sysfs 写入不依赖 fds
    if use_ufcs != 0 {
        let _ = batt_sysfs::write_proc_int(PROC_UFCS_FORCE_VAL, ma);
        let _ = batt_sysfs::write_proc_str(PROC_UFCS_FORCE_ACTIVE, "1");
    } else {
        let _ = batt_sysfs::write_proc_int(PROC_PPS_FORCE_VAL, ma);
        let _ = batt_sysfs::write_proc_str(PROC_PPS_FORCE_ACTIVE, "1");
    }
}

/// 取三者最小正值（忽略 0 和负值）
#[must_use]
pub const fn clamp_max_ma(cfg_max: i32, proto_max: i32, cable_max: i32) -> i32 {
    let mut m = cfg_max;
    if proto_max > 0 && proto_max < m {
        m = proto_max;
    }
    if cable_max > 0 && cable_max < m {
        m = cable_max;
    }
    m
}

/// 获取阶段名称
#[must_use]
pub const fn phase_name(ph: ChargePhase) -> &'static str {
    match ph {
        ChargePhase::Idle => "IDLE",
        ChargePhase::Rise => "RISE",
        ChargePhase::RestartRise => "RESTART_RISE",
        ChargePhase::Cv => "CV",
        ChargePhase::Tc => "TC",
        ChargePhase::Depol => "DEPOL",
        ChargePhase::Full => "FULL",
    }
}

/// 阶段状态转移判定
#[must_use]
pub fn next_phase(
    cur: ChargePhase,
    cfg: &BattConfig,
    parms: &BccParms,
    soc: i32,
    current_ma: i32,
) -> ChargePhase {
    let vbat = parms.vbat_mv;
    let ibat = parms.ibat_ma.abs();

    if parms.thermal_hi == 0 && cur != ChargePhase::Depol && cur != ChargePhase::RestartRise {
        return ChargePhase::Idle;
    }

    match cur {
        ChargePhase::Idle => ChargePhase::Rise,

        ChargePhase::Rise => {
            if cfg.cv_vol_mv > 0 && vbat >= cfg.cv_vol_mv {
                ChargePhase::Cv
            } else {
                ChargePhase::Rise
            }
        }

        ChargePhase::RestartRise => {
            if cfg.cv_vol_mv > 0 && vbat >= cfg.cv_vol_mv {
                ChargePhase::Cv
            } else {
                ChargePhase::RestartRise
            }
        }

        ChargePhase::Cv => {
            if cfg.tc_thr_soc > 0 && soc >= cfg.tc_thr_soc {
                return ChargePhase::Tc;
            }
            if cfg.tc_vol_thr_mv > 0 && vbat >= cfg.tc_vol_thr_mv {
                return ChargePhase::Tc;
            }
            ChargePhase::Cv
        }

        ChargePhase::Tc => {
            if current_ma <= 100 {
                return ChargePhase::Depol;
            }
            if cfg.tc_full_ma > 0
                && cfg.tc_vol_full_mv > 0
                && ibat <= cfg.tc_full_ma
                && vbat >= cfg.tc_vol_full_mv
            {
                return ChargePhase::Full;
            }
            ChargePhase::Tc
        }

        ChargePhase::Depol => ChargePhase::RestartRise,

        ChargePhase::Full => ChargePhase::Full,
    }
}

/// CV 阶梯表条目: (电压阈值 mV, 目标电流 mA)
pub struct CvStep {
    pub mv: i32,
    pub ma: i32,
}

/// 获取默认 CV 降流阶梯配置（基于锂电池物理特性）
/// `effective_max`: 当前有效最大电流 (mA), 用于计算 50% 比例档
#[must_use]
pub fn get_default_cv_steps(effective_max: i32) -> Vec<CvStep> {
    let mut half = (effective_max + 1) / 2;
    half = ((half + 25) / 50) * 50; // 对齐 50mA

    vec![
        CvStep { mv: 4450, ma: half },
        CvStep { mv: 4480, ma: 1000 },
        CvStep { mv: 4500, ma: 500 },
        CvStep { mv: 4520, ma: 200 },
    ]
}
