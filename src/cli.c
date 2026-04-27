#include "cli.h"

#include <getopt.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

#include "charging.h"

/* 电池型号 profile 表 (15 个型号，来自 r2 逆向 .rodata) */
static const struct {
    const char *name;
    int         index;
    int         param;       /* profile offset+8 的充电参数值 */
} g_battery_models[] = {
    {"B-163",  0,  163},
    {"B-233",  1,  233},
    {"B-283",  2,  283},
    {"B-409",  3,  409},
    {"B-571",  4,  571},
    {"K-163",  5,  163},
    {"K-233",  6,  233},
    {"K-283",  7,  283},
    {"K-409",  8,  409},
    {"K-571",  9,  571},
    {"P-192", 10,  192},
    {"P-224", 11,  224},
    {"P-256", 12,  256},
    {"P-384", 13,  384},
    {"P-521", 14,  521},
};

#define BATTERY_MODEL_COUNT (sizeof(g_battery_models) / sizeof(g_battery_models[0]))

static struct option long_options[] = {
    {"charge",   required_argument, NULL, 'c'},
    {"temp",     no_argument,       NULL, 't'},
    {"soc",      no_argument,       NULL, 's'},
    {"power",    no_argument,       NULL, 'p'},
    {"enable",   required_argument, NULL, 'e'},
    {"disable",  no_argument,       NULL, 'd'},
    {"pps",      required_argument, NULL, 'P'},
    {"ufcs",     required_argument, NULL, 'u'},
    {"log",      no_argument,       NULL, 'l'},
    {"dumpsys",  no_argument,       NULL, 'D'},
    {"model",    required_argument, NULL, 'm'},
    {"service",  no_argument,       NULL, 'S'},
    {NULL,       0,                 NULL, 0},
};

int cli_parse(int argc, char **argv, CliArgs *args)
{
    memset(args, 0, sizeof(*args));
    args->mode = CLI_MODE_SERVICE;

    int opt;
    while ((opt = getopt_long(argc, argv, "c:tsp:e:dP:u:lDm:S", long_options, NULL)) != -1) {
        switch (opt) {
        case 'c':
            args->mode = CLI_MODE_CHARGE;
            args->value = atoi(optarg);
            break;
        case 't':
            args->mode = CLI_MODE_TEMP;
            break;
        case 's':
            args->mode = CLI_MODE_SOC;
            break;
        case 'p':
            args->mode = CLI_MODE_POWER;
            break;
        case 'e':
            args->mode = CLI_MODE_ENABLE;
            args->value = atoi(optarg);
            break;
        case 'd':
            args->mode = CLI_MODE_ENABLE;
            args->value = 0;
            break;
        case 'P':
            args->mode = CLI_MODE_PPS;
            args->value = atoi(optarg);
            break;
        case 'u':
            args->mode = CLI_MODE_UFCS;
            args->value = atoi(optarg);
            break;
        case 'l':
            args->mode = CLI_MODE_LOG;
            break;
        case 'D':
            args->mode = CLI_MODE_DUMPSYS;
            break;
        case 'm':
            args->mode = CLI_MODE_MODEL;
            snprintf(args->model, sizeof(args->model), "%s", optarg);
            break;
        case 'S':
            args->mode = CLI_MODE_SERVICE;
            break;
        default:
            return -1;
        }
    }
    return 0;
}

/* 查找电池型号 profile */
static int find_battery_model(const char *name)
{
    for (int i = 0; i < (int)BATTERY_MODEL_COUNT; i++) {
        if (strcmp(name, g_battery_models[i].name) == 0)
            return i;
    }
    return -1;
}

/* 执行内核日志采集: echo + dmesg | grep */
static int exec_kernel_log(void)
{
    pid_t pid = fork();
    if (pid < 0) return -1;

    if (pid == 0) {
        /* 子进程: sh -c "echo == >> klog && dmesg -T | grep OPLUS_CHG >> klog" */
        execl("/system/bin/sh", "sh", "-c",
              "echo '== == == == == == == == == ==' >> /data/opbatt/kernellog/klog_$(date +%Y-%m-%d).log && "
              "dmesg -T | grep OPLUS_CHG >> /data/opbatt/kernellog/klog_$(date +%Y-%m-%d).log",
              NULL);
        _exit(127);
    }

    int status;
    waitpid(pid, &status, 0);
    return WIFEXITED(status) ? WEXITSTATUS(status) : -1;
}

