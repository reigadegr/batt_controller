# payload.elf.no_license 逆向分析报告

> 目标: 开源重写 opbatt_control 充电控制核心
> 更新: 2026-04-28 (完整充电周期 strace + 完整行为对齐)

---

## 一、基本信息

ELF 64-bit ARM aarch64, stripped, NDK r26d, 动态链接 + 静态 OpenSSL。
XOR 字符串混淆，~2877 函数无符号。网络仅 AF_UNIX → /dev/socket/logdw，无 TCP。
深度逆向文档: `tmp/DEEP_REVERSE_ANALYSIS.md`

---

## 二、bcc_parms 字段映射

| 字段 | 名称 | 含义 | 特征 |
|------|------|------|------|
| [0] | fcc | 满电容量 (mAh) | 恒定 5896~10544 |
| [1] | design_cap | 设计容量 | 恒定 5888~10608 |
| [2] | ic_param_a | 库仑计数器 | 1489→492→-1740, 线性递减跟踪已消耗电荷 |
| [3] | param_c | 设计参数 | 恒定 2637 |
| [4] | param_d | 设计参数 | 恒定 2621 |
| [5] | ic_param_b | = [2]+405 | 与 [2] 完美线性相关 |
| [6] | **vbat_mv** | **电池电压 (mV)** | 3223→4526 (满电时 ~4500) |
| [7] | **temp_01c** | **温度 (0.1°C)** | 303→361 (后期低温 ~36°C) |
| [8] | ibat_ma | 电池电流 (mA) | 负值=充电, -1421→-2001 |
| [9] | **thermal_hi** | **温控阈值上界** | 91→21 (不归零，极低值继续充电) |
| [10] | **thermal_lo** | **温控阈值下界** | = [9]-11 |
| [11] | **vbus_mv** | **总线电压 (mV)** | 3205→4514 |
| [12-13] | — | 保留 | 全程 0 |
| [14] | ufcs_max_ma | UFCS 最大电流 | 1000 (后期降低) |
| [15] | ufcs_en | UFCS 使能 | 1 |
| [16-17] | pps_max/pps_en | PPS 参数 | 2/0 |
| [18] | cable_type | 线缆类型 | 0 |

**温控阶梯 (完整充电周期确认):**

| 温度 (°C) | thermal_hi | thermal_lo |
|-----------|------------|------------|
| < 51.4 | 91 | 80 |
| 51.4~51.5 | 85 | 75 |
| ≥ 51.6 | 80 | 70 |
| 满电后期 | 21 | 10 |

注意: thermal_hi **不归零**，满电后保持极低值 (21/10) 继续涓流充电。

---

## 三、Voter 系统

### 3.1 UFCS_CURR Voter 完整列表 (22 个)

| Voter | 说明 | 典型值 | 我们是否解析 |
|-------|------|--------|-------------|
| MAX_VOTER | 最大允许电流 | 14600 | ✅ |
| HIDL_VOTER | HIDL 接口 | 0 | ❌ |
| BAD_SUBBOARD_VOTER | 子板异常 | 0 | ❌ |
| EIS_VOTER | 电化学阻抗 | 0 | ❌ |
| IMP_VOTER | 阻抗限制 | 9100 | ❌ |
| STEP_VOTER | 步进控制 | 9100→8000 | ✅ |
| BATT_TEMP_VOTER | 电池温度 | 14600 | ❌ |
| COOL_DOWN_VOTER | 降温限制 | 15000 | ❌ |
| SALE_MODE_VOTER | 销售模式 | 0 | ❌ |
| BCC_VOTER | BCC 限制 | 0 | ✅ |
| BATT_BAL_VOTER | 电池均衡 | 0 | ❌ |
| IBUS_OVER_VOTER | 总线过流 | 0 | ❌ |
| SLOW_CHG_VOTER | 慢充 | 0 | ❌ |
| CABLE_MAX_VOTER | 线缆限制 | 8000 | ✅ |
| ADAPTER_IMAX_VOTER | 适配器限制 | 9100 | ❌ |
| PLC_VOTER | PLC | 0 | ❌ |
| IC_VOTER | IC 硬件限制 | 13700 | ❌ |
| BATT_SOC_VOTER | SoC 限制 | 0 (本次未激活) | ❌ |
| **LIMIT_FCL_VOTER** | **FCL 限制** | **0→7200** | ✅ |
| PR_VOTER | PR | 0 | ❌ |
| BASE_MAX_VOTER | 基础最大值 | 9100 | ❌ |
| BAD_SUB_BTB_VOTER | 子板 BTB | 0 | ❌ |

