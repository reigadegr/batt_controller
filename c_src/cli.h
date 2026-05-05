#ifndef CLI_H
#define CLI_H

#include "config.h"
#include "sysfs.h"

/* CLI 运行模式 */
typedef enum {
    CLI_MODE_SERVICE,       /* --service: 进入服务模式 (默认) */
    CLI_MODE_CHARGE,        /* -c <mA>: 设置 BCC 充电电流 */
    CLI_MODE_TEMP,          /* -t: 读取电池温度 */
    CLI_MODE_SOC,           /* -s: 读取芯片 SoC */
    CLI_MODE_POWER,         /* -p: 读取适配器功率 */
    CLI_MODE_ENABLE,        /* -e <0|1>: 使能/禁用充电 */
    CLI_MODE_PPS,           /* -P <mA>: 强制 PPS 电流 */
    CLI_MODE_UFCS,          /* -u <mA>: 强制 UFCS 电流 */
    CLI_MODE_LOG,           /* -l: 抓取内核充电日志 */
    CLI_MODE_DUMPSYS,       /* -D: dumpsys 电池控制 */
    CLI_MODE_DUMPSYS_SET_AC,    /* -A: dumpsys battery set ac 1 */
    CLI_MODE_DUMPSYS_SET_STATUS, /* -T: dumpsys battery set status 2 */
    CLI_MODE_MODEL,         /* -m <name>: 查询电池型号 */
} CliMode;

/* CLI 解析结果 */
typedef struct {
    CliMode mode;
    int      value;         /* -c/-e/-P/-u 的参数值 */
    char     model[32];     /* -m 的型号名 */
} CliArgs;

/* 解析命令行选项，成功返回 0，失败返回 -1 */
int cli_parse(int argc, char **argv, CliArgs *args);

/* 执行一次性命令 (非服务模式)，成功返回 0 */
int cli_exec(const CliArgs *args, const BattConfig *cfg);

#endif /* CLI_H */
