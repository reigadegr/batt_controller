#ifndef SYSFS_H
#define SYSFS_H

/*
 * sysfs / proc 读写模块
 * 管理充电控制相关的文件描述符
 *
 * fd 映射 (来自 strace 实测):
 *   fd 3:  /sys/class/power_supply/usb/online          (R)
 *   fd 4:  /sys/class/power_supply/battery/temp        (R)
 *   fd 5:  /sys/class/oplus_chg/battery/chip_soc       (R)
 *   fd 7:  /sys/class/oplus_chg/common/adapter_power   (R)
 *   fd 8:  /sys/class/oplus_chg/battery/bcc_current    (W)
 *   fd 9:  /sys/class/oplus_chg/battery/mmi_charging_enable (W)
 *   fd 10: /proc/oplus-votable/PPS_CURR/force_val      (W)
 *   fd 11: /proc/oplus-votable/PPS_CURR/force_active   (W)
 *   fd 12: /proc/oplus-votable/UFCS_CURR/force_val     (W)
 *   fd 13: /proc/oplus-votable/UFCS_CURR/force_active  (W)
 *   (UFCS_CURR/status 用临时打开 read_temp_file, fd 6 不持久持有)
 */

/* sysfs/proc 文件描述符集合 */
typedef struct {
    int usb_online;              /* fd 3 */
    int battery_temp;            /* fd 4 */
    int chip_soc;                /* fd 5 */
    /* ufcs_status 用临时打开 (read_temp_file), 不需要持久 fd */
    int adapter_power;           /* fd 7 */
    int bcc_current;             /* fd 8  (W) */
    int mmi_charging_enable;     /* fd 9  (W) */
    int pps_force_val;           /* fd 10 (W) */
    int pps_force_active;        /* fd 11 (W) */
    int ufcs_force_val;          /* fd 12 (W) */
    int ufcs_force_active;       /* fd 13 (W) */
} SysfsFds;

/* 临时打开一个 sysfs 节点 (只读)，返回 fd，失败返回 -1 */
int sysfs_open_ro(const char *path);

/* 打开所有 sysfs/proc 节点，返回 0 成功，-1 失败 */
int sysfs_open_all(SysfsFds *fds);

/* 关闭所有 fd */
void sysfs_close_all(SysfsFds *fds);

/* 读取整数值 (lseek 到开头再读) */
int sysfs_read_int(int fd);

/* 读取字符串 (lseek 到开头再读)，返回读取的字节数 */
int sysfs_read_str(int fd, char *buf, int bufsz);

/* 写入整数值 */
int sysfs_write_int(int fd, int value);

/* 写入字符串 */
int sysfs_write_str(int fd, const char *val);

/* 初始化 PPS/UFCS force 为 0 (关闭强制模式) */
void sysfs_reset_votables(SysfsFds *fds);

/* 读取 bcc_parms (临时打开 /sys/class/oplus_chg/battery/bcc_parms) */
int sysfs_read_bcc_parms(char *buf, int bufsz);

/* 读取 battery_log_content (临时打开) */
int sysfs_read_battery_log(char *buf, int bufsz);

/* 读取 UFCS_CURR/status (临时打开，需要多次读取) */
int sysfs_read_ufcs_voters(char *buf, int bufsz);

#endif /* SYSFS_H */
