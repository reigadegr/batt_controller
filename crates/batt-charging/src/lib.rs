// UFCS 充电控制逻辑
// 基于 strace 实测的 Thread 2 行为还原
//
// 充电流程:
//   1. 读 battery_log_content → 解析电池状态
//   2. 读 UFCS_CURR/status (3次) → 解析 voter 信息
//   3. 确定充电器类型和最大电流
//   4. 递增电流: 500 → max (步长 inc_step)
//   5. 持续监控 bcc_parms + battery_temp + chip_soc

mod charging;
mod loop_;
mod phase;

pub use charging::dumpsys_reset;
pub use charging::dumpsys_set_ac;
pub use charging::dumpsys_set_status;
pub use loop_::run;

/// 充电阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ChargePhase {
    /// 未充电
    Idle,
    /// 上升阶段: 首次充电 quickstart 三段式
    Rise,
    /// 重启上升: dumpsys reset 后 +50mA 线性爬升
    RestartRise,
    /// 恒压阶段: 电压达标, 限流
    Cv,
    /// 涓流阶段: 高 `SoC` 低电流
    Tc,
    /// 去极化阶段: `force_val` 写 0 和极低值
    Depol,
    /// 满电
    Full,
}

/// `bcc_parms` 解析结果 (19 个逗号分隔值)
/// strace 886 次读取 + `battery_log_content` 交叉验证确认的真实字段映射
#[derive(Debug, Clone, Default)]
pub struct BccParms {
    /// fields[0]:  满电容量 mAh (恒定 ~5896)
    pub fcc: i32,
    /// fields[1]:  设计容量 (恒定 ~5888)
    pub design_cap: i32,
    /// fields[2]:  充电IC参数A (线性递减, 1489→492)
    pub ic_param_a: i32,
    /// fields[3]:  常量 ~2637
    pub param_c: i32,
    /// fields[4]:  常量 ~2621
    pub param_d: i32,
    /// fields[5]:  充电IC参数B (= `ic_param_a` + 405)
    pub ic_param_b: i32,
    /// fields[6]:  电池电压 (mV), 交叉验证确认
    pub vbat_mv: i32,
    /// fields[7]:  电池温度 (0.1°C), 303=30.3°C
    pub temp_01c: i32,
    /// fields[8]:  电池电流 (mA, 负值=充电)
    pub ibat_ma: i32,
    /// fields[9]:  温控阈值上界 (91→85→80, 三档阶梯)
    pub thermal_hi: i32,
    /// fields[10]: 温控阈值下界 (= `thermal_hi` - 11)
    pub thermal_lo: i32,
    /// fields[11]: 总线电压 (mV), 精确匹配 `battery_log`
    pub vbus_mv: i32,
    /// fields[12]: 保留, 通常 0
    pub field_12: i32,
    /// fields[13]: 保留, 通常 0
    pub field_13: i32,
    /// fields[14]: UFCS 最大电流
    pub ufcs_max_ma: i32,
    /// fields[15]: UFCS 使能
    pub ufcs_en: i32,
    /// fields[16]: PPS 最大电流
    pub pps_max_ma: i32,
    /// fields[17]: PPS 使能
    pub pps_en: i32,
    /// fields[18]: 线缆类型
    pub cable_type: i32,
}

/// UFCS voter 信息 (22个voter)
#[derive(Debug, Clone, Default)]
pub struct UfcsVoters {
    /// `MAX_VOTER`: 硬件最大电流上限
    pub max_ma: i32,
    /// `CABLE_MAX_VOTER`: 线缆最大电流
    pub cable_max_ma: i32,
    /// `STEP_VOTER`: 阶梯式电流限制
    pub step_ma: i32,
    /// `BCC_VOTER`: BCC协议电流限制
    pub bcc_ma: i32,
    /// `ADAPTER_IMAX_VOTER`: 适配器最大输出电流
    pub adapter_imax_ma: i32,
    /// `IC_VOTER`: 充电IC硬件限制
    pub ic_ma: i32,
    /// `BASE_MAX_VOTER`: 基础最大电流
    pub base_max_ma: i32,
    /// `BATT_TEMP_VOTER`: 电池温度保护电流
    pub batt_temp_ma: i32,
    /// `COOL_DOWN_VOTER`: 降温/降功率控制
    pub cool_down_ma: i32,
    /// `IMP_VOTER`: 阻抗/脉冲电流限制
    pub imp_ma: i32,
    /// `LIMIT_FCL_VOTER`: FCL(满充限制)限流
    pub limit_fcl_ma: i32,
    /// `BATT_SOC_VOTER`: 电池SOC限制电流
    pub batt_soc_ma: i32,
    /// `SALE_MODE_VOTER`: 展台模式电流限制
    pub sale_mode_ma: i32,
    /// `HIDL_VOTER`: HIDL接口设置的电流
    pub hidl_ma: i32,
    /// `BAD_SUBBOARD_VOTER`: 子板异常保护
    pub bad_subboard_ma: i32,
    /// `EIS_VOTER`: 电化学阻抗谱保护
    pub eis_ma: i32,
    /// `BATT_BAL_VOTER`: 电池均衡电流限制
    pub batt_bal_ma: i32,
    /// `IBUS_OVER_VOTER`: 输入总线过流保护
    pub ibus_over_ma: i32,
    /// `SLOW_CHG_VOTER`: 慢充模式电流限制
    pub slow_chg_ma: i32,
    /// `PLC_VOTER`: PLC通信电流限制
    pub plc_ma: i32,
    /// `PR_VOTER`: PR(优先级)控制
    pub pr_ma: i32,
    /// `BAD_SUB_BTB_VOTER`: 子板BTB连接异常
    pub bad_sub_btb_ma: i32,
}
