#ifndef MONITOR_H
#define MONITOR_H

/*
 * 监控线程模块
 *
 * Thread 1: USB 在线监控
 *   - 每 2s 轮询 /sys/class/power_supply/usb/online
 *   - 当 usb/online=1 时设置 charging_active 标志
 *   - fd 生命周期由充电线程自行管理
 *
 * Thread 3: 电池日志监控
 *   - 每 5s 读取 battery_log_content
 *   - 解析逗号分隔的电池状态数据
 */

#include "config.h"

/* battery_log_content 解析结果 */
typedef struct {
    int temp_raw;       /* [1] battery/temp 原始值 (0.1°C) */
    int temp_01c;       /* [2] 温度 0.1°C (bcc_parms[7]) */
    int vbat_mv;        /* [3] 电池电压 mV */
    int vbus_mv;        /* [4] 总线电压 mV */
    int ibat_ma;        /* [5] 电池电流 mA (负值=充电) */
    int chip_soc;       /* [6] 芯片 SoC % */
    int ui_soc;         /* [7] UI SoC % */
    int chg_sts;        /* [8] 充电状态 */
    int fcc_mah;        /* [12] 累积充电量 mAh */
} BatteryLog;

/* 全局共享状态 */
typedef struct {
    volatile int   usb_online;       /* USB 在线状态 (0/1) */
    volatile int   charging_active;  /* 充电控制已激活 */
    volatile int   running;          /* 总运行标志 */
    BattConfig     config;           /* 配置 */
    BatteryLog     blog;             /* battery_log_content 解析结果 */
} SharedState;

/*
 * USB 在线监控线程入口
 * 每 2s 检查 usb/online，当检测到充电时打开 sysfs fd
 */
void *monitor_usb_thread(void *arg);

/*
 * 电池日志监控线程入口
 * 每 5s 读取 battery_log_content
 */
void *monitor_battery_log_thread(void *arg);

#endif /* MONITOR_H */
