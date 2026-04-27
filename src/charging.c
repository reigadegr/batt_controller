#include "charging.h"

#include <fcntl.h>
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

/*
 * 写日志到 /data/opbatt/battchg.log 和 stdout
 * strace 确认二进制同时写 stdout(fd1) 和日志文件
 */
#define LOG_PATH "/data/opbatt/battchg.log"

static void log_write(const char *msg)
{
    printf("%s", msg);
    fflush(stdout);

    int fd = open(LOG_PATH, O_WRONLY | O_APPEND | O_CREAT | O_CLOEXEC, 0644);
    if (fd >= 0) {
        write(fd, msg, strlen(msg));
        close(fd);
    }
}

int charging_parse_bcc_parms(const char *str, BccParms *parms)
{
    memset(parms, 0, sizeof(*parms));
    int fields[20] = {0};
    int count = 0;
    const char *p = str;

    while (*p && count < 20) {
        fields[count++] = (int)strtol(p, (char **)&p, 10);
        if (*p == ',') p++;
    }

    if (count < 12) return -1;

    /* strace 886 次读取 + battery_log_content 交叉验证确认的字段映射 */
    parms->fcc           = fields[0];   /* 满电容量 mAh (恒定 ~5896) */
    parms->design_cap    = fields[1];   /* 设计容量 (恒定 ~5888) */
    parms->ic_param_a    = fields[2];   /* 充电IC参数A (线性递减) */
    parms->param_c       = fields[3];   /* 常量 ~2637 */
    parms->param_d       = fields[4];   /* 常量 ~2621 */
    parms->ic_param_b    = fields[5];   /* 充电IC参数B (= ic_param_a + 405) */
    parms->vbat_mv       = fields[6];   /* 电池电压 mV (交叉验证确认) */
    parms->temp_01c      = fields[7];   /* 温度 0.1°C (303=30.3°C) */
    parms->ibat_ma       = fields[8];   /* 电池电流 mA (负值=充电) */
    parms->thermal_hi    = fields[9];   /* 温控阈值上界 (91→85→80) */
    parms->thermal_lo    = fields[10];  /* 温控阈值下界 (= thermal_hi - 11) */
    parms->vbus_mv       = fields[11];  /* 总线电压 mV (精确匹配 battery_log) */

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
 * 充电重置序列
 * strace 确认的完整序列:
 *   1. dumpsys battery set ac 1
 *   2. dumpsys battery set status 2
 *   3. nanosleep(2s)
 *   4. mmi_charging_enable = "0" (禁用充电)
 *   5. nanosleep(1s)
 *   6. mmi_charging_enable = "1" (重新启用)
 *   7. nanosleep(8s) (等待充电重新初始化)
 *   8. dumpsys battery reset (在 sleep(8) 之后执行)
 */
void charging_dumpsys_reset(SysfsFds *fds)
{
    run_dumpsys("set", "ac", "1");
    run_dumpsys("set", "status", "2");

    sleep(2);
    sysfs_write_str(fds->mmi_charging_enable, "0");
    sleep(1);
    sysfs_write_str(fds->mmi_charging_enable, "1");
    sleep(8);

    /* strace 确认: dumpsys reset 在 sleep(8) 之后执行 */
    run_dumpsys("reset", NULL, NULL);
}

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

/*
 * 根据 SoC 计算动态轮询间隔
 * SoC 在 [soc_mon[0], soc_mon[1]] 范围内时使用 interval_ms[0] (快轮询)
 * 范围外使用 interval_ms[1] (慢轮询)
 */
static int calc_poll_interval(const int soc_mon[2], const int interval_ms[2], int soc)
{
    if (interval_ms[0] <= 0 && interval_ms[1] <= 0)
        return -1;  /* 未配置, 使用默认间隔 */

    if (soc_mon[0] <= 0 && soc_mon[1] <= 0)
        return -1;

    if (soc >= soc_mon[0] && soc < soc_mon[1])
        return interval_ms[0] > 0 ? interval_ms[0] : interval_ms[1];

    return interval_ms[1] > 0 ? interval_ms[1] : interval_ms[0];
}

/*
 * 阶段名称字符串
 */
static const char *phase_name(ChargePhase ph)
{
    switch (ph) {
    case PHASE_IDLE: return "IDLE";
    case PHASE_RISE: return "RISE";
    case PHASE_CV:   return "CV";
    case PHASE_TC:   return "TC";
    case PHASE_FULL: return "FULL";
    }
    return "?";
}

/*
 * 充电阶段状态机转换
 *
 * RISE → CV:   vbat >= cv_vol_mv
 * CV   → TC:   soc >= tc_thr_soc || vbat >= tc_vol_thr_mv
 * TC   → FULL: |ibat| <= tc_full_ma && vbat >= tc_vol_full_mv
 * ANY  → IDLE: charge_status == 0
 */
static ChargePhase next_phase(ChargePhase cur, const BattConfig *cfg,
                               const BccParms *parms, int soc)
{
    int vbat = parms->vbat_mv;
    int ibat = parms->ibat_ma < 0 ? -parms->ibat_ma : parms->ibat_ma;

    if (parms->thermal_hi == 0)
        return PHASE_IDLE;

    switch (cur) {
    case PHASE_IDLE:
        return PHASE_RISE;

    case PHASE_RISE:
        if (cfg->cv_vol_mv > 0 && vbat >= cfg->cv_vol_mv)
            return PHASE_CV;
        return PHASE_RISE;

    case PHASE_CV:
        if (cfg->tc_thr_soc > 0 && soc >= cfg->tc_thr_soc)
            return PHASE_TC;
        if (cfg->tc_vol_thr_mv > 0 && vbat >= cfg->tc_vol_thr_mv)
            return PHASE_TC;
        return PHASE_CV;

    case PHASE_TC:
        if (cfg->tc_full_ma > 0 && cfg->tc_vol_full_mv > 0 &&
            ibat <= cfg->tc_full_ma && vbat >= cfg->tc_vol_full_mv)
            return PHASE_FULL;
        return PHASE_TC;

    case PHASE_FULL:
        return PHASE_FULL;
    }

    return cur;
}

void charging_loop(SysfsFds *fds, const BattConfig *cfg, volatile int *running)
{
    char log_buf[1024];
    char ts[32];
    char line[256];
    UfcsVoters voters;
    BccParms parms;

    int current_ma = 500;
    int max_ma = cfg->ufcs_max;
    int cable_max = 0;
    int use_ufcs = 1;
    int inc_step = cfg->inc_step;
    int ramp_idx = 0;
    int restart_count = 0;
    int in_charge_cycle = 0;
    int soc = 0;
    ChargePhase phase = PHASE_IDLE;

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

    /* strace 确认: inc_step = effective_max / 10 */
    inc_step = max_ma > 0 ? max_ma / 10 : cfg->inc_step;

    /* ---- 阶段 4: 输出充电信息日志 ---- */
    get_timestamp(ts, sizeof(ts));
    snprintf(line, sizeof(line),
             "%s UFCS_CHG: AdpMAXma=%dma, CableMAXma=%dma, Maxallow=%dma, Maxset=%dma, OP_chg=1\n",
             ts, voters.max_ma, voters.cable_max_ma, voters.max_ma, max_ma);
    log_write(line);
    snprintf(line, sizeof(line),
             "%s ==== Charger type UFCS, set max current %dma ====\n", ts, max_ma);
    log_write(line);

    /* ---- 阶段 5: 充电控制主循环 ---- */
    while (*running) {
        /* 读取 bcc_parms */
        if (sysfs_read_bcc_parms(log_buf, sizeof(log_buf)) > 0) {
            charging_parse_bcc_parms(log_buf, &parms);
        }

        /* 读取 SoC */
        soc = sysfs_read_int(fds->chip_soc);

        /* 读取电池温度 (strace 确认用 bcc_parms field[7] 即 temp_01c) */
        sysfs_read_int(fds->battery_temp);

        /*
         * 充电周期结束检测:
         * charge_budget 从非零变为 0 时触发 dumpsys 重启
         * 需要先经过非零状态 (in_charge_cycle=1) 才触发
         */
        if (parms.thermal_hi > 0)
            in_charge_cycle = 1;

        if (parms.thermal_hi == 0 && in_charge_cycle) {
            /* 重置所有 votable */
            sysfs_reset_votables(fds);

            /* dumpsys 电池控制序列 + mmi_charging_enable 0→1 */
            charging_dumpsys_reset(fds);

            /* 重置计数限制: 超过 max_ufcs_chg_reset_cc 后等待 ufcs_reset_delay */
            if (cfg->max_ufcs_chg_reset_cc > 0 &&
                restart_count >= cfg->max_ufcs_chg_reset_cc) {
                get_timestamp(ts, sizeof(ts));
                snprintf(line, sizeof(line),
                         "%s ==== Reset limit reached (%d/%d), delay %ds ====\n",
                         ts, restart_count, cfg->max_ufcs_chg_reset_cc,
                         cfg->ufcs_reset_delay > 0 ? cfg->ufcs_reset_delay : 10);
                log_write(line);
                /* 超限后延迟等待, 防止频繁重置 */
                int delay = cfg->ufcs_reset_delay > 0 ? cfg->ufcs_reset_delay : 10;
                for (int i = 0; i < delay * 10 && *running; i++)
                    usleep(100000);
                restart_count = 0;
            }

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

            /* strace 确认: inc_step = effective_max / 10 (非 step_ma / 10) */
            inc_step = max_ma > 0 ? max_ma / 10 : cfg->inc_step;

            get_timestamp(ts, sizeof(ts));
            snprintf(line, sizeof(line),
                     "%s ==== Charger type %s, set max current %dma (restart #%d) ====\n",
                     ts, use_ufcs ? "UFCS" : "PPS", max_ma, ++restart_count);
            log_write(line);

            current_ma = 500;
            ramp_idx = 0;
            in_charge_cycle = 0;
            phase = PHASE_IDLE;
            continue;
        }

        /* 温控: 根据温度调整最大电流 (strace 确认 temp_01c = battery_temp) */
        int temp_offset = get_temp_curr_offset(cfg, parms.temp_01c);
        int effective_max = max_ma;
        if (temp_offset > 0 && effective_max > temp_offset)
            effective_max = temp_offset;

        /* ---- 充电阶段状态机 ---- */
        ChargePhase new_phase = next_phase(phase, cfg, &parms, soc);

        if (new_phase != phase) {
            get_timestamp(ts, sizeof(ts));
            snprintf(line, sizeof(line),
                     "%s ==== Phase %s -> %s (vbat=%dmV, soc=%d%%) ====\n",
                     ts, phase_name(phase), phase_name(new_phase),
                     parms.vbat_mv, soc);
            log_write(line);
            phase = new_phase;
        }

        switch (phase) {
        case PHASE_IDLE:
            /* 未充电, 不做操作 */
            break;

        case PHASE_RISE: {
            /*
             * RISE 三段式 (strace 确认):
             * 1. Quickstart: 500→1400 (同迭代双写, 无 sleep)
             * 2. 递增斜坡: +750, +350, +450, +550
             *    公式: round_to_50(cable_max / (22 - 4*ramp_idx))
             * 3. 全速步进: +800 (= cable_max/10) 直到 cap
             */
            int phase_max = effective_max;

            if (current_ma == 500 && ramp_idx == 0) {
                /* Quickstart: 写 500 后立即写 1400 */
                int qs_step = (cable_max * 9) / 80;
                qs_step = ((qs_step + 25) / 50) * 50;
                write_current(fds, use_ufcs, 500);
                current_ma = 500 + qs_step;
                write_current(fds, use_ufcs, current_ma);
                ramp_idx = 1;
                break;
            }

            if (current_ma >= phase_max) {
                /* 到达 cap, 维持但不重复写入 */
                break;
            }

            int step;
            if (ramp_idx == 1) {
                /* 首个斜坡步: 推测 cable_max*3/32 ≈ 750 */
                step = (cable_max * 3) / 32;
                step = ((step + 25) / 50) * 50;
            } else {
                int divisor = 22 - 4 * (ramp_idx - 2);
                if (divisor <= 10) {
                    step = cable_max > 0 ? cable_max / 10 : inc_step;
                } else {
                    step = (cable_max + divisor / 2) / divisor;
                    step = ((step + 25) / 50) * 50;
                    if (step > inc_step) step = inc_step;
                }
            }

            current_ma += step;
            if (current_ma > phase_max)
                current_ma = phase_max;

            write_current(fds, use_ufcs, current_ma);
            ramp_idx++;
            break;
        }

        case PHASE_CV: {
            /*
             * CV (恒压) 阶段: 电压已达 cv_vol_mv, 限制电流不超过 cv_max_ma
             * 电流逐步递减以维持电压
             */
            int cv_max = cfg->cv_max_ma > 0 ? cfg->cv_max_ma : effective_max;
            if (cv_max > effective_max)
                cv_max = effective_max;

            /* 如果当前电流超过 CV 限制, 逐步降低 (strace 确认步长 ~50mA) */
            if (current_ma > cv_max) {
                current_ma -= cfg->adjust_step > 0 ? cfg->adjust_step : 50;
                if (current_ma < cv_max)
                    current_ma = cv_max;
            }

            write_current(fds, use_ufcs, current_ma);
            break;
        }

        case PHASE_TC: {
            /*
             * TC (涓流) 阶段: 高 SoC, 低电流充电
             * 电流不超过 tc_full_ma, 维持在较低水平
             */
            int tc_max = cfg->tc_full_ma > 0 ? cfg->tc_full_ma : 500;
            if (tc_max > effective_max)
                tc_max = effective_max;

            if (current_ma > tc_max) {
                current_ma -= cfg->adjust_step > 0 ? cfg->adjust_step : 50;
                if (current_ma < tc_max)
                    current_ma = tc_max;
            }

            write_current(fds, use_ufcs, current_ma);
            break;
        }

        case PHASE_FULL:
            /* 满电: 持续写入最小维持电流 */
            write_current(fds, use_ufcs, 500);
            break;
        }

        /* 满电判断: batt_full_thr_mv 额外检查 */
        if (phase != PHASE_FULL && cfg->batt_full_thr_mv > 0 &&
            parms.vbat_mv >= cfg->batt_full_thr_mv) {
            get_timestamp(ts, sizeof(ts));
            snprintf(line, sizeof(line),
                     "%s ==== Battery full: vbat=%dmV >= batt_full_thr_mv=%dmV ====\n",
                     ts, parms.vbat_mv, cfg->batt_full_thr_mv);
            log_write(line);
            phase = PHASE_FULL;
        }

        /* 动态轮询间隔 */
        int poll_ms = cfg->loop_interval_ms;
        if (use_ufcs) {
            int ms = calc_poll_interval(cfg->ufcs_soc_mon, cfg->ufcs_interval_ms, soc);
            if (ms > 0) poll_ms = ms;
        } else {
            int ms = calc_poll_interval(cfg->pps_soc_mon, cfg->pps_interval_ms, soc);
            if (ms > 0) poll_ms = ms;
        }

        usleep((unsigned int)poll_ms * 1000);
    }
}
