#include "sysfs.h"

#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

/* sysfs 路径常量 */
#define PATH_USB_ONLINE         "/sys/class/power_supply/usb/online"
#define PATH_BATTERY_TEMP       "/sys/class/power_supply/battery/temp"
#define PATH_CHIP_SOC           "/sys/class/oplus_chg/battery/chip_soc"
#define PATH_UFCS_STATUS        "/proc/oplus-votable/UFCS_CURR/status"
#define PATH_ADAPTER_POWER      "/sys/class/oplus_chg/common/adapter_power"
#define PATH_BCC_CURRENT        "/sys/class/oplus_chg/battery/bcc_current"
#define PATH_MMI_CHARGING       "/sys/class/oplus_chg/battery/mmi_charging_enable"
#define PATH_BCC_PARMS          "/sys/class/oplus_chg/battery/bcc_parms"
#define PATH_BATTERY_LOG        "/sys/class/oplus_chg/battery/battery_log_content"

static int open_ro(const char *path)
{
    return open(path, O_RDONLY | O_CLOEXEC);
}

int sysfs_open_ro(const char *path)
{
    return open_ro(path);
}

static int open_wo(const char *path)
{
    return open(path, O_WRONLY | O_CLOEXEC);
}

int sysfs_open_all(SysfsFds *fds)
{
    fds->usb_online          = open_ro(PATH_USB_ONLINE);
    fds->battery_temp        = open_ro(PATH_BATTERY_TEMP);
    fds->chip_soc            = open_ro(PATH_CHIP_SOC);
    /* ufcs_status 用临时打开, 不持久持有 fd */
    fds->adapter_power       = open_ro(PATH_ADAPTER_POWER);
    fds->bcc_current         = open_wo(PATH_BCC_CURRENT);
    fds->mmi_charging_enable = open_wo(PATH_MMI_CHARGING);
    /* PPS/UFCS force 用 open-write-close，不持有持久 fd */

    /* 检查关键 fd 是否打开成功 */
    if (fds->usb_online < 0) return -1;
    return 0;
}

_Static_assert(sizeof(SysfsFds) == 6 * sizeof(int),
               "SysfsFds must contain only int fields for close_all iteration");

void sysfs_close_all(SysfsFds *fds)
{
    int *p = &fds->usb_online;
    for (int i = 0; i < (int)sizeof(SysfsFds) / (int)sizeof(int); i++) {
        if (p[i] >= 0) close(p[i]);
        p[i] = -1;
    }
}

int sysfs_read_int(int fd)
{
    if (fd < 0) return -1;
    char buf[16];
    lseek(fd, 0, SEEK_SET);
    ssize_t n = read(fd, buf, sizeof(buf) - 1);
    if (n <= 0) return -1;
    buf[n] = '\0';
    return atoi(buf);
}

int sysfs_read_str(int fd, char *buf, int bufsz)
{
    if (fd < 0) return -1;
    lseek(fd, 0, SEEK_SET);
    ssize_t n = read(fd, buf, bufsz - 1);
    if (n <= 0) return -1;
    buf[n] = '\0';
    return (int)n;
}

int sysfs_write_int(int fd, int value)
{
    if (fd < 0) return -1;
    char buf[16];
    int len = snprintf(buf, sizeof(buf), "%d", value);
    lseek(fd, 0, SEEK_SET);
    return (int)write(fd, buf, (size_t)len);
}

int sysfs_write_str(int fd, const char *val)
{
    if (fd < 0) return -1;
    lseek(fd, 0, SEEK_SET);
    return (int)write(fd, val, strlen(val));
}

void sysfs_reset_votables(void)
{
    /* strace 确认：初始化时写 "0" 到 4 个 votable 节点 */
    sysfs_write_proc_str(PROC_PPS_FORCE_VAL,      "0");
    sysfs_write_proc_str(PROC_PPS_FORCE_ACTIVE,   "0");
    sysfs_write_proc_str(PROC_UFCS_FORCE_VAL,     "0");
    sysfs_write_proc_str(PROC_UFCS_FORCE_ACTIVE,  "0");
}

static int write_proc(const char *path, const char *buf, int len)
{
    int fd = open(path, O_WRONLY | O_TRUNC | O_CLOEXEC);
    if (fd < 0) return -1;
    int ret = (int)write(fd, buf, (size_t)len);
    close(fd);
    return ret;
}

int sysfs_write_proc_int(const char *path, int value)
{
    char buf[16];
    int len = snprintf(buf, sizeof(buf), "%d", value);
    return write_proc(path, buf, len);
}

int sysfs_write_proc_str(const char *path, const char *val)
{
    return write_proc(path, val, (int)strlen(val));
}

/* 通用临时打开+读取+关闭 */
static int read_temp_file(const char *path, char *buf, int bufsz)
{
    int fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0) return -1;
    ssize_t n = read(fd, buf, bufsz - 1);
    close(fd);
    if (n <= 0) return -1;
    buf[n] = '\0';
    return (int)n;
}

int sysfs_read_bcc_parms(char *buf, int bufsz)
{
    return read_temp_file(PATH_BCC_PARMS, buf, bufsz);
}

int sysfs_read_battery_log(char *buf, int bufsz)
{
    return read_temp_file(PATH_BATTERY_LOG, buf, bufsz);
}

int sysfs_read_ufcs_voters(char *buf, int bufsz)
{
    /* strace 显示线程 2 连续 3 次打开+读取 UFCS_CURR/status */
    /* 这里只读一次，调用方可多次调用 */
    return read_temp_file(PATH_UFCS_STATUS, buf, bufsz);
}
