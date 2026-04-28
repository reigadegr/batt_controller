#include "config.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/*
 * 解析逗号分隔的整数数组
 * "42,43,44,45,46" → {42,43,44,45,46}, count=5
 */
static int parse_int_array(const char *val, int *arr, int max_count)
{
    int count = 0;
    const char *p = val;

    while (*p && count < max_count) {
        arr[count++] = (int)strtol(p, (char **)&p, 10);
        if (*p == ',') p++;
    }
    return count;
}

/*
 * 从 key=value 行中提取 value
 * 返回 value 指针，未找到返回 NULL
 */
static const char *extract_value(const char *line, const char *key)
{
    size_t klen = strlen(key);
    if (strncmp(line, key, klen) != 0) return NULL;
    /* 精确键匹配: key 后必须紧跟 '=' 或行尾 */
    if (line[klen] != '=' && line[klen] != '\0') return NULL;
    if (line[klen] == '\0') return NULL;
    return line + klen + 1;
}

int config_parse(const char *path, BattConfig *cfg)
{
    FILE *fp = fopen(path, "r");
    if (!fp) return -1;

    memset(cfg, 0, sizeof(*cfg));

    /* 默认值 */
    cfg->enabled = 1;
    cfg->adjust_step = 50;
    cfg->inc_step = 100;
    cfg->loop_interval_ms = 2000;
    cfg->restart_rise_step = 50;
    cfg->depol_pulse_ma = 500;
    cfg->depol_zero_ma = 0;

    /* strace 确认: SoC<20 → 450ms, SoC≥20 → 650ms */
    cfg->ufcs_soc_mon[0] = 20;
    cfg->ufcs_soc_mon[1] = 60;
    cfg->ufcs_interval_ms[0] = 450;
    cfg->ufcs_interval_ms[1] = 650;

    char line[256];
    while (fgets(line, sizeof(line), fp)) {
        /* 去掉换行 */
        size_t len = strlen(line);
        while (len > 0 && (line[len-1] == '\n' || line[len-1] == '\r'))
            line[--len] = '\0';

        const char *v;

        if ((v = extract_value(line, "temp_range"))) {
            cfg->temp_range_count = parse_int_array(v, cfg->temp_range, TEMP_RANGE_MAX);
        } else if ((v = extract_value(line, "temp_curr_offset"))) {
            cfg->temp_curr_offset_count = parse_int_array(v, cfg->temp_curr_offset, TEMP_RANGE_MAX);
        } else if ((v = extract_value(line, "adjust_step"))) {
            cfg->adjust_step = atoi(v);
        } else if ((v = extract_value(line, "inc_step"))) {
            cfg->inc_step = atoi(v);
        } else if ((v = extract_value(line, "dec_step"))) {
            cfg->dec_step = atoi(v);
        } else if ((v = extract_value(line, "max_ufcs_chg_reset_cc"))) {
            cfg->max_ufcs_chg_reset_cc = atoi(v);
        } else if ((v = extract_value(line, "ufcs_reset_delay"))) {
            cfg->ufcs_reset_delay = atoi(v);
        } else if ((v = extract_value(line, "ufcs_max"))) {
            cfg->ufcs_max = atoi(v);
        } else if ((v = extract_value(line, "pps_max"))) {
            cfg->pps_max = atoi(v);
        } else if ((v = extract_value(line, "cable_override"))) {
            cfg->cable_override = atoi(v);
        } else if ((v = extract_value(line, "ufcs_soc_mon"))) {
            parse_int_array(v, cfg->ufcs_soc_mon, 2);
        } else if ((v = extract_value(line, "ufcs_interval_ms"))) {
            parse_int_array(v, cfg->ufcs_interval_ms, 2);
        } else if ((v = extract_value(line, "pps_soc_mon"))) {
            parse_int_array(v, cfg->pps_soc_mon, 2);
        } else if ((v = extract_value(line, "pps_interval_ms"))) {
            parse_int_array(v, cfg->pps_interval_ms, 2);
        } else if ((v = extract_value(line, "loop_interval_ms"))) {
            cfg->loop_interval_ms = atoi(v);
        } else if ((v = extract_value(line, "batt_vol_thr"))) {
            parse_int_array(v, cfg->batt_vol_thr, 2);
        } else if ((v = extract_value(line, "batt_vol_soc"))) {
            parse_int_array(v, cfg->batt_vol_soc, 2);
        } else if ((v = extract_value(line, "batt_con_soc"))) {
            cfg->batt_con_soc = atoi(v);
        } else if ((v = extract_value(line, "rise_quickstep_thr_mv"))) {
            cfg->rise_quickstep_thr_mv = atoi(v);
        } else if ((v = extract_value(line, "rise_wait_thr_mv"))) {
            cfg->rise_wait_thr_mv = atoi(v);
        } else if ((v = extract_value(line, "cv_vol_mv"))) {
            cfg->cv_vol_mv = atoi(v);
        } else if ((v = extract_value(line, "cv_max_ma"))) {
            cfg->cv_max_ma = atoi(v);
        } else if ((v = extract_value(line, "cv_step_mv"))) {
            cfg->cv_step_count = parse_int_array(v, cfg->cv_step_mv, CV_STEP_MAX);
        } else if ((v = extract_value(line, "cv_step_ma"))) {
            int ma_count = parse_int_array(v, cfg->cv_step_ma, CV_STEP_MAX);
            if (cfg->cv_step_count > 0 && ma_count < cfg->cv_step_count)
                cfg->cv_step_count = ma_count;
        } else if ((v = extract_value(line, "tc_vol_thr_mv"))) {
            cfg->tc_vol_thr_mv = atoi(v);
        } else if ((v = extract_value(line, "tc_thr_soc"))) {
            cfg->tc_thr_soc = atoi(v);
        } else if ((v = extract_value(line, "tc_full_ma"))) {
            cfg->tc_full_ma = atoi(v);
        } else if ((v = extract_value(line, "tc_vol_full_mv"))) {
            cfg->tc_vol_full_mv = atoi(v);
        } else if ((v = extract_value(line, "curr_inc_wait_cycles"))) {
            cfg->curr_inc_wait_cycles = atoi(v);
        } else if ((v = extract_value(line, "batt_full_thr_mv"))) {
            cfg->batt_full_thr_mv = atoi(v);
        } else if ((v = extract_value(line, "restart_rise_step"))) {
            cfg->restart_rise_step = atoi(v);
        } else if ((v = extract_value(line, "depol_pulse_ma"))) {
            cfg->depol_pulse_ma = atoi(v);
        } else if ((v = extract_value(line, "depol_zero_ma"))) {
            cfg->depol_zero_ma = atoi(v);
        } else if ((v = extract_value(line, "enabled"))) {
            cfg->enabled = atoi(v);
        }
    }

    fclose(fp);
    return 0;
}

