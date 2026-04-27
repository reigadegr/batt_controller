# payload.elf.no_license 逆向分析报告

> 目标: 开源重写 opbatt_control 充电控制核心

---

## 一、基本信息

ELF 64-bit ARM aarch64, stripped, NDK r26d, 动态链接 (libc/libdl/liblog/libm) + 静态 OpenSSL。
XOR 字符串混淆，仅 sysfs 路径和 `ro.arch` 明文保留。~2877 函数无符号。

---

## 二、已确认功能 (strace + 反汇编)

sysfs/procfs 10 路径读写 ✅ | bcc_parms 19 字段解析 ✅ | UFCS voter 解析 ✅ |
dumpsys fork+exec 重置序列 ✅ | mmi_charging_enable 0→1 重置 ✅ |
UFCS/PPS 协议选择 ✅ | 温控电流偏移 ✅ | 配置文件 28 key 解析 ✅ |
电池型号表 15 个 ✅ | CLI 12 选项 ✅ | 充电重置计数限制 ✅ |
UFCS 重置延迟 ✅ | 满电判断 4570mV ✅ | 3 线程架构 ✅ |
日志写入 /data/opbatt/battchg.log ✅ | USB 在线轮询 2s ✅ | 电池日志轮询 5s ✅

---

## 三、strace 实测充电时序

> 数据: `tmp/strace_log/strace_20260427_162732.log` (21171 行, 26 分钟, PJZ110, UFCS)

```
RISE #1: 500→1750→3550→3650→5400→7550→8000 (步长不均, 2s间隔)
reset:   votable=0 → dumpsys×3 → sleep(2) → mmi=0 → sleep(1) → mmi=1 → sleep(8)
RISE #2: 1300→2100→2900→...→8000 (步长恒定800, ~480ms间隔)
CV 衰减: 8000→6400→5300→4300→...→1000 (~480ms间隔, 步长50-100mA)
TC 保持: 1000mA × 28次 (~8秒)
新 RISE: 1050→1100→1150→1200→1250→1300 (2s间隔)
```

网络: 仅 AF_UNIX → /dev/socket/logdw，无 TCP。

---

## 四、代码 vs strace 不一致 (已修正)

| 问题 | 状态 |
|------|------|
| dumpsys 机制原报告误判为 dlopen | ✅ strace 确认 fork+exec |
| mmi_charging_enable 重置缺失 | ✅ 已补全 (0→sleep1→1→sleep8) |
| dumpsys reset 后缺 sleep(2) | ✅ 已补全 |
| bcc_parms 错误的 offset 检测 | ✅ 已简化为固定 [0] 起始 |
| Android 属性读取与二进制不一致 | ✅ 已移除 (二进制只读 ro.arch) |
| 日志仅 stdout | ✅ 已添加 /data/opbatt/battchg.log |
| ufcs_status 持久 fd 未使用 | ✅ 已移除，改用临时打开 |

---

## 五、仍未验证的推测性代码

| 功能 | 置信度 | 问题 |
|------|--------|------|
| **ChargePhase 状态机** (IDLE/RISE/CV/TC/FULL) | ❌ 推测 | strace 无阶段名，仅观测到 RISE→衰减→保持→新 RISE 电流模式 |
| **RISE #1 步长** (1250/1800/100/1750/2150/450) | ❌ 不一致 | 代码 inc_step=100 无法产生，rise_quickstep_thr_mv 减速逻辑也无法解释 |
| **RISE #2 步长** (恒定 800) | ❌ 不一致 | 与 RISE #1 差异大，可能 ufcs_en 状态变化导致 inc_step 被 STEP_VOTER/10 覆盖 |
| **CV 衰减触发机制** | ❌ 推测 | 代码用 cv_vol_mv 电压阈值，但 strace 中衰减可能由 bcc_parms 驱动 |
| **SoC 轮询间隔** | ❌ 矛盾 | 代码区间内 650ms > 区间外 400ms，与"快轮询"注释矛盾 |
| **500→5000 首次跳转** | ❌ 推测 | strace RISE #1 首步 500→1750，非 500→5000 |
| **batt_full_thr_mv=4570** | ❓ 未触发 | 本次充电未达到满电，无法验证 |
| **config key 前缀匹配** | ⚠️ 隐患 | `strncmp` 无 `=` 终止检查，"inc_step" 可能匹配 "batt_inc_step" |

---

## 六、待完成审查任务

### Agent 1: charging.c 逐行审查

对照 strace 逐函数验证 `src/charging.c`，重点:

1. **bcc_parms 字段含义**: strace 行 75 `5896,5888,1175,...` — fields[0]=5896 是 vbat? 还是别的?
2. **RISE #1 步长不一致** (strace 行 77-143): 代码如何从 inc_step=100 产生 1250/1800 步长? 还是说 bcc_parms 的某个字段直接驱动电流?
3. **RISE #2 步长 800** (strace 行 1076-1155): 是否因为 ufcs_en 从 1→0 触发了 `inc_step = step_ma/10 = 9100/10 ≈ 910`? 但实际是 800。
4. **CV 衰减** (strace 行 2945-19488): 当前"只限制上限"的 CV 逻辑能否产生持续衰减? 还是 bcc_parms 字段[7] (ibus_ma) 或其他字段在驱动?
5. **充电重启后起始电流 1300** (strace 行 1076): 代码设 `current_ma=1000`，为什么 strace 是 1300?

strace 日志: `tmp/strace_log/strace_20260427_162732.log`，每次最多读 30 行。

### Agent 4: 代码 vs strace 交叉验证

模拟代码执行路径，逐步对照 strace syscall 序列。按 PID 分组:
- PID 15505: USB 监控 (2s)
- PID 15506: 充电控制 (2s/480ms)
- PID 15507: 电池日志 (5s)

重点: 充电重置序列 (行 521-880)、RISE #2 (行 1076-1155)、CV 衰减 (行 2945-19488)。

### 已完成

Agent 2 (sysfs+monitor) ✅ | Agent 3 (config+cli+main) ✅ | 已修复 2 个 bug

### 输出

审查报告写入 `tmp/agent1_review.md` 和 `tmp/agent4_review.md`。
发现 bug 直接修复+提交，遵循 AGENTS.md 规范。
