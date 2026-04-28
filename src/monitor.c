#include "monitor.h"

#include "sysfs.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/*
 * Thread 1: USB 在线监控
 *
 * 只负责检测 usb/online 状态并设置标志。
 * sysfs fd 生命周期由充电线程管理，避免竞态。
 */
void *monitor_usb_thread(void *arg)
{
    SharedState *st = (SharedState *)arg;
    int prev_online = 0;

    while (st->running) {
        /* 临时打开 usb/online 读取状态 */
        int fd = sysfs_open_ro("/sys/class/power_supply/usb/online");
        int online = 0;
        if (fd >= 0) {
            online = sysfs_read_int(fd);
            close(fd);
        }

        if (online > 0 && !prev_online) {
            st->usb_online = 1;
            st->charging_active = 1;
        } else if (online <= 0 && prev_online) {
            st->usb_online = 0;
            st->charging_active = 0;
        }
        prev_online = (online > 0) ? 1 : 0;

        sleep(2);
    }

    return NULL;
}

/*
 * 解析 battery_log_content 字段
 * 格式: ,[f1],[f2],[f3],[f4],[f5],[f6],[f7],[f8],[f9],[f10],[f11],...
 * 前导逗号前为空, 字段从索引 1 开始
 * strace 交叉验证:
 *   [1]=temp_raw, [2]=temp_01c, [3]=vbat_mv, [4]=vbus_mv, [5]=ibat_ma,
 *   [6]=chip_soc, [7]=ui_soc, [8]=chg_sts, [9]=ac_online, ...
 *   [12]=fcc_mah (累积充电量)
 */
static void parse_battery_log(const char *buf, BatteryLog *blog)
{
    int fields[20] = {0};
    int count = 0;
    const char *p = buf;

    /* 跳过前导逗号 */
    if (*p == ',') p++;

    while (*p && *p != '\n' && count < 20) {
        fields[count++] = (int)strtol(p, (char **)&p, 10);
        if (*p == ',') p++;
    }

    if (count >= 8) {
        blog->temp_raw  = fields[0];    /* [1] battery_temp raw */
        blog->temp_01c  = fields[1];    /* [2] temp_01c */
        blog->vbat_mv   = fields[2];    /* [3] 电池电压 mV */
        blog->vbus_mv   = fields[3];    /* [4] 总线电压 mV */
        blog->ibat_ma   = fields[4];    /* [5] 电池电流 mA */
        blog->chip_soc  = fields[5];    /* [6] chip_soc % */
        blog->ui_soc    = fields[6];    /* [7] ui_soc % */
        blog->chg_sts   = fields[7];    /* [8] 充电状态 */
    }
    if (count >= 12) {
        blog->fcc_mah   = fields[11];   /* [12] 累积充电量 mAh */
    }
}

/*
 * Thread 3: 电池日志监控
 *
 * strace 实测行为:
 *   每 5s 读取 /sys/class/oplus_chg/battery/battery_log_content
 *   格式: 逗号分隔的电池状态数据
 *   示例: ,442,303,2983,2959,973,1,2,1,15,0,0,6571,1,1,...
 */
void *monitor_battery_log_thread(void *arg)
{
    SharedState *st = (SharedState *)arg;
    char buf[4096];

    while (st->running) {
        if (st->usb_online) {
            int n = sysfs_read_battery_log(buf, sizeof(buf));
            if (n > 0) {
                parse_battery_log(buf, &st->blog);
            }
        }

        sleep(5);
    }

    return NULL;
}
