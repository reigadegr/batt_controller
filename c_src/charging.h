#ifndef CHARGING_H
#define CHARGING_H

/*
 * UFCS 充电控制逻辑
 * 基于 strace 实测的 Thread 2 行为还原
 *
 * 充电流程:
 *   1. 读 battery_log_content → 解析电池状态
 *   2. 读 UFCS_CURR/status (3次) → 解析 voter 信息
 *   3. 确定充电器类型和最大电流
 *   4. 递增电流: 500 → max (步长 inc_step)
 *   5. 持续监控 bcc_parms + battery_temp + chip_soc
 */

#include "config.h"
#include "sysfs.h"

/* 充电阶段 */
typedef enum {
    PHASE_IDLE,          /* 未充电 */
    PHASE_RISE,          /* 上升阶段: 首次充电 quickstart 三段式 */
    PHASE_RESTART_RISE,  /* 重启上升: dumpsys reset 后 +50mA 线性爬升 */
    PHASE_CV,            /* 恒压阶段: 电压达标, 限流 */
    PHASE_TC,            /* 涓流阶段: 高 SoC 低电流 */
    PHASE_DEPOL,         /* 去极化阶段: force_val 写 0 和极低值 */
    PHASE_FULL,          /* 满电 */
} ChargePhase;

/* bcc_parms 解析结果 (19 个逗号分隔值) */
/* strace 886 次读取 + battery_log_content 交叉验证确认的真实字段映射 */
typedef struct {
    int fcc;               /* fields[0]:  满电容量 mAh (恒定 ~5896) */
    int design_cap;        /* fields[1]:  设计容量 (恒定 ~5888) */
    int ic_param_a;        /* fields[2]:  充电IC参数A (线性递减, 1489→492) */
    int param_c;           /* fields[3]:  常量 ~2637 */
    int param_d;           /* fields[4]:  常量 ~2621 */
    int ic_param_b;        /* fields[5]:  充电IC参数B (= ic_param_a + 405) */
    int vbat_mv;           /* fields[6]:  电池电压 (mV), 交叉验证确认 */
    int temp_01c;          /* fields[7]:  电池温度 (0.1°C), 303=30.3°C */
    int ibat_ma;           /* fields[8]:  电池电流 (mA, 负值=充电) */
    int thermal_hi;        /* fields[9]:  温控阈值上界 (91→85→80, 三档阶梯) */
    int thermal_lo;        /* fields[10]: 温控阈值下界 (= thermal_hi - 11) */
    int vbus_mv;           /* fields[11]: 总线电压 (mV), 精确匹配 battery_log */
    int field_12;          /* fields[12]: 保留, 通常 0 */
    int field_13;          /* fields[13]: 保留, 通常 0 */
    int ufcs_max_ma;       /* fields[14]: UFCS 最大电流 */
    int ufcs_en;           /* fields[15]: UFCS 使能 */
    int pps_max_ma;        /* fields[16]: PPS 最大电流 */
    int pps_en;            /* fields[17]: PPS 使能 */
    int cable_type;        /* fields[18]: 线缆类型 */
} BccParms;

/* UFCS voter 信息 (22个voter) */
typedef struct {
    int max_ma;            /* MAX_VOTER: 硬件最大电流上限 */
    int cable_max_ma;      /* CABLE_MAX_VOTER: 线缆最大电流 */
    int step_ma;           /* STEP_VOTER: 阶梯式电流限制 */
    int bcc_ma;            /* BCC_VOTER: BCC协议电流限制 */
    int adapter_imax_ma;   /* ADAPTER_IMAX_VOTER: 适配器最大输出电流 */
    int ic_ma;             /* IC_VOTER: 充电IC硬件限制 */
    int base_max_ma;       /* BASE_MAX_VOTER: 基础最大电流 */
    int batt_temp_ma;      /* BATT_TEMP_VOTER: 电池温度保护电流 */
    int cool_down_ma;      /* COOL_DOWN_VOTER: 降温/降功率控制 */
    int imp_ma;            /* IMP_VOTER: 阻抗/脉冲电流限制 */
    int limit_fcl_ma;      /* LIMIT_FCL_VOTER: FCL(满充限制)限流 */
    int batt_soc_ma;       /* BATT_SOC_VOTER: 电池SOC限制电流 */
    int sale_mode_ma;      /* SALE_MODE_VOTER: 展台模式电流限制 */
    int hidl_ma;           /* HIDL_VOTER: HIDL接口设置的电流 */
    int bad_subboard_ma;   /* BAD_SUBBOARD_VOTER: 子板异常保护 */
    int eis_ma;            /* EIS_VOTER: 电化学阻抗谱保护 */
    int batt_bal_ma;       /* BATT_BAL_VOTER: 电池均衡电流限制 */
    int ibus_over_ma;      /* IBUS_OVER_VOTER: 输入总线过流保护 */
    int slow_chg_ma;       /* SLOW_CHG_VOTER: 慢充模式电流限制 */
    int plc_ma;            /* PLC_VOTER: PLC通信电流限制 */
    int pr_ma;             /* PR_VOTER: PR(优先级)控制 */
    int bad_sub_btb_ma;    /* BAD_SUB_BTB_VOTER: 子板BTB连接异常 */
} UfcsVoters;

/* 解析 bcc_parms 字符串 */
int charging_parse_bcc_parms(const char *str, BccParms *parms);

/* 解析 UFCS voter 信息 */
int charging_parse_ufcs_voters(const char *status, UfcsVoters *voters);

/*
 * 充电重置序列 (strace 确认, 2026-04-28 完整周期验证)
 * 仅执行: dumpsys battery reset
 * 不含 mmi_charging_enable 写入
 */
void charging_dumpsys_reset(SysfsFds *fds);

/*
 * 充电控制主循环 (在子线程中运行)
 * fds: 已打开的 sysfs fd 集合
 * cfg: 配置
 * running: 运行标志指针 (设为 0 时退出循环)
 */
void charging_loop(SysfsFds *fds, const BattConfig *cfg, volatile int *running);

/* 获取默认 CV 降流阶梯配置 (基于锂电池物理特性)
 * effective_max: 当前有效最大电流 (mA), 用于计算 50% 比例档
 * out_mv/out_ma: 输出数组, 容量至少 CV_STEP_MAX
 * 返回阶梯数 */
int get_default_cv_steps(int effective_max, int *out_mv, int *out_ma);

#endif /* CHARGING_H */