static int cli_read_sysfs(CliMode mode)
{
    SysfsFds fds;
    if (sysfs_open_all(&fds) < 0) {
        fprintf(stderr, "sysfs_open_all failed\n");
        return -1;
    }
    int val;
    switch (mode) {
    case CLI_MODE_TEMP:  val = sysfs_read_int(fds.battery_temp);  break;
    case CLI_MODE_SOC:   val = sysfs_read_int(fds.chip_soc);     break;
    default:             val = sysfs_read_int(fds.adapter_power); break;
    }
    printf("%d\n", val);
    sysfs_close_all(&fds);
    return 0;
}

static int cli_force_current(const CliArgs *args)
{
    SysfsFds fds;
    if (sysfs_open_all(&fds) < 0) return -1;
    if (args->mode == CLI_MODE_PPS) {
        sysfs_write_int(fds.pps_force_val, args->value);
        sysfs_write_str(fds.pps_force_active, "1");
        printf("PPS force_val set to %d mA\n", args->value);
    } else {
        sysfs_write_int(fds.ufcs_force_val, args->value);
        sysfs_write_str(fds.ufcs_force_active, "1");
        printf("UFCS force_val set to %d mA\n", args->value);
    }
    sysfs_close_all(&fds);
    return 0;
}

int cli_exec(const CliArgs *args, const BattConfig *cfg)
{
    SysfsFds fds;
    memset(&fds, 0, sizeof(fds));

    switch (args->mode) {
    case CLI_MODE_TEMP:
    case CLI_MODE_SOC:
    case CLI_MODE_POWER:
        return cli_read_sysfs(args->mode);
    case CLI_MODE_CHARGE: {
        if (sysfs_open_all(&fds) < 0) return -1;
        sysfs_write_int(fds.bcc_current, args->value);
        printf("bcc_current set to %d mA\n", args->value);
        sysfs_close_all(&fds);
        return 0;
    }
    case CLI_MODE_ENABLE: {
        if (sysfs_open_all(&fds) < 0) return -1;
        sysfs_write_int(fds.mmi_charging_enable, args->value);
        printf("mmi_charging_enable set to %d\n", args->value);
        sysfs_close_all(&fds);
        return 0;
    }
    case CLI_MODE_PPS:
    case CLI_MODE_UFCS:
        return cli_force_current(args);
    case CLI_MODE_LOG: {
        int ret = exec_kernel_log();
        if (ret == 0)
            printf("kernel log saved to /data/opbatt/kernellog/\n");
        else
            fprintf(stderr, "kernel log collection failed (exit %d)\n", ret);
        return ret;
    }
    case CLI_MODE_DUMPSYS: {
        /* strace 确认的完整重置序列 */
        if (sysfs_open_all(&fds) < 0) return -1;
        charging_dumpsys_reset(&fds);
        printf("dumpsys battery reset sequence complete\n");
        sysfs_close_all(&fds);
        return 0;
    }
    case CLI_MODE_MODEL: {
        int idx = find_battery_model(args->model);
        if (idx < 0) {
            fprintf(stderr, "unknown battery model: %s\n", args->model);
            fprintf(stderr, "available models:");
            for (int i = 0; i < (int)BATTERY_MODEL_COUNT; i++)
                fprintf(stderr, " %s", g_battery_models[i].name);
            fprintf(stderr, "\n");
            return -1;
        }
        printf("model: %s (index=%d, param=%d)\n",
               g_battery_models[idx].name,
               g_battery_models[idx].index,
               g_battery_models[idx].param);
        (void)cfg;
        return 0;
    }
    case CLI_MODE_SERVICE:
        /* 不应到达这里，服务模式由 main.c 处理 */
        return 0;
    }

    return -1;
}