/*
 * 打印配置 — 复现原始二进制的 stdout 输出格式
 * strace 确认的输出顺序：
 *   === Initialize the CV configuration v1.8.9-OBF ===
 *   temp_range: 42 43 44 45 46
 *   temp_curr_offset: 800 1200 1800 2500 4500
 *   adjust_step: 50
 *   inc_step: 100
 *   dec_step: 100
 *   batt_vol_thr: 4559 4559
 *   batt_vol_soc: 75 85
 *   batt_con_soc: 94
 *   max_ufcs_chg_reset_cc: 1
 *   ufcs_max: 9100
 *   pps_max: 5000
 *   ufcs_soc_mon: 20 60
 *   ufcs_interval_ms: 650 400
 *   pps_soc_mon: 20 68
 *   pps_interval_ms: 650 400
 *   loop_interval_ms: 2000
 *   ============================
 */
void config_dump(const BattConfig *cfg)
{
    printf("=== Initialize the CV configuration v1.8.9-OBF ===\n");

    printf("temp_range:");
    for (int i = 0; i < cfg->temp_range_count; i++)
        printf(" %d", cfg->temp_range[i]);
    printf("\n");

    printf("temp_curr_offset:");
    for (int i = 0; i < cfg->temp_curr_offset_count; i++)
        printf(" %d", cfg->temp_curr_offset[i]);
    printf("\n");

    printf("adjust_step: %d\n", cfg->adjust_step);
    printf("inc_step: %d\n", cfg->inc_step);
    printf("dec_step: %d\n", cfg->dec_step);

    printf("batt_vol_thr: %d %d\n", cfg->batt_vol_thr[0], cfg->batt_vol_thr[1]);
    printf("batt_vol_soc: %d %d\n", cfg->batt_vol_soc[0], cfg->batt_vol_soc[1]);
    printf("batt_con_soc: %d\n", cfg->batt_con_soc);

    printf("max_ufcs_chg_reset_cc: %d\n", cfg->max_ufcs_chg_reset_cc);
    printf("ufcs_max: %d\n", cfg->ufcs_max);
    printf("pps_max: %d\n", cfg->pps_max);

    printf("ufcs_soc_mon: %d %d\n", cfg->ufcs_soc_mon[0], cfg->ufcs_soc_mon[1]);
    printf("ufcs_interval_ms: %d %d\n", cfg->ufcs_interval_ms[0], cfg->ufcs_interval_ms[1]);
    printf("pps_soc_mon: %d %d\n", cfg->pps_soc_mon[0], cfg->pps_soc_mon[1]);
    printf("pps_interval_ms: %d %d\n", cfg->pps_interval_ms[0], cfg->pps_interval_ms[1]);

    printf("restart_rise_step: %d\n", cfg->restart_rise_step);
    printf("depol_pulse_ma: %d\n", cfg->depol_pulse_ma);
    printf("depol_zero_ma: %d\n", cfg->depol_zero_ma);

    printf("loop_interval_ms: %d\n", cfg->loop_interval_ms);
    printf("============================\n");
}
