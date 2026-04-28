#include "charging_internal.h"
#include "sysfs.h"

#include <stdio.h>
#include <unistd.h>

/* 充电周期结束处理: thermal_hi 极低 + 当前电流很小时触发 */
int handle_cycle_end(LoopCtx *c, char *log_buf)
{
    char ts[32], line[256];

    /* strace 确认: thermal_hi 降到 <=20 且已进入充电周期时重置 */
    if (c->parms.thermal_hi > 0)
        c->in_charge_cycle = 1;

    if (c->parms.thermal_hi > 20 || !c->in_charge_cycle || c->current_ma > 100)
        return 0;

    /* 重置所有 votable */
    sysfs_reset_votables();

    /* dumpsys 电池控制序列 */
    charging_dumpsys_reset(c->fds);

    /* 重置计数限制: 超过 max_ufcs_chg_reset_cc 后等待 ufcs_reset_delay */
    c->restart_count++;
    if (c->cfg->max_ufcs_chg_reset_cc > 0 &&
        c->restart_count > c->cfg->max_ufcs_chg_reset_cc) {
        get_timestamp(ts, sizeof(ts));
        snprintf(line, sizeof(line),
                 "%s ==== Reset limit reached (%d/%d), delay %ds ====\n",
                 ts, c->restart_count, c->cfg->max_ufcs_chg_reset_cc,
                 c->cfg->ufcs_reset_delay > 0 ? c->cfg->ufcs_reset_delay : 10);
        log_write(line);
        int delay = c->cfg->ufcs_reset_delay > 0 ? c->cfg->ufcs_reset_delay : 10;
        int delay_ms = c->cfg->loop_interval_ms > 0 ? c->cfg->loop_interval_ms : 450;
        for (int i = 0; i < delay * (1000 / delay_ms) && *c->running; i++)
            usleep((unsigned int)delay_ms * 1000);
        c->restart_count = 0;
    }

    /* 根据 bcc_parms 决定下一周期的协议 */
    c->use_ufcs = choose_protocol(c->cfg, &c->parms);

    /* 重新读取 voter 信息确定新的最大电流和步长 */
    read_voters_3x(log_buf, 1024, &c->voters);

    c->cable_max = c->voters.cable_max_ma;
    c->max_ma = clamp_max_ma(c->use_ufcs ? c->cfg->ufcs_max : c->cfg->pps_max,
                             c->use_ufcs ? c->parms.ufcs_max_ma : c->parms.pps_max_ma,
                             c->cable_max);

    /* strace 确认: inc_step = effective_max / 10 (非 step_ma / 10) */
    c->inc_step = c->max_ma > 0 ? c->max_ma / 10 : c->cfg->inc_step;

    get_timestamp(ts, sizeof(ts));
    snprintf(line, sizeof(line),
             "%s ==== Charger type %s, set max current %dma (restart #%d) ====\n",
             ts, c->use_ufcs ? "UFCS" : "PPS", c->max_ma, c->restart_count);
    log_write(line);

    c->current_ma = 500;
    c->ramp_idx = 0;
    c->cv_step_idx = 0;
    c->cv_holding = 0;
    c->in_charge_cycle = 0;
    c->phase = PHASE_RESTART_RISE;
    return 1;
}

/* 计算 effective_max: 温控 + thermal_hi + STEP_VOTER 限流 */
void calc_effective_max(LoopCtx *c)
{
    int temp_offset = get_temp_curr_offset(c->cfg, c->parms.temp_01c);
    c->effective_max = c->max_ma;
    if (temp_offset > 0 && c->effective_max > temp_offset)
        c->effective_max = temp_offset;

    /* thermal_hi 限流: strace 确认 thermal_hi 阶梯 91→85→80 限制电流上限 */
    if (c->parms.thermal_hi > 0) {
        int thermal_cap = c->parms.thermal_hi * 100;
        if (thermal_cap > 0 && thermal_cap < c->effective_max)
            c->effective_max = thermal_cap;
    }

    /* STEP_VOTER 限流: strace 确认 STEP_VOTER 从 9100 变为 8000 */
    if (c->voters.step_ma > 0 && c->voters.step_ma < c->effective_max)
        c->effective_max = c->voters.step_ma;
}

