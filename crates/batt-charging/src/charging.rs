use std::process::Command;

use batt_config::BattConfig;
use batt_sysfs::{
    PROC_PPS_FORCE_ACTIVE, PROC_PPS_FORCE_VAL, PROC_UFCS_FORCE_ACTIVE, PROC_UFCS_FORCE_VAL,
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
///
/// 按行匹配，避免子串误匹配（如 `MAX_VOTER:` 匹配到 `CABLE_MAX_VOTER:`）。
fn extract_voter_int(status: &str, tag: &str) -> i32 {
    for line in status.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix(tag) else {
            continue;
        };
        let Some(vpos) = rest.find("v=") else {
            continue;
        };
        let val_str = &rest[vpos + 2..];
        let end = val_str
            .bytes()
            .position(|b| !(b.is_ascii_digit() || b == b'-'))
            .unwrap_or(val_str.len());
        if end > 0 {
            return val_str[..end].parse().unwrap_or(0);
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

/// 读取 voter 信息（原版读 3 次，此处优化为仅读 1 次）
pub fn read_voters_3x(voters: &mut UfcsVoters) {
    if let Some(s) = batt_sysfs::read_ufcs_voters() {
        parse_ufcs_voters(&s, voters);
    }
}

/// 执行 dumpsys battery reset
/// strace 确认: 充电重置序列仅含 dumpsys battery reset
pub fn dumpsys_reset() {
    let _ = Command::new("dumpsys").arg("battery").arg("reset").status();
}

/// 执行 dumpsys battery set ac 1
/// 设置 AC 电源充电模式
pub fn dumpsys_set_ac() {
    let _ = Command::new("dumpsys")
        .arg("battery")
        .arg("set")
        .arg("ac")
        .arg("1")
        .status();
}

/// 执行 dumpsys battery set status 2
/// 设置电池状态为充电中
pub fn dumpsys_set_status() {
    let _ = Command::new("dumpsys")
        .arg("battery")
        .arg("set")
        .arg("status")
        .arg("2")
        .status();
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
///
/// # Errors
///
/// 当 sysfs 写入失败时返回错误。
pub fn write_current(use_ufcs: i32, ma: i32) -> Result<(), i32> {
    if use_ufcs != 0 {
        batt_sysfs::write_proc_int(PROC_UFCS_FORCE_VAL, ma)?;
        batt_sysfs::write_proc_str(PROC_UFCS_FORCE_ACTIVE, "1")?;
    } else {
        batt_sysfs::write_proc_int(PROC_PPS_FORCE_VAL, ma)?;
        batt_sysfs::write_proc_str(PROC_PPS_FORCE_ACTIVE, "1")?;
    }
    Ok(())
}

/// 取三者最小正值（忽略 0 和负值）
#[must_use]
pub const fn clamp_max_ma(cfg_max: i32, proto_max: i32, cable_max: i32) -> i32 {
    let mut m = i32::MAX;
    if cfg_max > 0 && cfg_max < m {
        m = cfg_max;
    }
    if proto_max > 0 && proto_max < m {
        m = proto_max;
    }
    if cable_max > 0 && cable_max < m {
        m = cable_max;
    }
    if m == i32::MAX { 0 } else { m }
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

        ChargePhase::Depol | ChargePhase::Full => ChargePhase::Full,
    }
}
