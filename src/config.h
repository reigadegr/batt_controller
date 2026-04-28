#ifndef CONFIG_H
#define CONFIG_H

/*
 * 配置解析模块
 * 解析 /data/opbatt/batt_control (INI key=value 格式)
 * 基于 strace 实测还原的 28 个配置键
 */

#include <stdint.h>

/* 温控档位数 */
#define TEMP_RANGE_MAX    5

/* CV 降流阶梯最大档数 */
#define CV_STEP_MAX       8

/* 充电配置 */
typedef struct {
    /* 温控 */
    int      temp_range[TEMP_RANGE_MAX];     /* 温度阈值 (°C) */
    int      temp_range_count;
    int      temp_curr_offset[TEMP_RANGE_MAX]; /* 各温度段电流偏移 (mA) */
    int      temp_curr_offset_count;

    /* 电流调节步长 */
    int      adjust_step;                    /* 电流微调步长 (mA) */
    int      inc_step;                       /* 电流递增步长 (mA) */
    int      dec_step;                       /* 电流递减步长 (mA, CV 阶段) */

    /* UFCS 快充 */
    int      max_ufcs_chg_reset_cc;          /* UFCS 充电重置计数 */
    int      ufcs_reset_delay;               /* UFCS 重置延迟 (s) */
    int      ufcs_max;                       /* UFCS 最大电流 (mA) */
    int      pps_max;                        /* PPS 最大电流 (mA) */
    int      cable_override;                 /* 线缆覆盖标志 */

    /* UFCS/PPS SoC 监控区间 */
    int      ufcs_soc_mon[2];               /* UFCS SoC 监控范围 [low, high] */
    int      ufcs_interval_ms[2];           /* UFCS 轮询间隔 [fast, slow] (ms) */
    int      pps_soc_mon[2];                /* PPS SoC 监控范围 [low, high] */
    int      pps_interval_ms[2];            /* PPS 轮询间隔 [fast, slow] (ms) */

    /* 主循环 */
    int      loop_interval_ms;               /* 主循环间隔 (ms) */

    /* 电池电压控制 */
    int      batt_vol_thr[2];               /* 电池电压阈值 [low, high] (mV) */
    int      batt_vol_soc[2];               /* 电池电压 SoC [low, high] (%) */
    int      batt_con_soc;                   /* 电池连接 SoC 阈值 (%) */

    /* 上升阶段 */
    int      rise_quickstep_thr_mv;          /* 快速上升阈值 (mV) */
    int      rise_wait_thr_mv;               /* 上升等待阈值 (mV) */

    /* CV (恒压) 阶段 — vbat 阈值驱动阶梯降流 */
    int      cv_vol_mv;                      /* CV 阶段电压 (mV) */
    int      cv_max_ma;                      /* CV 阶段最大电流 (mA) */
    int      cv_step_mv[CV_STEP_MAX];        /* CV 降流 vbat 阈值 (mV) */
    int      cv_step_ma[CV_STEP_MAX];        /* CV 降流目标电流 (mA) */
    int      cv_step_count;                  /* CV 降流阶梯数 */

    /* TC (涓流充电) 阶段 */
    int      tc_vol_thr_mv;                  /* TC 阶段电压阈值 (mV) */
    int      tc_thr_soc;                     /* TC 阶段 SoC 阈值 (%) */
    int      tc_full_ma;                     /* TC 满电电流 (mA) */
    int      tc_vol_full_mv;                 /* TC 满电电压 (mV) */

    /* 充电完成 */
    int      curr_inc_wait_cycles;           /* 电流递增等待周期 */
    int      batt_full_thr_mv;               /* 电池满电阈值 (mV) */

    /* 重启 RISE 阶段 */
    int      restart_rise_step;              /* 重启 RISE 步长 (mA), 默认 50 */

    /* 去极化阶段 */
    int      depol_pulse_ma;                 /* 去极化脉冲电流 (mA), 默认 500 */
    int      depol_zero_ma;                  /* 去极化零电流 (mA), 默认 0 */

    /* 使能标志 */
    int      enabled;                        /* 总开关 */
} BattConfig;

/* 解析配置文件，成功返回 0，失败返回 -1 */
int config_parse(const char *path, BattConfig *cfg);

/* 打印配置到 stdout (复现原始输出格式) */
void config_dump(const BattConfig *cfg);

#endif /* CONFIG_H */
