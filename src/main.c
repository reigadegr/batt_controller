/*
 * payload.elf.no_license (v7.3.2) 还原实现
 *
 * 架构: 3 线程模型 (pthread_create)
 *   Thread 1: USB 在线监控 (每 2s)
 *   Thread 2: UFCS 充电控制 (主充电循环)
 *   Thread 3: 电池日志监控 (每 5s)
 *
 * 基于 strace 动态分析 + r2 静态逆向协同还原
 */

#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "cli.h"
#include "config.h"
#include "charging.h"
#include "monitor.h"

#define CONFIG_PATH "/data/opbatt/batt_control"

/* 全局共享状态 */
static SharedState g_state;

/* 充电控制线程包装 */
static void *charging_thread_wrapper(void *arg)
{
    SharedState *st = (SharedState *)arg;

    /* 等待 USB 在线 */
    while (st->running && !st->charging_active) {
        usleep(100000);  /* 100ms */
    }

    if (!st->running) return NULL;

    /* 进入充电控制主循环 */
    charging_loop(&st->fds, &st->config, &st->charging_active);

    return NULL;
}

static void sighandler(int sig)
{
    (void)sig;
    g_state.running = 0;
    g_state.charging_active = 0;
}

static void load_config(BattConfig *cfg)
{
    if (config_parse(CONFIG_PATH, cfg) < 0) {
        memset(cfg, 0, sizeof(*cfg));
        cfg->enabled = 1;
        cfg->adjust_step = 50;
        cfg->inc_step = 100;
        cfg->dec_step = 100;
        cfg->ufcs_max = 9100;
        cfg->pps_max = 5000;
        cfg->loop_interval_ms = 2000;
    }
}

int main(int argc, char **argv)
{
    CliArgs cli;
    if (cli_parse(argc, argv, &cli) < 0)
        return 1;

    /* 一次性命令模式: 执行后直接退出 */
    if (cli.mode != CLI_MODE_SERVICE) {
        BattConfig cfg;
        load_config(&cfg);
        return cli_exec(&cli, &cfg);
    }

    /* 服务模式 */
    memset(&g_state, 0, sizeof(g_state));
    g_state.running = 1;

    signal(SIGINT,  sighandler);
    signal(SIGTERM, sighandler);
    signal(SIGPIPE, SIG_IGN);

    load_config(&g_state.config);
    config_dump(&g_state.config);

    pthread_t tid_usb, tid_charging, tid_battery;

    if (pthread_create(&tid_usb, NULL, monitor_usb_thread, &g_state) != 0) {
        perror("pthread_create usb_monitor");
        return 1;
    }
    if (pthread_create(&tid_charging, NULL, charging_thread_wrapper, &g_state) != 0) {
        perror("pthread_create charging");
        return 1;
    }
    if (pthread_create(&tid_battery, NULL, monitor_battery_log_thread, &g_state) != 0) {
        perror("pthread_create battery_log");
        return 1;
    }

    pthread_join(tid_usb, NULL);
    pthread_join(tid_charging, NULL);
    pthread_join(tid_battery, NULL);

    return 0;
}