### 3.2 LIMIT_FCL_VOTER 行为 (CSV 确认)

- **激活条件**: SoC=16%, temp=48.1°C
- **限制值**: v=7200
- **特性**: 一旦激活**永不关闭**

---

## 四、完整充电周期 strace (2026-04-28 attach 模式)

> 数据: `/data/local/tmp/opbatt_trace/strace_20260428_154917_attach.log` (17788 行, ~20 分钟)
> 方式: strace -p attach 到已运行的 opbatt_control

### 4.1 完整时间线

```
15:49:22  dumpsys battery reset (充电周期结束标志)
15:49:23  RISE 开始: 550 → 每步+50mA → 3500
15:49:51  到达 3500mA 峰值 (不是 8000! thermal_hi=21 限制)
15:49:51~15:53:26  维持 3500 (~3.5 分钟不写 force_val)
15:53:26  3500 → 3000 (-500, 大跳降)
15:53:26~15:53:53  3000 附近振荡 (2950↔3000↔2850↔2900→3000)
15:53:53~16:07:10  维持 3000 (~13 分钟不写 force_val)
16:07:10  3000 → 1300 (-1700, 大跳降)
16:07:10~16:08:58  1300~1550 振荡后继续下降 (850→650→400→250→50→0)
16:09:xx  去极化: 50→500→300→250→50→0 (写 0!)
16:09:xx  第二轮: 500→300→250→50→1000 (+950 跳升)
16:09:xx  新 RISE: 1000 → +50mA/步 → 1850
16:10:31  SoC=100%, bcc_parms thermal_hi=0, force_val=1000
```

### 4.2 force_val 完整变化序列 (133 次变化)

**第一阶段: +50mA 线性 RISE (无 quickstart!)**
```
550 → 600 → 650 → ... → 3500 (59 步, +50mA/步, ~28 秒)
```

**第二阶段: CV 振荡降流**
```
3500 → 3000 (-500)
3000 ↔ 2950 ↔ 3000 ↔ 2900 ↔ 2850 ↔ 2800 ↔ 2850 ↔ 2900 ↔ 2950 ↔ 3000
```

**第三阶段: 大跳降 + 振荡**
```
3000 → 1300 (-1700)
1300 ↔ 1350 ↔ 1400 ↔ 1550 ↔ 1500 ↔ 1450 ↔ 1500 ↔ 1450 ↔ 1400 ↔ 1350 ↔ 1300 ↔ 1250 ↔ 1200 ↔ 1250 ↔ 1200 ↔ 1150 ↔ 1100 ↔ 1050 ↔ 1000 ↔ 950 ↔ 900 ↔ 850
```

**第四阶段: 极低电流 + 去极化**
```
850 → 650 (-200) → 600 → 400 (-200) → 300 → 250 → 50 (-200)
50 → 500 (+450, 去极化!) → 300 → 250 → 50 → 0
500 → 300 → 250 → 50 → 0 (第二轮去极化)
```

**第五阶段: 新周期重启**
```
500 → 300 → 250 → 50 → 1000 (+950)
1000 → 1050 → 1100 → ... → 1850 (+50mA/步)
1850 → 1000 (-850, 大跳降)
```

### 4.3 关键 bcc_parms 状态

