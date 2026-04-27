#ifndef MONITOR_H
#define MONITOR_H

/*
 * 监控线程模块
 *
 * Thread 1: USB 在线监控
 *   - 每 2s 轮询 /sys/class/power_supply/usb/online
 *   - 当 usb/online=1 时触发充电初始化
 *   - 管理 sysfs fd 的生命周期
 *
 * Thread 3: 电池日志监控
 *   - 每 5s 读取 battery_log_content
 *   - 解析逗号分隔的电池状态数据
 */

#include "config.h"
#include "sysfs.h"

/* 全局共享状态 */
typedef struct {
    volatile int   usb_online;       /* USB 在线状态 (0/1) */
    volatile int   charging_active;  /* 充电控制已激活 */
    volatile int   running;          /* 总运行标志 */
    SysfsFds       fds;              /* sysfs 文件描述符 */
    BattConfig     config;           /* 配置 */
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