/* RISE / RESTART_RISE 阶段 */
void exec_rise(LoopCtx *c)
{
    int phase_max = c->effective_max;

    if (c->phase == PHASE_RESTART_RISE) {
        /* 重启 RISE: +50mA 线性爬升, 无 quickstart
         * strace 确认 (2026-04-28 完整周期):
         * 550→600→650→...→3500, 每步 +50mA, ~480ms 间隔
         */
        int step = c->cfg->restart_rise_step > 0 ? c->cfg->restart_rise_step : 50;
        if (c->current_ma < phase_max) {
            c->current_ma += step;
            if (c->current_ma > phase_max)
                c->current_ma = phase_max;
            write_current(c->fds, c->use_ufcs, c->current_ma);
        }
        return;
    }

    /* 以下为首次充电的 quickstart 三段式 RISE */

    if (c->current_ma == 500 && c->ramp_idx == 0) {
        /* Quickstart: 写 500 后立即写高值
         * vbat >= rise_quickstep_thr: 直接跳到 cable_max-750
         * vbat < rise_quickstep_thr:  渐进 500 + cable_max*13/80
         */
        write_current(c->fds, c->use_ufcs, 500);
        if (c->cfg->rise_quickstep_thr_mv > 0 &&
            c->parms.vbat_mv >= c->cfg->rise_quickstep_thr_mv) {
            /* 高电压 quickstep: 一步逼近 cable_max */
            int margin = (c->cable_max * 3) / 32;
            margin = ((margin + 25) / 50) * 50;
            c->current_ma = c->cable_max - margin;
            c->ramp_idx = 99;  /* 跳过 ramp, 直接全速步进 */
        } else {
            /* 低电压 quickstart: 渐进升流 */
            int qs_step = (c->cable_max * 13) / 80;
            qs_step = ((qs_step + 25) / 50) * 50;
            c->current_ma = 500 + qs_step;
            c->ramp_idx = 1;
        }
        write_current(c->fds, c->use_ufcs, c->current_ma);
        return;
    }

    if (c->current_ma >= phase_max)
        return;

    int step;
    if (c->ramp_idx >= 1 && c->ramp_idx <= 4) {
        /* 斜坡阶段: 剩余距离除法 */
        int remaining = c->cable_max - c->current_ma;
        int divisor = (c->ramp_idx == 1) ? 17 : 11;
        step = (remaining + divisor / 2) / divisor;
        step = ((step + 25) / 50) * 50;
        if (step > c->inc_step) step = c->inc_step;
    } else if (c->cfg->adjust_step > 0 && c->ramp_idx >= 5 && c->ramp_idx <= 6) {
        /* 微调过渡: adjust_step (strace 确认 2 步) */
        step = c->cfg->adjust_step;
    } else {
        /* 全速步进: cable_max / 10 */
        step = c->cable_max > 0 ? c->cable_max / 10 : c->inc_step;
    }

    c->current_ma += step;
    if (c->current_ma > phase_max)
        c->current_ma = phase_max;

    write_current(c->fds, c->use_ufcs, c->current_ma);
    c->ramp_idx++;
}