| 时间 | thermal_hi | thermal_lo | vbat_mv | temp_01c | ufcs_max |
|------|-----------|-----------|---------|----------|----------|
| 15:49:23 (RISE 起始) | 21 | 10 | 4511 | 303 | 1000 |
| 15:49:51 (峰值 3500) | 21 | 10 | 4522 | 303 | 1000 |
| 16:10:31 (SoC=100%) | 0 | 0 | 4450 | 303 | 0 |

### 4.4 颠覆性发现

| # | 发现 | 之前的认知 | 纠正后 |
|---|------|-----------|--------|
| 1 | **重启 RISE 是 +50mA 线性爬升** | quickstart 三段式 | 两种模式: 首次充电=quickstart, 重启=+50mA 线性 |
| 2 | **峰值受 thermal_hi 限制** | 固定到达 cable_max (8000) | thermal_hi=21 → 峰值只有 3500 |
| 3 | **thermal_hi 不归零** | thermal_hi=0 是周期结束标志 | thermal_hi=21 仍可充电，SoC=100% 时才归零 |
| 4 | **force_val 可以写 0** | 去极化由内核处理 | 用户态程序确实写 0 和极低值 (50mA) |
| 5 | **CV 阶段有振荡** | CV 只减不增 | 3000↔2950↔3000 振荡，电流可回升 |
| 6 | **dumpsys reset 后无 mmi toggle** | dumpsys reset + mmi 0→1 | 只有 dumpsys reset，无 mmi 写入 |
| 7 | **CV 降流是大跳降** | 阶梯式递减 | 3500→3000(-500), 3000→1300(-1700) 一步到位 |

---

## 五、充电阶段状态机 (修正后)

```
                    首次充电
USB 插入 → IDLE ─────────→ RISE (quickstart 三段式: 500→quickstart→ramp→full_speed)
                                │
                                │ vbat >= cv_vol_mv
                                ▼
                              CV (阶梯降流: 大跳降 + 振荡维持)
                                │
                                │ force_val 降到极低
                                ▼
                              TC (极低电流: 850→650→400→250→50)
                                │
                                │ force_val = 0
                                ▼
                              DEPOL (去极化: 0→500→300→250→50→0, 可多轮)
                                │
                                │ 去极化完成
                                ▼
                              FULL (force_val=1000, SoC=100%)
                                │
                                │ dumpsys battery reset
                                ▼
                            重新开始 → RISE (+50mA 线性爬升, 无 quickstart)
                                │
                                │ thermal_hi 很低, 峰值受限
                                ▼
                              CV → TC → DEPOL → FULL → reset → ...
```

---

## 六、已有功能与缺失对照

### 已确认/已实现 ✅

| 功能 | 状态 |
|------|------|
| sysfs/procfs 12 路径读写 | ✅ |
| bcc_parms 19 字段解析 | ✅ |
| UFCS voter 解析 (4 个) | ✅ |
| dumpsys fork+exec 重置序列 | ✅ |
| UFCS/PPS 协议选择 | ✅ |
| 温控电流偏移 (temp_01c 驱动) | ✅ |
| 配置文件 28 key 解析 | ✅ |
| 电池型号表 15 个 | ✅ |
| CLI 12 选项 | ✅ |
| 充电重置计数限制 | ✅ |
| 3 线程架构 (USB 检测 2s / 充电控制 / 日志采集 5s) | ✅ |
| 日志写入 /data/opbatt/battchg.log | ✅ |
| RISE 三段式步长算法 (首次充电) | ✅ |
| CV 阶梯降流 (vbat 阈值驱动) | ✅ |
| CV 维持模式 | ✅ |
| thermal_hi 阶梯限流 (×100→mA) | ✅ |
| battery_log_content 解析 | ✅ |
| USB 拔出后 fd 生命周期管理 | ✅ |
| 11 个 bug 修复 (4 agent 审查) | ✅ |

### 需要修正/新增 ❌

