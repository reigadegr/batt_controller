# payload.elf.no_license 逆向分析报告

> 目标: 开源重写 opbatt_control 充电控制核心
> 更新: 2026-04-27 (bcc_parms 映射修正 + strace 深度分析)

---

## 一、基本信息

ELF 64-bit ARM aarch64, stripped, NDK r26d, 动态链接 (libc/libdl/liblog/libm) + 静态 OpenSSL。
XOR 字符串混淆，仅 sysfs 路径和 `ro.arch` 明文保留。~2877 函数无符号。

---

## 二、已确认功能 ✅

sysfs/procfs 10 路径读写 | bcc_parms 19 字段解析 | UFCS voter 解析 |
dumpsys fork+exec 重置序列 | mmi_charging_enable 0→1 重置 |
UFCS/PPS 协议选择 | 温控电流偏移 | 配置文件 28 key 解析 |
电池型号表 15 个 | CLI 12 选项 | 充电重置计数限制 |
UFCS 重置延迟 | 满电判断 4570mV | 3 线程架构 |
日志写入 /data/opbatt/battchg.log | USB 在线轮询 2s | 电池日志轮询 5s |
bcc_parms 字段映射修正 (strace 3130 次读取验证) |
11 个 bug 修复 (4 agent 审查完成)

---

## 三、strace 实测充电时序

> 数据: `tmp/strace_log/strace_20260427_162732.log` (21171 行, 26 分钟, PJZ110, UFCS)

```
RISE #1: 500→1750→3550→3650→5400→7550→8000 (步长不均, 2s间隔, 500和1750间隔0ms)
reset:   votable=0 → dumpsys set ac/status → sleep(2) → mmi=0 → sleep(1) → mmi=1 → sleep(8) → dumpsys reset
RISE #2: 1300→2100→2900→...→8000 (步长恒定800, ~480ms间隔)
CV 衰减: 8000→6400→5300→4300→4250→...→1000 (初始大步长1600/1100/1000, 后转50mA小步)
放电:    1000→...→0→-150→-300→-450→-600→-550→...→-300 (负电流, 代码无此逻辑)
TC 保持: 1000mA × 28次 (~8秒)
新 RISE: 1050→1100→1150→1200→1250→1300 (2s间隔, 步长50)
```

网络: 仅 AF_UNIX → /dev/socket/logdw，无 TCP。

---

## 四、已修正的问题 ✅

| 问题 | 修正 |
|------|------|
| bcc_parms 字段映射错误 | ✅ fields[0]/[1] 非 vbat/ibat，[8] 非 power_mw；已按 strace 修正 |
| dumpsys fork+exec | ✅ 原误判 dlopen |
| mmi_charging_enable 重置 | ✅ 补全 0→sleep1→1→sleep8 |
| dumpsys reset 时序 | ✅ 移到 sleep(8) 之后 |
| 日志写入 | ✅ 添加 /data/opbatt/battchg.log |
| ufcs_status 持久 fd | ✅ 改用临时打开 |
| SoC 轮询边界 | ✅ `<=` 改为 `<` |
| CV/TC 衰减步长 | ✅ dec_step(100) 改为 adjust_step(50) |
| config key 前缀匹配 | ✅ strncmp 改为 extract_value 含 `=` 检查 |
| 其他 2 个 bug | ✅ (详见 tmp/agent1_review.md) |

---

## 五、bcc_parms 字段映射 (修正后)

> strace 3130 次读取验证，详见 `tmp/STRACE_VS_SRC_GAP_ANALYSIS.md`

| 字段 | 名称 | 含义 | 特征 |
|------|------|------|------|
| [0] | fcc | 满电容量 (mAh) | 恒定 5896 |
| [1] | design_cap | 设计容量 | 恒定 5888 |
| [2] | ic_param_a | 充电IC参数A | 1175→-697, 线性递减 ~1.25/s |
| [3] | param_c | 设计参数 | 恒定 2637 |
| [4] | param_d | 设计参数 | 恒定 2621 |
| [5] | ic_param_b | 充电IC参数B | = [2]+405 |
| [6] | vbus_mv | 总线电压 (mV) | 3643→4423, 随充电升高 |
| [7] | const_409 | 常量 | 恒定 409 |
| [8] | ibat_ma | 电池电流 (mA) | 负值=充电, -1740→-7981→+76→-556 |
| [9] | charge_budget | 充电预算 | 91→0, dumpsys reset 后复位到 91 |
| [10] | budget_sub | 预算子项 | = [9]-11, 差值恒定 |
| [11] | batt_vol | 电池电压 (mV) | 3623→4454, 随充电升高 |
| [12] | field_12 | 未知 | 通常 0 |
| [13] | field_13 | 未知 | 通常 0 |
| [14] | ufcs_max_ma | UFCS 最大电流 | 1900 |
| [15] | ufcs_en | UFCS 使能 | 1→0 (充电结束时) |
| [16] | pps_max_ma | PPS 最大电流 | 0→2 |
| [17] | pps_en | PPS 使能 | 0→2 |
| [18] | cable_type | 线缆类型 | 0 |

---

## 六、待验证的推测性代码

### 6.1 RISE #1 电流递增算法 ❌❌❌

**问题:** 代码用 `current_ma += inc_step` (固定 100mA 线性递增)，但 strace 实测步长完全不规则:

