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

int main(int argc, char **argv)
{
    (void)argc;
    (void)argv;

    /* 初始化共享状态 */
    memset(&g_state, 0, sizeof(g_state));
    g_state.running = 1;

    /* 注册信号处理 */
    signal(SIGINT,  sighandler);
    signal(SIGTERM, sighandler);
    signal(SIGPIPE, SIG_IGN);

    /* 解析配置文件 */
    if (config_parse(CONFIG_PATH, &g_state.config) < 0) {
        /* 配置文件不可读时使用默认值 */
        memset(&g_state.config, 0, sizeof(g_state.config));
        g_state.config.enabled = 1;
        g_state.config.adjust_step = 50;
        g_state.config.inc_step = 100;
        g_state.config.dec_step = 100;
        g_state.config.ufcs_max = 9100;
        g_state.config.pps_max = 5000;
        g_state.config.loop_interval_ms = 2000;
    }

    /* 打印配置 (复现原始输出) */
    config_dump(&g_state.config);

    /* 创建 3 个线程 */
    pthread_t tid_usb, tid_charging, tid_battery;

    /* Thread 1: USB 在线监控 */
    if (pthread_create(&tid_usb, NULL, monitor_usb_thread, &g_state) != 0) {
        perror("pthread_create usb_monitor");
        return 1;
    }

    /* Thread 2: 充电控制 */
    if (pthread_create(&tid_charging, NULL, charging_thread_wrapper, &g_state) != 0) {
        perror("pthread_create charging");
        return 1;
    }

    /* Thread 3: 电池日志 */
    if (pthread_create(&tid_battery, NULL, monitor_battery_log_thread, &g_state) != 0) {
        perror("pthread_create battery_log");
        return 1;
    }

    /* 主线程等待所有子线程 (复现 strace 中的 futex wait) */
    pthread_join(tid_usb, NULL);
    pthread_join(tid_charging, NULL);
    pthread_join(tid_battery, NULL);

    return 0;
}
