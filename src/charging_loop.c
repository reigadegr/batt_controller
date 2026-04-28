#include "charging_internal.h"
#include "sysfs.h"

#include <stdio.h>
#include <unistd.h>

void charging_loop(SysfsFds *fds, const BattConfig *cfg, volatile int *running)
{
    char log_buf[1024];
    char ts[32];
    char line[256];
    LoopCtx c = {
        .fds = fds, .cfg = cfg, .running = running,
        .current_ma = 500, .max_ma = cfg->ufcs_max,
        .use_ufcs = 1,
        .phase = PHASE_IDLE,
    };

    /* ---- 阶段 1: 读取电池状态日志 ---- */
    sysfs_read_battery_log(log_buf, sizeof(log_buf));

    /* ---- 阶段 2: 重置 votable ---- */
    sysfs_reset_votables();

    read_voters_3x(log_buf, sizeof(log_buf), &c.voters);

    c.cable_max = c.voters.cable_max_ma;
    c.max_ma = clamp_max_ma(c.max_ma, 0, c.cable_max);

    /* strace 确认: inc_step = effective_max / 10 */
    c.inc_step = c.max_ma > 0 ? c.max_ma / 10 : cfg->inc_step;

    /* ---- 阶段 4: 输出充电信息日志 ---- */
    get_timestamp(ts, sizeof(ts));
    snprintf(line, sizeof(line),
             "%s UFCS_CHG: AdpMAXma=%dma, CableMAXma=%dma, Maxallow=%dma, Maxset=%dma, OP_chg=1\n",
             ts, c.voters.max_ma, c.voters.cable_max_ma, c.voters.max_ma, c.max_ma);
    log_write(line);
    snprintf(line, sizeof(line),
             "%s ==== Charger type UFCS, set max current %dma ====\n", ts, c.max_ma);
    log_write(line);

    /* ---- 阶段 5: 充电控制主循环 ---- */
    while (*running) {
        /* 读取 bcc_parms */
        if (sysfs_read_bcc_parms(log_buf, sizeof(log_buf)) > 0) {
            charging_parse_bcc_parms(log_buf, &c.parms);
        } else {
            usleep(500000);
            continue;
        }

        /* 读取 SoC */
        c.soc = sysfs_read_int(fds->chip_soc);

        /* 充电周期结束检测 */
        if (handle_cycle_end(&c, log_buf))
            continue;

        /* 温控: 根据温度/thermal_hi/STEP_VOTER 调整最大电流 */
        calc_effective_max(&c);

        /* ---- 充电阶段状态机 ---- */
        ChargePhase new_phase = next_phase(c.phase, cfg, &c.parms,
                                           c.soc, c.current_ma);
        if (new_phase != c.phase) {
            get_timestamp(ts, sizeof(ts));
            snprintf(line, sizeof(line),
                     "%s ==== Phase %s -> %s (vbat=%dmV, soc=%d%%) ====\n",
                     ts, phase_name(c.phase), phase_name(new_phase),
                     c.parms.vbat_mv, c.soc);
            log_write(line);
            c.phase = new_phase;
        }

        switch (c.phase) {
        case PHASE_IDLE:
            break;
        case PHASE_RISE:
        case PHASE_RESTART_RISE:
            exec_rise(&c);
            break;
        case PHASE_CV:
            exec_cv(&c);
            break;
        case PHASE_TC:
            exec_tc(&c);
            break;
        case PHASE_DEPOL:
            exec_depol(&c);
            break;
        case PHASE_FULL:
            /* strace 确认: FULL 阶段持续写 1000mA (非 500) */
            write_current(fds, c.use_ufcs, 1000);
            break;
        }

        /* 满电判断: batt_full_thr_mv 额外检查 */
        if (c.phase != PHASE_FULL && cfg->batt_full_thr_mv > 0 &&
            c.parms.vbat_mv >= cfg->batt_full_thr_mv) {
            get_timestamp(ts, sizeof(ts));
            snprintf(line, sizeof(line),
                     "%s ==== Battery full: vbat=%dmV >= batt_full_thr_mv=%dmV ====\n",
                     ts, c.parms.vbat_mv, cfg->batt_full_thr_mv);
            log_write(line);
            c.phase = PHASE_FULL;
        }

        usleep(500000);
    }
}
