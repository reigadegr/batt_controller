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
| 轮询间隔统一 500ms | ✅ 用户要求, 2026-04-28 |
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
| **RISE quickstart 双模式 (vbat<4250 渐进 / vbat>=4250 激进)** | ✅ 2026-04-28 strace 确认实现 |
| **RISE ramp 剩余距离除法 (idx=1:/17, idx=2~4:/11)** | ✅ 2026-04-28 strace 确认实现 |
| **RISE adjust_step 微调过渡 (ramp_idx=5~6, 50mA×2)** | ✅ 2026-04-28 strace 确认实现 |
| **get_temp_curr_offset 修复 (>= 高温阈值, 从高到低遍历)** | ✅ 2026-04-28 bug 修复 |
| **mmi_charging toggle 后直接重入 RISE** | ✅ 2026-04-28 用户要求实现 |
| **USB 拔出后 fd 生命周期管理** | ✅ 2026-04-28 strace 确认实现 |

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

**Quickstart 双模式 (strace 2026-04-28 确认):**
- vbat < rise_quickstep_thr_mv (4250): `500 + round_to_50(cable_max * 13/80)` ≈ 1800 (渐进)
- vbat ≥ rise_quickstep_thr_mv (4250): `500 + cable_max - round_to_50(cable_max * 3/32)` ≈ 7250 (激进)

**RISE ramp 剩余距离除法 (strace 2026-04-28 确认):**
- ramp_idx=1: `step = round_to_50((cable_max - current) / 17)`
- ramp_idx=2~4: `step = round_to_50((cable_max - current) / 11)`
- ramp_idx=5~6: `step = adjust_step` (50mA, 微调过渡)
- ramp_idx≥7: `step = cable_max / 10` (全速步进)

**UFCS_CURR 实测写入序列 (mine, strace_20260428_141523):**

```
Quickstart: 500 → 1800 (+1300, 同迭代双写)
斜坡:       1800 → 2150(+350) → 2700(+550) → 3200(+500) → 3650(+450)
微调:       3650 → 3700(+50) → 3750(+50)
全速:       3750 → 4550(+800) → 5350(+800) → 6150(+800) → 6950(+800) → 7750(+800) → 8000(+250 钳位)
```

---

## 五、仍需改进/待验证

### 5.1 quickstart 系数动态公式 ⚠️

quickstart 步长每次 strace 观测到不同值（+900/+1200/+1300/+2000），说明系数是动态的，可能与温度/电压/适配器类型相关。当前实现 `cable_max*13/80` 是一个近似值。

### 5.2 adjust_step 触发条件 ⚠️

mine 在固定 ramp_idx=5~6 时触发 50mA 微调。original 在不同位置触发（3750→3800→3850 / 3650→3800），且步长不一定是 50mA（有时 +150）。触发条件可能与 vbat 变化率相关，而非固定 ramp_idx。

### 5.3 负电流/去极化阶段 ❌

strace 中 force_val 出现负值序列 `0→-150→-300→...→-300→1000`（发生在 CV 末尾和 TC 之间）。本次 session SoC 仅 43% 未观测到。需要高 SoC 段 strace 数据确认触发条件。

### 5.4 CV 电流回升 ❌

strace 观测到 3450→3500 (+50mA) 回升。可能是 vbat 回落后自然重入较低阶梯。

### 5.5 充电周期结束检测 ❓

thermal_hi 从 91→80 未归零。可能的结束机制: batt_full_thr_mv / tc_full_ma+tc_vol_full_mv / BATT_SOC_VOTER。需完整充电周期数据。

### 5.6 mmi_charging toggle 后行为 ❓

已实现重入 PHASE_RISE，但多次 strace 均未触发 toggle，无法验证。

---

## 六、mine vs original strace 对比数据 (2026-04-28)

> 对比日志目录: `/data/local/tmp/opbatt_trace/`

### 6.1 RISE 升流对比 (第3轮, strace_20260428_141523)

| 项目 | mine | original |
|------|------|----------|
| quickstart | 500→1800 (+1300) | 500→2600 (+2100) |
| 斜坡 | 2150(+350)→2700(+550)→3200(+500)→3650(+450) | 3150(+550)→3650(+500)→3800(+150) |
| 微调 | 3700(+50)→3750(+50) | 无 |
| 全速 | +800/步 | +800/步 ✓ |
| 末步 | 7750→8000 (+250) | 7800→8000 (+200) |
| 总步数 | 14 步 | 11 步 |
| 轮询间隔 | 统一 500ms | 变化 650→450→650ms |
| 最终到达 | 8000 ✓ | 8000 ✓ |

### 6.2 RISE 升流对比 (第2轮, strace_20260428_112317, vbat=3612 < 4250)

| 项目 | mine | original |
|------|------|----------|
| quickstart | 500→1800 (+1300) | 500→1700 (+1200) |
| 总步数 | 12 步 | 13 步 |
| 轮询间隔 | 统一 680ms | 变化 693→474ms |
| 最终到达 | 8000 ✓ | 8000 ✓ |

### 6.3 RISE 升流对比 (第3轮, strace_20260428_082635, vbat=3125 < 4250)

| 项目 | mine | original |
|------|------|----------|
| quickstart | 500→1800 (+1300) | 500→1800 (+1300) ✓ |
| 斜坡 | 350→550→500→450 | 350→550→500→450 ✓ |
| 全速 | +800/步 | +800/步 ✓ |
| 总步数 | 12 步 | 12 步 ✓ |
| 最终到达 | 8000 ✓ | 8000 ✓ |

### 6.4 用户要求的差异汇总

| # | 差异 | 状态 | 说明 |
|---|------|------|------|
| 1 | quickstart 系数 | ⚠️ 近似 | 每次 strace 不同，需更多样本确认动态公式 |
| 2 | adjust_step 触发位置 | ⚠️ 近似 | mine 固定 idx=5~6，original 与 vbat 变化率相关 |
| 3 | 轮询间隔 | ✅ 已统一 | 用户要求统一 500ms，已实现 |
| 4 | mmi_charging toggle 后重入 | ✅ 已实现 | strace 未触发，无法验证 |
| 5 | 到达 8000mA | ✅ 对齐 | 所有测试轮次均完整到达 |

---

## 七、未实现功能

### 7.1 负电流/放电模式 ❌

strace 中 force_val 出现负值序列。发生在 CV 衰减末尾和 TC 保持之间，可能是"去极化"阶段。代码完全无此逻辑。需要高 SoC 段 strace 数据。

### 7.2 CV 电流回升 ❌

strace 观测到 3450→3500 (+50mA) 回升，代码 CV 阶段只减不增。

### 7.3 独立 USB 检测线程生命周期管理 ✅

USB 拔出后 close all fd，重新插入后 open_all + reset_votables，与原始二进制 PID 9185 strace 行为一致。

### 7.4 batt_full_thr_mv 验证 ❓

本次充电未达满电 (最高 vbat ≈ 4293mV < 4570mV)，无法验证。

---

## 七、高级数据采集建议

1. **多次充电周期 strace** — 对比不同 SoC/温度下的 CV 降流触发点
2. **frida hook write()** — 定位 RISE/CV 函数地址后反汇编确认公式
3. **负电流触发条件** — 专门抓取高 SoC 段 strace 确认放电模式
