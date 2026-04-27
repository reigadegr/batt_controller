#include "charging.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

/* 获取时间戳字符串 "[YYYY-MM-DD-HH:MM:SS]" */
static void get_timestamp(char *buf, int bufsz)
{
    time_t now = time(NULL);
    struct tm *t = localtime(&now);
    strftime(buf, bufsz, "[%Y-%m-%d-%H:%M:%S]", t);
}

int charging_parse_bcc_parms(const char *str, BccParms *parms)
{
    memset(parms, 0, sizeof(*parms));
    int fields[19] = {0};
    int count = 0;
    const char *p = str;

    while (*p && count < 19) {
        fields[count++] = (int)strtol(p, (char **)&p, 10);
        if (*p == ',') p++;
    }

    if (count < 12) return -1;

    parms->vbat_mv      = fields[0];
    parms->ibat_ma      = fields[1];
    parms->temp_01c     = fields[2];
    parms->fcc          = fields[3];
    parms->rm           = fields[4];
    parms->soh          = fields[5];
    parms->vbus_mv      = fields[6];
    parms->ibus_ma      = fields[7];
    parms->power_mw     = fields[8];
    parms->cycles       = fields[9];
    parms->charge_status = fields[10];
    parms->batt_vol     = fields[11];

    if (count >= 19) {
        parms->field_12     = fields[12];
        parms->field_13     = fields[13];
        parms->ufcs_max_ma  = fields[14];
        parms->ufcs_en      = fields[15];
        parms->pps_max_ma   = fields[16];
        parms->pps_en       = fields[17];
        parms->cable_type   = fields[18];
    }

    return 0;
}

int charging_parse_ufcs_voters(const char *status, UfcsVoters *voters)
{
    memset(voters, 0, sizeof(*voters));

    const char *p;

    p = strstr(status, "MAX_VOTER:");
    if (p) {
        p = strstr(p, "v=");
        if (p) voters->max_ma = atoi(p + 2);
    }

    p = strstr(status, "CABLE_MAX_VOTER:");
    if (p) {
        p = strstr(p, "v=");
        if (p) voters->cable_max_ma = atoi(p + 2);
    }

    p = strstr(status, "STEP_VOTER:");
    if (p) {
        p = strstr(p, "v=");
        if (p) voters->step_ma = atoi(p + 2);
    }

    p = strstr(status, "BCC_VOTER:");
    if (p) {
        p = strstr(p, "v=");
        if (p) voters->bcc_ma = atoi(p + 2);
    }

    return 0;
}

/* fork+execvp 执行单条 dumpsys 命令 */
static void run_dumpsys(const char *a1, const char *a2, const char *a3)
{
    pid_t pid = fork();
    if (pid < 0) return;

    if (pid == 0) {
        const char *argv[5];
        argv[0] = "dumpsys";
        argv[1] = "battery";
        int ac = 2;
        if (a1) argv[ac++] = a1;
        if (a2) argv[ac++] = a2;
        if (a3) argv[ac++] = a3;
        argv[ac] = NULL;
        execvp("dumpsys", (char *const *)argv);
        _exit(127);
    }

    int status;
    waitpid(pid, &status, 0);
}

/*
 * dumpsys 电池控制序列
 *
 * strace 确认的顺序:
 *   1. dumpsys battery set ac 1
 *   2. dumpsys battery set status 2
 *   3. dumpsys battery reset
 */
void charging_dumpsys_reset(void)
{
    run_dumpsys("set", "ac", "1");
    run_dumpsys("set", "status", "2");
    run_dumpsys("reset", NULL, NULL);
}

/*
 * 根据 bcc_parms 和配置决定使用 UFCS 还是 PPS 协议
 * 返回: 1=UFCS, 0=PPS
 */
static int choose_protocol(const BattConfig *cfg, const BccParms *parms)
{
    if (cfg->cable_override)
        return (cfg->cable_override > 0) ? 1 : 0;

    if (parms->ufcs_en && parms->ufcs_max_ma > 0)
        return 1;

    if (parms->pps_en && parms->pps_max_ma > 0)
        return 0;

    return 1;
}

/*
 * 根据温度查找电流偏移
 */
static int get_temp_curr_offset(const BattConfig *cfg, int temp_01c)
{
    int temp = temp_01c / 10;
    for (int i = 0; i < cfg->temp_range_count; i++) {
        if (temp <= cfg->temp_range[i]) {
            if (i < cfg->temp_curr_offset_count)
                return cfg->temp_curr_offset[i];
            return 0;
        }
    }
    return 0;
}

/*
 * 写入电流到 UFCS 或 PPS votable
 */
static void write_current(SysfsFds *fds, int use_ufcs, int ma)
{
    if (use_ufcs) {
        sysfs_write_int(fds->ufcs_force_val, ma);
        sysfs_write_str(fds->ufcs_force_active, "1");
    } else {
        sysfs_write_int(fds->pps_force_val, ma);
        sysfs_write_str(fds->pps_force_active, "1");
    }
}

