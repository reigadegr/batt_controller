# payload.elf.no_license 逆向分析报告

> 目标: 开源重写 opbatt_control 充电控制核心
> 更新: 2026-04-28 (深度逆向 + CV阶梯降流 + voter扩展 + battery_log解析)

---

## 一、基本信息

ELF 64-bit ARM aarch64, stripped, NDK r26d, 动态链接 + 静态 OpenSSL。
XOR 字符串混淆，~2877 函数无符号。网络仅 AF_UNIX → /dev/socket/logdw，无 TCP。
深度逆向文档: `tmp/DEEP_REVERSE_ANALYSIS.md`

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
| **CV 阶梯降流 (vbat 阈值驱动)** | ✅ strace 确认, 2026-04-28 实现 |
| **CV 维持模式 (阶梯走完后不写 force_val)** | ✅ strace 确认, 2026-04-28 实现 |
| **LIMIT_FCL_VOTER 解析** | ✅ CSV 确认, 2026-04-28 实现 |
| **thermal_hi 阶梯限流 (×100→mA)** | ✅ CSV 确认, 2026-04-28 实现 |
| **STEP_VOTER 限流 (9100→8000)** | ✅ CSV 确认, 2026-04-28 实现 |
| **battery_log_content 解析** | ✅ strace 确认, 2026-04-28 实现 |
| **dec_step 配置项** | ✅ 2026-04-28 实现 |

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

## 五、仍需改进/待验证

### 5.1 负电流/去极化阶段 ❌

strace 中 force_val 出现负值序列 `0→-150→-300→...→-300→1000`（发生在 CV 末尾和 TC 之间）。本次 session SoC 仅 43% 未观测到。需要高 SoC 段 strace 数据确认触发条件。

### 5.2 CV 电流回升 ❌

历史 strace 观测到 3450→3500 (+50mA) 回升。可能是 vbat 回落后自然重入较低阶梯。

### 5.3 充电周期结束检测 ❓

thermal_hi 从 91→80 未归零。可能的结束机制: batt_full_thr_mv / tc_full_ma+tc_vol_full_mv / BATT_SOC_VOTER。需完整充电周期数据。

### 5.4 quickstart/首步公式 ⚠️

quickstart `cable_max*9/80` 和首斜坡步 `cable_max*3/32` 数值匹配但未从二进制直接确认。

### 5.5 config_dump 默认值 ⚠️

strace 显示 `ufcs_interval_ms: 650 400`，代码默认值 `450 650`。顺序和值不同，运行时由配置文件覆盖。

---

## 六、未实现功能

### 6.1 负电流/放电模式 ❌

strace 中 force_val 出现负值序列。发生在 CV 衰减末尾和 TC 保持之间，可能是"去极化"阶段。代码完全无此逻辑。需要高 SoC 段 strace 数据。

### 6.2 CV 电流回升 ❌

strace 观测到 3450→3500 (+50mA) 回升，代码 CV 阶段只减不增。

### 6.3 独立 USB 检测线程生命周期管理 ⚠️

二进制 PID 9185 在 USB 拔出后关闭所有 fd，重新插入时重新打开。代码中 sysfs_open_all 只调用一次。

### 6.4 batt_full_thr_mv 验证 ❓

本次充电未达满电 (最高 vbat ≈ 4293mV < 4570mV)，无法验证。

---

## 七、高级数据采集建议

1. **多次充电周期 strace** — 对比不同 SoC/温度下的 CV 降流触发点
2. **frida hook write()** — 定位 RISE/CV 函数地址后反汇编确认公式
3. **负电流触发条件** — 专门抓取高 SoC 段 strace 确认放电模式