| # | 功能 | 当前状态 | 需要的改动 |
|---|------|---------|-----------|
| 1 | **重启 RISE (+50mA 线性)** | 只有 quickstart 三段式 | 新增 PHASE_RESTART_RISE，+50mA/步 |
| 2 | **去极化阶段** | 不存在 | 新增 PHASE_DEPOL，写 0→500→...→0 |
| 3 | **CV 振荡/电流回升** | CV 只减不增 | CV 阶段允许 vbat 回落时回升 |
| 4 | **充电周期结束检测** | thermal_hi==0 触发 | 改为 thermal_hi<=极低阈值 + dumpsys reset |
| 5 | **dumpsys reset 序列** | 包含 mmi_charging_enable 0→1 | 去掉 mmi toggle，只做 dumpsys reset |
| 6 | **effective_max 计算** | 不考虑 ufcs_max_ma 变化 | thermal_hi 极低时 ufcs_max 也变小 |

### 仍需更多数据 ⚠️

| 功能 | 说明 |
|------|------|
| quickstart 动态系数 | 公式 `cable_max * X / 80`, X=9~21, 可能与温度相关 |
| adjust_step 触发条件 | 可能是剩余距离驱动而非固定 ramp_idx |
| BATT_SOC_VOTER | 本次数据 SoC=100% 时 thermal_hi=0, 未见 BATT_SOC_VOTER 激活 |

---

## 七、ELF 静态分析发现

| 地址 | 发现 | 说明 |
|------|------|------|
| `0xd6c4c` | `round_to_nearest(value, divisor)` | 步长对齐到 50mA |
| `0xd6c94` | 主循环函数入口 | 完整 prologue + xor 常量初始化 |
| `0xd6be4` | `MOV #500` → `[x24, #0x6d8]` | 结构体中 current_ma 字段写入 |
| `0xd5d08` | `sub w8, w8, #0x190` (减 400) | 可能是 CV 降流逻辑 |
| `0xd5c94` | `MOV #500` + `blr x8` | 可能是 FULL 阶段写 500mA |

config 值硬编码在二进制中，字符串全部 XOR 混淆。

---

## 八、待实现功能（接手 AI 任务清单）

> 数据来源: `strace_20260428_154917_attach.log` (完整充电周期, 17788 行)
> 当前源码: `src/charging.c`, `src/charging.h`, `src/config.h`, `src/config.c`, `src/main.c`

### 8.1 新增充电阶段枚举 (`charging.h`)

在 `ChargePhase` 枚举中新增两个阶段：

```c
typedef enum {
    PHASE_IDLE,
    PHASE_RISE,           /* 首次上升: quickstart 三段式 (已有) */
    PHASE_RESTART_RISE,   /* 重启上升: +50mA 线性爬升 (新增) */
    PHASE_CV,             /* 恒压阶段: 阶梯降流 + 振荡 (已有, 需改) */
    PHASE_TC,             /* 涓流阶段: 极低电流 (已有, 需改) */
    PHASE_DEPOL,          /* 去极化: force_val 写 0 和极低值 (新增) */
    PHASE_FULL,           /* 满电 (已有) */
} ChargePhase;
```

### 8.2 新增配置字段 (`config.h` + `config.c`)

在 `BattConfig` 结构体中新增：

```c
int restart_rise_step;   /* 重启 RISE 步长 (mA), 默认 50 */
int depol_pulse_ma;      /* 去极化脉冲电流 (mA), 默认 500 */
int depol_zero_ma;       /* 去极化零电流阈值 (mA), 默认 0 */
```

config.c 中新增解析 + config_dump 输出 + main.c load_config 设置默认值。

### 8.3 修复 dumpsys_reset (`charging.c`)

**当前**: dumpsys reset + mmi_charging_enable 0→1 + sleep 2+1+8 秒
**应改为**: 只做 `run_dumpsys("reset", NULL, NULL)`，去掉 mmi toggle 和 sleep

strace 确认: 充电周期结束后**没有** mmi_charging_enable 写入，只有 dumpsys battery reset。

