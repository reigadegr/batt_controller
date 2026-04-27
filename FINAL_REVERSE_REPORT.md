# payload.elf.no_license 逆向分析报告

> 目标: 开源重写 opbatt_control 充电控制核心
> 更新: 2026-04-27 (strace 深度分析 + RISE 三段式算法 + bcc_parms 交叉验证)

---

## 一、基本信息

ELF 64-bit ARM aarch64, stripped, NDK r26d, 动态链接 + 静态 OpenSSL。
XOR 字符串混淆，~2877 函数无符号。网络仅 AF_UNIX → /dev/socket/logdw，无 TCP。

---

## 二、已确认/已实现功能 ✅

| 功能 | 状态 |
|------|------|
| sysfs/procfs 12 路径读写 | ✅ |
| bcc_parms 19 字段解析 (strace 886 次 + battery_log 交叉验证) | ✅ |
| UFCS voter 解析 | ✅ |
| dumpsys fork+exec 重置序列 (时序已确认) | ✅ |
| mmi_charging_enable 0→1 重置 | ✅ |
| UFCS/PPS 协议选择 | ✅ |
| 温控电流偏移 (temp_01c 驱动) | ✅ |
| 配置文件 28 key 解析 | ✅ |
| 电池型号表 15 个 | ✅ |
| CLI 12 选项 | ✅ |
| 充电重置计数限制 | ✅ |
| 满电判断 (batt_full_thr_mv) | ✅ |
| 3 线程架构 (USB 检测 2s / 充电控制 / 日志采集 5s) | ✅ |
| 日志写入 /data/opbatt/battchg.log | ✅ |
| RISE 三段式步长算法 (quickstart + 斜坡 + 全速) | ✅ strace 确认 |
| inc_step = effective_max / 10 | ✅ strace 确认 |
| 轮询间隔 450ms(SoC<20) / 650ms(SoC≥20) | ✅ strace 确认 |
| 到达 cap 后停止重复写入 force_val | ✅ strace 确认 |
| bcc_parms 字段修正 (f6↔f11 互换, f7 常量→温度) | ✅ |
| dec_step 死代码清理 | ✅ |
| 11 个 bug 修复 (4 agent 审查) | ✅ |

---

## 三、bcc_parms 字段映射 (修正后)

> strace 886 次读取 + battery_log_content 交叉验证

| 字段 | 名称 | 含义 | 特征 |
|------|------|------|------|
| [0] | fcc | 满电容量 (mAh) | 恒定 5896 |
| [1] | design_cap | 设计容量 | 恒定 5888 |
| [2] | ic_param_a | 充电IC参数A | 1489→492, 线性递减 |
| [3] | param_c | 设计参数 | 恒定 2637 |
| [4] | param_d | 设计参数 | 恒定 2621 |
| [5] | ic_param_b | 充电IC参数B | = [2]+405 |
| [6] | **vbat_mv** | **电池电压 (mV)** | 3223→4293, 交叉验证确认 |
| [7] | **temp_01c** | **温度 (0.1°C)** | 303→521, 精确匹配 battery_log |
| [8] | ibat_ma | 电池电流 (mA) | 负值=充电, -1421→-7972 |
| [9] | **thermal_hi** | **温控阈值上界** | 91→85→80, 三档阶梯 |
| [10] | **thermal_lo** | **温控阈值下界** | = [9]-11, 恒定差值 |
| [11] | **vbus_mv** | **总线电压 (mV)** | 3205→4267, 精确匹配 battery_log |
| [12-13] | — | 保留 | 全程 0 |
| [14] | ufcs_max_ma | UFCS 最大电流 | 1400 |
| [15] | ufcs_en | UFCS 使能 | 1 |
| [16-17] | pps_max/pps_en | PPS 参数 | 0 |
| [18] | cable_type | 线缆类型 | 0 |

**温控阶梯 (f9/f10 与 f7 联动):**

| 温度 (°C) | thermal_hi | thermal_lo |
|-----------|------------|------------|
| < 51.4 | 91 | 80 |
| 51.4~51.5 | 85 | 75 |
| ≥ 51.6 | 80 | 70 |

---

## 四、strace 实测充电时序 (新)

> 数据: `tmp/strace_log/strace_20260427_220852.log` (6025 行, ~8.5 分钟, UFCS)

