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
    PHASE_IDLE,     /* 未充电 */
    PHASE_RISE,     /* 上升阶段: 电流递增 */
    PHASE_CV,       /* 恒压阶段: 电压达标, 限流 */
    PHASE_TC,       /* 涓流阶段: 高 SoC 低电流 */
    PHASE_FULL,     /* 满电 */
} ChargePhase;

/* bcc_parms 解析结果 (19 个逗号分隔值) */
typedef struct {
    int vbat_mv;           /* 电池电压 (mV) */
    int ibat_ma;           /* 电池电流 (mA, 负值=充电) */
    int temp_01c;          /* 温度 (0.1°C) */
    int fcc;               /* 满电容量 */
    int rm;                /* 剩余容量 */
    int soh;               /* 健康度 */
    int vbus_mv;           /* 总线电压 (mV) */
    int ibus_ma;           /* 总线电流 (mA) */
    int power_mw;          /* 功率 (mW) */
    int cycles;            /* 循环次数 */
    int charge_status;     /* 充电状态 */
    int batt_vol;          /* 电池电压2 */
    int field_12;          /* 未知字段 */
    int field_13;          /* 未知字段 */
    int ufcs_max_ma;       /* UFCS 最大电流 */
    int ufcs_en;           /* UFCS 使能 */
    int pps_max_ma;        /* PPS 最大电流 */
    int pps_en;            /* PPS 使能 */
    int cable_type;        /* 线缆类型 */
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
 * 充电重置序列 (strace 确认)
 * fork+execvp 执行: battery set ac 1, battery set status 2, battery reset
 * 然后: mmi_charging_enable 0 → sleep 1s → mmi_charging_enable 1 → sleep 8s
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
