#include "monitor.h"

#include <stdio.h>
#include <unistd.h>

/*
 * Thread 1: USB 在线监控
 *
 * strace 实测行为:
 *   1. 打开所有 sysfs/proc fd (usb/online, battery_temp, chip_soc,
 *      ufcs_status, adapter_power, bcc_current, mmi_charging_enable,
 *      pps_force_val, pps_force_active, ufcs_force_val, ufcs_force_active)
 *   2. 重置 votable: 写 "0" 到 PPS/UFCS force
 *   3. 循环: 每 2s 读 usb/online
 */
void *monitor_usb_thread(void *arg)
{
    SharedState *st = (SharedState *)arg;

    /* 打开所有 sysfs 节点 */
    if (sysfs_open_all(&st->fds) < 0) {
        fprintf(stderr, "sysfs_open_all failed\n");
        return NULL;
    }

    /* 重置 votable */
    sysfs_reset_votables(&st->fds);

    /* 轮询 USB 在线状态 */
    while (st->running) {
        int online = sysfs_read_int(st->fds.usb_online);

        if (online > 0 && !st->usb_online) {
            /* USB 刚插入 → 触发充电初始化 */
            st->usb_online = 1;
            st->charging_active = 1;
        } else if (online <= 0 && st->usb_online) {
            /* USB 拔出 → 停止充电 */
            st->usb_online = 0;
            st->charging_active = 0;
        }

        sleep(2);
    }

    sysfs_close_all(&st->fds);
    return NULL;
}

/*
 * Thread 3: 电池日志监控
 *
 * strace 实测行为:
 *   每 5s 读取 /sys/class/oplus_chg/battery/battery_log_content
 *   格式: 逗号分隔的电池状态数据
 *   示例: ,407,409,4387,4366,-5215,58,58,1,15,0,0,8790,58,1,4435,1100,2000,...
 */
void *monitor_battery_log_thread(void *arg)
{
    SharedState *st = (SharedState *)arg;
    char buf[4096];

    while (st->running) {
        if (st->usb_online) {
            int n = sysfs_read_battery_log(buf, sizeof(buf));
            if (n > 0) {
                /* 解析电池状态 — 字段含义基于 strace 推断:
                 *  [0]: 空 (前导逗号)
                 *  [1]: battery_temp (0.1°C)
                 *  [2]: chip_soc (%)
                 *  [3]: vbat (mV)
                 *  [4]: ibat (mA)
                 *  ...
                 */
            }
        }

        sleep(5);
    }

    return NULL;
}