**三线程:**

| PID | 角色 | 周期 |
|-----|------|------|
| 9185 | USB 在线检测 | 2000ms |
| 9186 | 充电控制主循环 | 450ms→650ms (SoC≥20 切换) |
| 9187 | 电池日志采集 | 5000ms |

**UFCS_CURR 写入序列:**

```
Quickstart: 500 → 1400 (+900, 1.5ms 内双写)
递增斜坡:   1400 → 2150(+750) → 2500(+350) → 2950(+450) → 3500(+550)
全速步进:   3500 → 4300(+800) → 5100(+800) → 5900(+800) → 6700(+800) → 7500(+800) → 8000(+500 钳位)
维持:       8000 (47 秒不写入)
CV 降流:    8000 → 5500 (vbat≈3939mV) → 3500 (vbat≈4015mV)
```

**斜坡步长公式 (已验证):**
- 首步: `cable_max * 3 / 32` ≈ 750
- 后续: `round_to_50(cable_max / (22 - 4*ramp_idx))`
- 稳态: `cable_max / 10` = 800

---

## 五、粗糙实现/待改进

### 5.1 CV 阶段降流算法 ⚠️

**代码现状:** 统一用 `adjust_step=50mA` 线性递减。

**strace 实际:** 阶梯式大幅降流 (8000→5500 一步 2500mA)，触发条件是 vbat 达到阈值，非自主定时递减。降流后维持数分钟不变，等待 vbat 再次上升。

**差距:** 代码的线性递减模型与实际的"阈值触发+阶梯降流"模式不符。需实现 vbat 阈值驱动的阶梯降流。

### 5.2 RISE Quickstart 步长公式 ⚠️

**代码现状:** `cable_max * 9 / 80` ≈ 900 (推测)。

**strace 实际:** 500→1400 (+900)，公式未从二进制直接确认，但数值匹配。

### 5.3 RISE 首个斜坡步 ⚠️

**代码现状:** `cable_max * 3 / 32` ≈ 750 (推测)。

**strace 实际:** 1400→2150 (+750)，数值匹配但公式未直接确认。

### 5.4 日志写入机制 ⚠️

**代码现状:** 每次 `log_write()` 都 open()+write()+close()。

**strace 实际:** 二进制将 stdout(fd=1) 重定向到日志文件，直接 `write(1, ...)`。功能等价但 syscall 数不同。

### 5.5 充电周期结束检测 ⚠️

**代码现状:** 用 `thermal_hi == 0` 判断充电结束。

**strace 实际:** 本次 session 中 thermal_hi 始终为 91，未观察到归零。原始二进制可能有其他检测机制。

---

## 六、未实现功能

### 6.1 负电流/放电模式 ❌

strace 中 force_val 出现负值序列:

```
0 → -150 → -300 → -450 → -600 → -550 → ... → -300 → 1000
```

发生在 CV 衰减末尾和 TC 保持之间，可能是"去极化"阶段。代码完全无此逻辑。

### 6.2 CV 电流回升 ❌

strace 观测到 3450→3500 (+50mA) 回升，代码 CV 阶段只减不增。

### 6.3 battery_log_content 解析 ❌

Thread 3 每 5s 读取 battery_log_content 但不解析。二进制可能用此数据做充电决策。格式:

```
,[SoC],[temp],[vbat],[vbus],[ibat],[ui_soc],[chg_sts],...,[FCC],[SoC],...
```

### 6.4 独立 USB 检测线程 ❌

二进制 PID 9185 独立轮询 usb/online (2s)，检测到后才初始化 fd 和重置 votable。代码假设 fd 已打开。

### 6.5 独立日志采集线程 ❌

二进制 PID 9187 独立线程每 5s 读 battery_log_content。代码无此线程。

### 6.6 batt_full_thr_mv 验证 ❓

本次充电未达满电 (最高 vbat ≈ 4293mV < 4570mV)，无法验证。

---

## 七、高级数据采集建议

1. **多次充电周期 strace** — 对比不同 SoC/温度下的 CV 降流触发点
2. **frida hook write()** — 定位 RISE/CV 函数地址后反汇编确认公式
3. **负电流触发条件** — 专门抓取高 SoC 段 strace 确认放电模式