```
500 → 1750 (+1250)  ← 0ms 内连续写入
1750 → 3550 (+1800) ← 2s 间隔
3550 → 3650 (+100)  ← 2s 间隔
3650 → 5400 (+1750) ← 2s 间隔
5400 → 7550 (+2150) ← 2s 间隔
7550 → 8000 (+450)  ← 480ms 间隔 (已达上限)
```

**分析:** 步长 1250/1800/100/1750/2150/450 无任何简单公式可解释。500 和 1750 在同一时间戳写入 (间隔 0ms)，说明初始有快速双步机制。2s 间隔与 loop_interval_ms=2000 一致。

**可能的算法方向:**
1. bcc_parms 某字段直接驱动 force_val (非自主计算)
2. 多段加速逻辑，每段步长由不同公式计算
3. 基于 vbat/ibat 反馈的自适应步长

**验证方法:** frida hook write(12, ...) 打印调用栈，定位 RISE 函数地址后反汇编。

### 6.2 CV 衰减算法 ❌❌❌

**问题:** 代码用 `current_ma -= adjust_step` (固定 50mA 递减)，但 strace 实测:

```
初始大步长 (触发衰减时, 跨分钟):
  8000→6400 (-1600)  距上次写入 204 秒
  6400→5300 (-1100)  距上次写入 180 秒
  5300→4300 (-1000)  距上次写入 333 秒

小步长稳态衰减 (~480ms 间隔):
  4300→4250→4200→4150→...→1000 (步长 50mA)

电流回升 (代码无此逻辑):
  3450→3500 (+50mA)
```

**分析:** 初始大步长说明衰减不是自主计算，而是由 bcc_parms 反馈驱动。大步长之间间隔数分钟，期间 force_val 不变但 bcc_parms 持续变化 (vbat 升高, ibat 降低)。当某个阈值触发时才大幅降低 force_val。

**关键 bcc_parms 数据 (8000→6400 触发点):**
```
触发前: fields[2]=743, fields[8]=-6388, fields[9]=75, fields[10]=65, fields[11]=4268
触发后: fields[2]=742, fields[8]=-6809, fields[9]=75, fields[10]=65, fields[11]=4281
```
fields[11](batt_vol)=4268→4281，可能触发条件是 vbat 达到某个阈值。

**验证方法:** frida hook 或多次 strace 对比不同温度/SoC 下的触发点。

### 6.3 RISE #2 起始电流和步长 ⚠️

**问题:**
- 起始电流: 代码设 `current_ma=1000`，strace 是 **1300** (+300mA 偏差)
- 步长: 代码 `step_ma/10 = 9100/10 = 910`，strace 是 **800**

**分析:** 800 = 8000/10，其中 8000 是 CABLE_MAX_VOTER 限制后的有效最大电流。可能步长 = effective_max/10 而非 step_ma/10。起始 1300 的 300mA 偏差来源不明，可能与 bcc_parms 某字段有关。

### 6.4 负电流/放电模式 ❌

**问题:** strace 中 force_val 出现负值:

```
0 → -150 → -100 → 0 → 50 → -100 → -50 → 500 → 450 → 250 → 50 → 0
→ -150 → -300 → -450 → -600 → -550 → -450 → -400 → -350 → -300
→ 1000 (进入 TC 保持)
```

**分析:** 代码中 `write_current()` 不处理负值。这段放电序列发生在 CV 衰减末尾和 TC 保持之间，可能是"去极化"或"电池放松"阶段。

### 6.5 ChargePhase 状态机 ❌

**问题:** 代码定义 IDLE/RISE/CV/TC/FULL 五个阶段，但 strace 无阶段名称输出。
strace 只观测到: RISE→衰减→放电→TC保持→新RISE 的电流模式，与代码的五阶段模型部分吻合但细节不符。

### 6.6 batt_full_thr_mv=4570 ❓

本次充电未达到满电 (最高 vbat=4545mV < 4570mV)，无法验证。

### 6.7 日志写入机制 ⚠️

**问题:** 代码每次 `log_write()` 都 open()+write()+close()。strace 显示二进制将 stdout(fd=1) 重定向到日志文件，直接 `write(1, ...)` 而非反复打开。

### 6.8 充电控制轮询间隔 ⚠️

**strace 实测:** 充电线程轮询间隔为 400ms/650ms/450ms 混合，平均 475ms。
**代码:** `loop_interval_ms=2000`，动态轮询用 `ufcs_interval_ms={650,400}`。450ms 间隔无对应。

### 6.9 battery_log_content 解析 ❌

Thread 3 每 5s 读取 battery_log_content 但不解析。二进制可能用此数据做充电决策。
格式: `,temp_01c,const_409,vbat?,ibat?,power?,SoC,SoC,charge_status,...,FCC,SoC,...`

---

## 七、高级数据采集方案

详见 `tmp/ADVANCED_DATA_COLLECTION.md`，推荐优先级:

1. **sysfs 快照 + strace** — 零成本，立即可做
2. **frida 调用栈回溯** — 定位 RISE #1 和 CV 衰减函数的关键
3. **oplus_chg 动态调试** — 确认内核驱动行为
4. **增强 strace (多次充电周期)** — 对比不同条件下的行为差异
5. **充电 IC 寄存器直读** — 最底层验证 bcc_parms 字段含义
