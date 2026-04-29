#ifndef CHARGING_INTERNAL_H
#define CHARGING_INTERNAL_H

/*
 * charging 内部共享定义
 * 仅被 charging.c / charging_phase.c / charging_loop.c 包含
 */

#include "charging.h"
#include "log.h"

/* 充电主循环上下文: 避免在函数间传递过多参数 */
typedef struct {
    /* 配置和硬件 */
    SysfsFds *fds;
    const BattConfig *cfg;
    volatile int *running;

    /* 运行时状态 */
    int current_ma;
    int max_ma;
    int cable_max;
    int use_ufcs;
    int inc_step;
    int ramp_idx;
    int restart_count;
    int in_charge_cycle;
    int soc;
    ChargePhase phase;
    int cv_step_idx;
    int cv_holding;
    int effective_max;
    int rise_max_reached;  /* RISE 阶段到达 phase_max 后置 1, 静默维持 */

    /* 临时数据 */
    BccParms parms;
    UfcsVoters voters;
} LoopCtx;

/* ---- charging.c 中的辅助函数 ---- */

const char *phase_name(ChargePhase ph);
ChargePhase next_phase(ChargePhase cur, const BattConfig *cfg,
                       const BccParms *parms, int soc, int current_ma);
int choose_protocol(const BattConfig *cfg, const BccParms *parms);
int get_temp_curr_offset(const BattConfig *cfg, int temp_01c);
void write_current(SysfsFds *fds, int use_ufcs, int ma);
int clamp_max_ma(int cfg_max, int proto_max, int cable_max);
void read_voters_3x(char *buf, int bufsz, UfcsVoters *voters);

/* ---- charging_phase.c 中的阶段处理函数 ---- */

/* 充电周期结束处理 (含 dumpsys reset + 协议切换)
 * 返回 1 表示执行了周期结束逻辑 (调用方应 continue) */
int handle_cycle_end(LoopCtx *c, char *log_buf);

/* 计算 effective_max (温控 + thermal_hi + STEP_VOTER 限流) */
void calc_effective_max(LoopCtx *c);

/* 各阶段执行逻辑 */
void exec_rise(LoopCtx *c);
void exec_cv(LoopCtx *c);
void exec_tc(LoopCtx *c);
void exec_depol(LoopCtx *c);

#endif /* CHARGING_INTERNAL_H */
