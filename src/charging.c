#include "charging.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
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

/*
 * 解析 UFCS_CURR/status 输出
 * 格式示例:
 *   UFCS_CURR: MAX_VOTER:        en=1 v=9100
 *   UFCS_CURR: CABLE_MAX_VOTER:  en=1 v=8000
 *   UFCS_CURR: STEP_VOTER:       en=1 v=5300
 *   UFCS_CURR: BCC_VOTER:        en=0 v=0
 */
int charging_parse_ufcs_voters(const char *status, UfcsVoters *voters)
{
    memset(voters, 0, sizeof(*voters));

    const char *p;

    /* MAX_VOTER */
    p = strstr(status, "MAX_VOTER:");
    if (p) {
        p = strstr(p, "v=");
        if (p) voters->max_ma = atoi(p + 2);
    }

    /* CABLE_MAX_VOTER */
    p = strstr(status, "CABLE_MAX_VOTER:");
    if (p) {
        p = strstr(p, "v=");
        if (p) voters->cable_max_ma = atoi(p + 2);
    }

    /* STEP_VOTER */
    p = strstr(status, "STEP_VOTER:");
    if (p) {
        p = strstr(p, "v=");
        if (p) voters->step_ma = atoi(p + 2);
    }

    /* BCC_VOTER */
    p = strstr(status, "BCC_VOTER:");
    if (p) {
        p = strstr(p, "v=");
        if (p) voters->bcc_ma = atoi(p + 2);
    }

    return 0;
}

/*
 * 充电控制主循环
 *
 * strace 实测的完整流程:
 *
 * 1. 读 battery_log_content → 解析电池状态
 * 2. 写 "0" 到 PPS/UFCS force (重置)
 * 3. 读 UFCS_CURR/status × 3 → 解析 voter
 * 4. 读时区信息 (tzdata) → 生成时间戳
 * 5. 输出 UFCS 充电信息日志
 * 6. 循环:
 *    a. 读 bcc_parms
 *    b. 读 battery_temp
 *    c. 写 UFCS force_val = current_ma (递增)
 *    d. 写 UFCS force_active = "1"
 *    e. 读 chip_soc
 *    f. sleep ~loop_interval
 */
void charging_loop(SysfsFds *fds, const BattConfig *cfg, volatile int *running)
{
    char log_buf[1024];
    char ts[32];
    UfcsVoters voters;
    BccParms parms;

    int current_ma = 500;          /* 起始电流 */
    int max_ma = cfg->ufcs_max;    /* 最大电流 */
    int cable_max = 0;             /* 线缆最大电流 (从 voter 解析) */

    /* ---- 阶段 1: 读取电池状态日志 ---- */
    if (sysfs_read_battery_log(log_buf, sizeof(log_buf)) > 0) {
        /* battery_log_content 格式: 逗号分隔的电池状态数据 */
        /* 字段包括: vbat, ibat, temp, fcc, rm, soc, ... */
    }

    /* ---- 阶段 2: 重置 votable ---- */
    sysfs_reset_votables(fds);

    /* ---- 阶段 3: 读取 UFCS voter 信息 (连续 3 次) ---- */
    for (int i = 0; i < 3; i++) {
        if (sysfs_read_ufcs_voters(log_buf, sizeof(log_buf)) > 0) {
            charging_parse_ufcs_voters(log_buf, &voters);
        }
    }

    /* 从 voter 确定实际最大电流 */
    cable_max = voters.cable_max_ma;
    if (cable_max > 0 && cable_max < max_ma)
        max_ma = cable_max;

    /* ---- 阶段 4: 输出充电信息日志 ---- */
    get_timestamp(ts, sizeof(ts));
    printf("%s UFCS_CHG: AdpMAXma=%dma, CableMAXma=%dma, Maxallow=%dma, Maxset=%dma, OP_chg=1\n",
           ts, voters.max_ma, voters.cable_max_ma, voters.max_ma, max_ma);
    printf("%s ==== Charger type UFCS, set max current %dma ====\n", ts, max_ma);
    fflush(stdout);

    /* ---- 阶段 5: 充电控制循环 ---- */
    while (*running) {
        /* 读取 bcc_parms */
        if (sysfs_read_bcc_parms(log_buf, sizeof(log_buf)) > 0) {
            charging_parse_bcc_parms(log_buf, &parms);
        }

        /* 读取电池温度和 chip_soc (用于后续温控判断) */
        sysfs_read_int(fds->battery_temp);
        sysfs_read_int(fds->chip_soc);

        /* 电流递增逻辑 */
        if (current_ma <= max_ma) {
            /* 阶段 A: 从 500 跳到 inc_step 的整数倍 */
            if (current_ma == 500) {
                /* strace: 先写 500, 再写 5000 */
                sysfs_write_int(fds->ufcs_force_val, 500);
                sysfs_write_str(fds->ufcs_force_active, "1");
                current_ma = 5000;
                sysfs_write_int(fds->ufcs_force_val, current_ma);
                sysfs_write_str(fds->ufcs_force_active, "1");
            } else {
                /* 每次递增 inc_step (默认 100mA) */
                sysfs_write_int(fds->ufcs_force_val, current_ma);
                sysfs_write_str(fds->ufcs_force_active, "1");
                current_ma += cfg->inc_step;
            }
        } else {
            /* 已达最大电流，维持输出 */
            sysfs_write_int(fds->ufcs_force_val, max_ma);
            sysfs_write_str(fds->ufcs_force_active, "1");
        }

        /* 等待循环间隔 */
        usleep((unsigned int)(cfg->loop_interval_ms) * 1000);
    }
}