void charging_loop(SysfsFds *fds, const BattConfig *cfg, volatile int *running)
{
    char log_buf[1024];
    char ts[32];
    UfcsVoters voters;
    BccParms parms;

    int current_ma = 500;
    int max_ma = cfg->ufcs_max;
    int cable_max = 0;
    int wait_counter = 0;
    int use_ufcs = 1;
    int inc_step = cfg->inc_step;
    int restart_count = 0;
    int prev_ufcs_en = -1;
    int in_charge_cycle = 0;  /* 已进入充电周期 (charge_status 曾非零) */

    /* ---- 阶段 1: 读取电池状态日志 ---- */
    sysfs_read_battery_log(log_buf, sizeof(log_buf));

    /* ---- 阶段 2: 重置 votable ---- */
    sysfs_reset_votables(fds);

    /* ---- 阶段 3: 读取 UFCS voter 信息 (连续 3 次) ---- */
    for (int i = 0; i < 3; i++) {
        if (sysfs_read_ufcs_voters(log_buf, sizeof(log_buf)) > 0) {
            charging_parse_ufcs_voters(log_buf, &voters);
        }
    }

    cable_max = voters.cable_max_ma;
    if (cable_max > 0 && cable_max < max_ma)
        max_ma = cable_max;

    /* ---- 阶段 4: 输出充电信息日志 ---- */
    get_timestamp(ts, sizeof(ts));
    printf("%s UFCS_CHG: AdpMAXma=%dma, CableMAXma=%dma, Maxallow=%dma, Maxset=%dma, OP_chg=1\n",
           ts, voters.max_ma, voters.cable_max_ma, voters.max_ma, max_ma);
    printf("%s ==== Charger type UFCS, set max current %dma ====\n", ts, max_ma);
    fflush(stdout);

    /* ---- 阶段 5: 充电控制主循环 ---- */
    while (*running) {
        /* 读取 bcc_parms */
        if (sysfs_read_bcc_parms(log_buf, sizeof(log_buf)) > 0) {
            charging_parse_bcc_parms(log_buf, &parms);
        }

        /* 检测 ufcs_en 状态变化: 1→0 时切换步长为 STEP_VOTER/10 */
        if (parms.ufcs_en != prev_ufcs_en) {
            if (prev_ufcs_en == 1 && parms.ufcs_en == 0 && voters.step_ma > 0) {
                inc_step = voters.step_ma / 10;
            } else if (parms.ufcs_en == 1) {
                inc_step = cfg->inc_step;
            }
            prev_ufcs_en = parms.ufcs_en;
        }

        /* 读取电池温度 */
        sysfs_read_int(fds->battery_temp);

        /*
         * 充电周期结束检测:
         * strace 显示 charge_status 从非零变为 0 时触发 dumpsys 重启
         * 需要先经过非零状态 (in_charge_cycle=1) 才触发
         */
        if (parms.charge_status > 0)
            in_charge_cycle = 1;

        if (parms.charge_status == 0 && in_charge_cycle) {
            /* 重置所有 votable */
            sysfs_reset_votables(fds);

            /* dumpsys 电池控制序列 */
            charging_dumpsys_reset();

            /* 根据 bcc_parms 决定下一周期的协议 */
            use_ufcs = choose_protocol(cfg, &parms);

            /* 重新读取 voter 信息确定新的最大电流和步长 */
            for (int i = 0; i < 3; i++) {
                if (sysfs_read_ufcs_voters(log_buf, sizeof(log_buf)) > 0)
                    charging_parse_ufcs_voters(log_buf, &voters);
            }

            if (use_ufcs) {
                max_ma = cfg->ufcs_max;
                if (parms.ufcs_max_ma > 0 && parms.ufcs_max_ma < max_ma)
                    max_ma = parms.ufcs_max_ma;
            } else {
                max_ma = cfg->pps_max;
                if (parms.pps_max_ma > 0 && parms.pps_max_ma < max_ma)
                    max_ma = parms.pps_max_ma;
            }

            cable_max = voters.cable_max_ma;
            if (cable_max > 0 && cable_max < max_ma)
                max_ma = cable_max;

            /*
             * 电流递增步长:
             * strace 显示重启后步长 = STEP_VOTER / 10
             * 例: STEP_VOTER=8000 → 步长=800mA
             */
            if (voters.step_ma > 0)
                inc_step = voters.step_ma / 10;
            else
                inc_step = cfg->inc_step;

            get_timestamp(ts, sizeof(ts));
            printf("%s ==== Charger type %s, set max current %dma (restart #%d) ====\n",
                   ts, use_ufcs ? "UFCS" : "PPS", max_ma, ++restart_count);
            fflush(stdout);

            current_ma = 1000;
            wait_counter = 0;
            prev_ufcs_en = -1;
            in_charge_cycle = 0;
            continue;
        }

        /* 温控: 根据温度调整最大电流 */
        int temp_offset = get_temp_curr_offset(cfg, parms.temp_01c);
        int effective_max = max_ma;
        if (temp_offset > 0 && effective_max > temp_offset)
            effective_max = temp_offset;

        /* 电流递增逻辑 */
        if (current_ma <= effective_max) {
            if (cfg->curr_inc_wait_cycles > 0 &&
                wait_counter < cfg->curr_inc_wait_cycles) {
                wait_counter++;
                write_current(fds, use_ufcs, current_ma);
            } else {
                wait_counter = 0;

                if (current_ma == 500 && restart_count == 0) {
                    /* 首次启动: 500 → 5000 跳转 */
                    write_current(fds, use_ufcs, 500);
                    current_ma = 5000;
                    write_current(fds, use_ufcs, current_ma);
                } else {
                    write_current(fds, use_ufcs, current_ma);
                    current_ma += inc_step;
                }
            }
        } else {
            write_current(fds, use_ufcs, effective_max);
        }

        /* 读取 chip_soc (在写入之后) */
        sysfs_read_int(fds->chip_soc);

        usleep((unsigned int)(cfg->loop_interval_ms) * 1000);
    }
}