/* CV 恒压阶段: 阶梯降流 */
void exec_cv(LoopCtx *c)
{
    char ts[32], line[256];

    if (c->cv_holding) {
        /* 阶梯已走完, 静默维持。
         * strace 确认 (2026-04-28): CV 阶段有振荡回升
         * (3000↔2950↔3000), vbat 回落时可回到较低阶梯。
         */
        for (int i = 0; i < c->cv_step_idx; i++) {
            if (c->parms.vbat_mv < c->cfg->cv_step_mv[i]) {
                c->current_ma = (i > 0) ? c->cfg->cv_step_ma[i - 1] : c->current_ma;
                c->cv_step_idx = i;
                c->cv_holding = 0;
                write_current(c->fds, c->use_ufcs, c->current_ma);
                return;
            }
        }
        return;
    }

    int dropped = 0;
    for (int i = c->cv_step_idx; i < c->cfg->cv_step_count; i++) {
        if (c->parms.vbat_mv >= c->cfg->cv_step_mv[i]) {
            c->current_ma = c->cfg->cv_step_ma[i];
            c->cv_step_idx = i + 1;
            dropped = 1;
        }
    }

    if (dropped) {
        write_current(c->fds, c->use_ufcs, c->current_ma);
        get_timestamp(ts, sizeof(ts));
        snprintf(line, sizeof(line),
                 "%s ==== CV step-down to %dmA (vbat=%dmV, step=%d) ====\n",
                 ts, c->current_ma, c->parms.vbat_mv, c->cv_step_idx);
        log_write(line);
    }

    /* 所有阶梯走完, 进入静默维持 */
    if (c->cfg->cv_step_count > 0 && c->cv_step_idx >= c->cfg->cv_step_count) {
        c->cv_holding = 1;
        get_timestamp(ts, sizeof(ts));
        snprintf(line, sizeof(line),
                 "%s ==== CV holding at %dmA (vbat=%dmV) ====\n",
                 ts, c->current_ma, c->parms.vbat_mv);
        log_write(line);
    }

    /* 无阶梯配置时, 兜底用 dec_step 线性递减 */
    if (c->cfg->cv_step_count == 0) {
        int cap = c->cfg->cv_max_ma > 0 ? c->cfg->cv_max_ma : c->effective_max;
        if (cap > c->effective_max) cap = c->effective_max;
        if (c->current_ma > cap) {
            int step = c->cfg->dec_step > 0 ? c->cfg->dec_step : 100;
            c->current_ma -= step;
            if (c->current_ma < cap) c->current_ma = cap;
            write_current(c->fds, c->use_ufcs, c->current_ma);
        }
    }
}

/* TC 涓流阶段 */
void exec_tc(LoopCtx *c)
{
    int cap = c->cfg->tc_full_ma > 0 ? c->cfg->tc_full_ma : 500;
    if (cap > c->effective_max) cap = c->effective_max;
    if (c->current_ma > cap) {
        int step = c->cfg->dec_step > 0 ? c->cfg->dec_step : 100;
        c->current_ma -= step;
        if (c->current_ma < cap) c->current_ma = cap;
        write_current(c->fds, c->use_ufcs, c->current_ma);
    }
}

/* DEPOL 去极化阶段 */
void exec_depol(LoopCtx *c)
{
    char ts[32], line[256];

    /* 去极化阶段 (strace 2026-04-28 完整周期确认):
     * 完整序列: 50→-100→500→300→250→50→0→-50→-200→-350→500→300→250→50
     * 两轮脉冲+负值去极化。force_val 确实写入负值。
     */
    int pulse = c->cfg->depol_pulse_ma > 0 ? c->cfg->depol_pulse_ma : 500;
    int neg_step = c->cfg->depol_neg_step > 0 ? c->cfg->depol_neg_step : 150;

    /* Round 1: 初始负值 + 脉冲下降至 0 */
    write_current(c->fds, c->use_ufcs, -(neg_step * 2 / 3));
    usleep(500000);

    write_current(c->fds, c->use_ufcs, pulse);
    usleep(500000);
    write_current(c->fds, c->use_ufcs, 300);
    usleep(500000);
    write_current(c->fds, c->use_ufcs, 250);
    usleep(500000);
    write_current(c->fds, c->use_ufcs, 50);
    usleep(500000);
    write_current(c->fds, c->use_ufcs, 0);
    usleep(500000);

    /* Round 2: 负值递减 + 脉冲下降 */
    int neg = -50;
    for (int i = 0; i < 3; i++) {
        if (!*c->running) break;
        write_current(c->fds, c->use_ufcs, neg);
        usleep(500000);
        neg -= neg_step;
    }

    write_current(c->fds, c->use_ufcs, pulse);
    usleep(500000);
    write_current(c->fds, c->use_ufcs, 300);
    usleep(500000);
    write_current(c->fds, c->use_ufcs, 250);
    usleep(500000);
    write_current(c->fds, c->use_ufcs, 50);
    usleep(500000);

    get_timestamp(ts, sizeof(ts));
    snprintf(line, sizeof(line),
             "%s ==== DEPOL complete, preparing restart ====\n", ts);
    log_write(line);

    c->current_ma = 500;
    c->ramp_idx = 0;
    c->cv_step_idx = 0;
    c->cv_holding = 0;
}
