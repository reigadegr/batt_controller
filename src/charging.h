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

/* UFCS voter 信息 */
typedef struct {
    int max_ma;            /* MAX_VOTER 值 */
    int cable_max_ma;      /* CABLE_MAX_VOTER 值 */
    int step_ma;           /* STEP_VOTER 值 */
    int bcc_ma;            /* BCC_VOTER 值 */
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

#endif /* CHARGING_H */