### 8.4 修复充电周期结束检测 (`charging.c`)

**当前**: `thermal_hi == 0` 触发 dumpsys reset
**应改为**: `thermal_hi <= 20 && current_ma <= 100` 触发 dumpsys reset

strace 确认: thermal_hi 在满电后期是 21（不是 0），SoC=100% 时才到 0。
周期结束标志是 force_val 已降到极低（≤100mA）+ thermal_hi 很低（≤20）。

触发后: dumpsys reset → phase = PHASE_RESTART_RISE, current_ma = 500。

### 8.5 新增 PHASE_RESTART_RISE 处理 (`charging.c`)

充电结束后重启的 RISE 阶段，+50mA 线性爬升，**没有 quickstart**。

```c
case PHASE_RESTART_RISE: {
    int step = cfg->restart_rise_step > 0 ? cfg->restart_rise_step : 50;
    int phase_max = effective_max;
    if (current_ma < phase_max) {
        current_ma += step;
        if (current_ma > phase_max) current_ma = phase_max;
        write_current(fds, use_ufcs, current_ma);
    }
    break;
}
```

转入条件: dumpsys reset 后
转出条件: vbat >= cv_vol_mv → PHASE_CV

### 8.6 新增 PHASE_DEPOL 处理 (`charging.c`)

去极化阶段: force_val 写脉冲值然后逐步降到 0。

strace 观测到: `50→500→300→250→50→0` 然后 `500→300→250→50→0`（两轮）

```c
case PHASE_DEPOL: {
    int pulse = cfg->depol_pulse_ma > 0 ? cfg->depol_pulse_ma : 500;
    /* 脉冲 */
    write_current(fds, use_ufcs, pulse);
    usleep(500000);
    /* 逐步降到 0 */
    for (int v = 200; v >= 0; v -= 50) {
        if (!*running) break;
        write_current(fds, use_ufcs, v);
        usleep(500000);
    }
    write_current(fds, use_ufcs, 0);
    /* 去极化完成 → 重启 RISE */
    current_ma = 500;
    ramp_idx = 0;
    phase = PHASE_RESTART_RISE;
    break;
}
```

转入条件: PHASE_TC 中 force_val 降到 ≤ 50mA
转出条件: 写完 0 后直接转 PHASE_RESTART_RISE

### 8.7 修复 CV 阶段 — 允许电流回升 (`charging.c`)

strace 观测到 CV 振荡: 3000↔2950↔3000 (+50mA 回升)。

当前 CV 阶梯走完后进入 cv_holding 静默模式（只减不增）。
需增加: 如果 vbat 回落到低于当前阶梯阈值，回到较低阶梯（电流回升）。

```c
if (cv_holding) {
    /* 检查 vbat 回落 — 允许电流回升 */
    for (int i = 0; i < cv_step_idx; i++) {
        if (parms.vbat_mv < cfg->cv_step_mv[i]) {
            current_ma = (i > 0) ? cfg->cv_step_ma[i - 1] : current_ma;
            cv_step_idx = i;
            cv_holding = 0;
            write_current(fds, use_ufcs, current_ma);
            break;
        }
    }
    break;
}
```

### 8.8 更新 phase_name 和 next_phase (`charging.c`)

`phase_name()` 新增:
```c
case PHASE_RESTART_RISE: return "RESTART_RISE";
case PHASE_DEPOL:        return "DEPOL";
```

`next_phase()` 新增转换:
- PHASE_FULL → PHASE_RESTART_RISE (dumpsys reset 后)
- PHASE_RESTART_RISE → PHASE_CV (vbat >= cv_vol_mv)
- PHASE_TC → PHASE_DEPOL (current_ma <= 50)
- PHASE_DEPOL → PHASE_RESTART_RISE (去极化完成)

### 8.9 验证

改完后执行:
```bash
cd /data/data/com.termux/files/home/batt/src && make clean && make
```

确保编译通过，然后可以 strace 对比 original 和 mine 的完整充电周期行为。
