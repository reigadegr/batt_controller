#include "charging.h"
#include "charging_internal.h"

#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

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

static int extract_voter_int(const char *status, const char *tag)
{
    const char *p = strstr(status, tag);
    if (!p) return 0;
    p = strstr(p, "v=");
    return p ? atoi(p + 2) : 0;
}

int charging_parse_ufcs_voters(const char *status, UfcsVoters *voters)
{
    memset(voters, 0, sizeof(*voters));
    voters->max_ma       = extract_voter_int(status, "MAX_VOTER:");
    voters->cable_max_ma = extract_voter_int(status, "CABLE_MAX_VOTER:");
    voters->step_ma      = extract_voter_int(status, "STEP_VOTER:");
    voters->bcc_ma       = extract_voter_int(status, "BCC_VOTER:");
    return 0;
}

void read_voters_3x(char *buf, int bufsz, UfcsVoters *voters)
{
    for (int i = 0; i < 3; i++) {
        if (sysfs_read_ufcs_voters(buf, bufsz) > 0)
            charging_parse_ufcs_voters(buf, voters);
    }
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

void charging_dumpsys_reset(SysfsFds *fds)
{
    (void)fds;
    run_dumpsys("reset", NULL, NULL);
}

int choose_protocol(const BattConfig *cfg, const BccParms *parms)
{
    if (cfg->cable_override)
        return (cfg->cable_override > 0) ? 1 : 0;

    if (parms->ufcs_en && parms->ufcs_max_ma > 0)
        return 1;

    if (parms->pps_en && parms->pps_max_ma > 0)
        return 0;

    return 1;
}

int get_temp_curr_offset(const BattConfig *cfg, int temp_01c)
{
    int temp = temp_01c / 10;
    for (int i = cfg->temp_range_count - 1; i >= 0; i--) {
        if (temp >= cfg->temp_range[i]) {
            if (i < cfg->temp_curr_offset_count)
                return cfg->temp_curr_offset[i];
            return 0;
        }
    }
    return 0;
}

void write_current(SysfsFds *fds, int use_ufcs, int ma)
{
    if (use_ufcs) {
        sysfs_write_int(fds->ufcs_force_val, ma);
        sysfs_write_str(fds->ufcs_force_active, "1");
    } else {
        sysfs_write_int(fds->pps_force_val, ma);
        sysfs_write_str(fds->pps_force_active, "1");
    }
}

int clamp_max_ma(int cfg_max, int proto_max, int cable_max)
{
    int m = cfg_max;
    if (proto_max > 0 && proto_max < m) m = proto_max;
    if (cable_max > 0 && cable_max < m) m = cable_max;
    return m;
}

const char *phase_name(ChargePhase ph)
{
    switch (ph) {
    case PHASE_IDLE:         return "IDLE";
    case PHASE_RISE:         return "RISE";
    case PHASE_RESTART_RISE: return "RESTART_RISE";
    case PHASE_CV:           return "CV";
    case PHASE_TC:           return "TC";
    case PHASE_DEPOL:        return "DEPOL";
    case PHASE_FULL:         return "FULL";
    }
    return "?";
}

ChargePhase next_phase(ChargePhase cur, const BattConfig *cfg,
                       const BccParms *parms, int soc, int current_ma)
{
    int vbat = parms->vbat_mv;
    int ibat = parms->ibat_ma < 0 ? -parms->ibat_ma : parms->ibat_ma;

    if (parms->thermal_hi == 0 && cur != PHASE_DEPOL && cur != PHASE_RESTART_RISE)
        return PHASE_IDLE;

    switch (cur) {
    case PHASE_IDLE:
        return PHASE_RISE;

    case PHASE_RISE:
        if (cfg->cv_vol_mv > 0 && vbat >= cfg->cv_vol_mv)
            return PHASE_CV;
        return PHASE_RISE;

    case PHASE_RESTART_RISE:
        if (cfg->cv_vol_mv > 0 && vbat >= cfg->cv_vol_mv)
            return PHASE_CV;
        return PHASE_RESTART_RISE;

    case PHASE_CV:
        if (cfg->tc_thr_soc > 0 && soc >= cfg->tc_thr_soc)
            return PHASE_TC;
        if (cfg->tc_vol_thr_mv > 0 && vbat >= cfg->tc_vol_thr_mv)
            return PHASE_TC;
        return PHASE_CV;

    case PHASE_TC:
        if (current_ma <= 100)
            return PHASE_DEPOL;
        if (cfg->tc_full_ma > 0 && cfg->tc_vol_full_mv > 0 &&
            ibat <= cfg->tc_full_ma && vbat >= cfg->tc_vol_full_mv)
            return PHASE_FULL;
        return PHASE_TC;

    case PHASE_DEPOL:
        return PHASE_RESTART_RISE;

    case PHASE_FULL:
        return PHASE_FULL;
    }

    return cur;
}
