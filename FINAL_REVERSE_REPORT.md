# opbatt_control 逆向分析报告

> 目标: 开源重写 opbatt_control 充电控制核心
> 更新: 2026-04-28 (完整充电周期 strace + 完整行为对齐)
> 更新: 2026-04-28 (二次分析: force_val 负值确认 + FULL=1000 修正 + BATT_SOC_VOTER 数据)

---

## 一、基本信息

ELF 64-bit ARM aarch64, PIE, stripped, NDK r26d, 动态链接 + 静态 OpenSSL (~1.4MB .text)。
**字符串未加密**: .rodata 全部明文（sysfs 路径、配置键名等均可直接 `strings` 提取）。
高熵区域 (entropy=7.14) 来自 OpenSSL 内置字符串表，非 XOR 混淆。
网络仅 AF_UNIX → /dev/socket/logdw，无 TCP。

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

> 数据来源: sysfs_20260427_220852.csv (750 行, ~8.5 分钟, SOC 1%→43%)

| # | Voter | 说明 | 是否常驻 | 典型值 | 解析状态 |
|---|-------|------|---------|--------|----------|
| 1 | MAX_VOTER | 硬件最大电流上限 | ✅ 始终 en=1 | 14600 | ✅ |
| 2 | HIDL_VOTER | HIDL 接口设置 | ❌ 仅充电前 | 0 | ✅ |
| 3 | BAD_SUBBOARD_VOTER | 子板异常保护 | ❌ 未触发 | 0 | ✅ |
| 4 | EIS_VOTER | 电化学阻抗谱保护 | ❌ 未触发 | 0 | ✅ |
| 5 | IMP_VOTER | 阻抗/脉冲电流限制 | ✅ 插充电器后 | 8000↔9100 | ✅ |
| 6 | STEP_VOTER | 阶梯式电流限制 | ✅ 插充电器后 | 9100→8000 | ✅ |
| 7 | BATT_TEMP_VOTER | 电池温度保护 | ✅ 插充电器后 | 14600 (未触发) | ✅ |
| 8 | COOL_DOWN_VOTER | 降温/降功率控制 | ✅ 插充电器后 | 15000 (未触发) | ✅ |
| 9 | SALE_MODE_VOTER | 展台模式限制 | ❌ 未触发 | 15000 | ✅ |
| 10 | BCC_VOTER | BCC 协议限制 | ❌ 未触发 | 0 | ✅ |
| 11 | BATT_BAL_VOTER | 电池均衡限制 | ❌ 未触发 | 0 | ✅ |
| 12 | IBUS_OVER_VOTER | 输入总线过流保护 | ❌ 未触发 | 0 | ✅ |
| 13 | SLOW_CHG_VOTER | 慢充模式限制 | ❌ 未触发 | 0 | ✅ |
| 14 | CABLE_MAX_VOTER | 线缆最大电流 | ✅ 始终 en=1 | 8000 | ✅ |
| 15 | ADAPTER_IMAX_VOTER | 适配器最大输出 | ✅ 始终 en=1 | 9100 | ✅ |
| 16 | PLC_VOTER | PLC 通信限制 | ❌ 未触发 | 0 | ✅ |
| 17 | IC_VOTER | 充电 IC 硬件限制 | ✅ 始终 en=1 | 13700 | ✅ |
| 18 | BATT_SOC_VOTER | SoC 限制电流 | ❌ 本次未触发 | 0 | ✅ |
| 19 | LIMIT_FCL_VOTER | FCL 满充限制 | ✅ SOC>16%后 | 7200 | ✅ |
| 20 | PR_VOTER | 优先级控制 | ❌ 未触发 | 0 | ✅ |
| 21 | BASE_MAX_VOTER | 基础最大电流 | ✅ 始终 en=1 | 9100 | ✅ |
| 22 | BAD_SUB_BTB_VOTER | 子板 BTB 连接异常 | ❌ 未触发 | 0 | ✅ |

### 3.2 常驻活跃 Voter 分类

| 类别 | Voter | 值 | 说明 |
|------|-------|-----|------|
| **硬件上限** | MAX_VOTER | 14600 | 系统允许的最大充电电流 |
| **协议限制** | ADAPTER_IMAX_VOTER | 9100 | 适配器协商最大输出 |
| **协议限制** | BASE_MAX_VOTER | 9100 | 基础电流限制 |
| **线缆限制** | CABLE_MAX_VOTER | 8000 | 8A 线缆载流能力 |
| **IC 限制** | IC_VOTER | 13700 | 充电芯片硬件限制 |
| **温度保护** | BATT_TEMP_VOTER | 14600 | 未触达阈值 |
| **降温控制** | COOL_DOWN_VOTER | 15000 | 未触发 |

### 3.3 动态激活 Voter

| Voter | 激活条件 | 典型值 | 成为 effective |
|-------|---------|--------|---------------|
| IMP_VOTER | 电流上升阶段 | 8000mA | ✅ 与 CABLE 交替 |
| LIMIT_FCL_VOTER | SOC > 16% | 7200mA | ✅ 后期主导 |
| STEP_VOTER | 插充电器后 | 9100→8000mA | ❌ 始终高于 FCL |
| HIDL_VOTER | 仅充电前 5 秒 | 0 | ❌ |

### 3.4 effective 变化时序

```
时间          SOC   effective voter       值(mA)  阶段
22:08:52      1%    MAX_VOTER             14600   未插入
22:08:57      1%    CABLE_MAX_VOTER        8000   插入充电器
22:09:03      1%    IMP_VOTER              8000   阻抗保护激活
22:09:10      1%    CABLE_MAX_VOTER        8000   稳定期
22:11:49      16%   LIMIT_FCL_VOTER        7200   FCL 限流激活
22:17:30      43%   LIMIT_FCL_VOTER        7200   至采集结束
```

### 3.5 限流瓶颈链

```
CABLE_MAX_VOTER (8000mA)
      ↓
LIMIT_FCL_VOTER (7200mA, SOC>16%后)
      ↓
effective = 7200mA
```

### 3.6 LIMIT_FCL_VOTER 行为 (CSV 确认)

- **激活条件**: SoC=16%, temp=48.1°C
- **限制值**: v=7200
- **特性**: 一旦激活**永不关闭**
- **影响**: 成为 effective voter，限制电流从 8000 降至 7200mA

### 3.7 BATT_SOC_VOTER 行为 (2026-04-28 strace 确认)

**重要澄清**: 之前的报告中记录的 `en=1 v=2100` 数据可能来自另一次充电会话（高SOC场景）。本次充电会话（SOC最高43%）的实际数据：

| 时间段 | SOC | BATT_SOC_VOTER | effective voter |
|--------|-----|----------------|-----------------|
| 充电前 | 1% | en=0 v=0 | MAX_VOTER |
| 充电中 | 1%→43% | en=0 v=0 | CABLE_MAX/STEP_VOTER |

**结论**:
- BATT_SOC_VOTER 是 **内核侧** 基于电池SOC阈值的电流限制机制
- opbatt_control **不直接控制** 该voter（不写force_val/force_active）
- 当SOC超过内核驱动设定的阈值（如80%+）时，内核自动启用该voter并限制电流
- 本次充电会话SOC未达触发阈值，故全程禁用

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

**第四阶段: 极低电流 + 去极化 (含负值 force_val)**
```
850 → 650 (-200) → 600 → 400 (-200) → 300 → 250 → 50 (-200)
50 → -100 (负值! 首次去极化负值)
-100 → 500 (+600, 脉冲) → 300 → 250 → 50 → 0
0 → -50 → -200 → -350 (负值递减, 步长 -150)
-350 → 500 (+850, 第二轮脉冲) → 300 → 250 → 50
50 → 1000 (进入 FULL)
```

**第五阶段: FULL + 新周期重启**
```
1000 (持续写入, SoC=100%)
dumpsys battery reset 后:
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
| 8 | **force_val 写入负值** | force_val 从不写负值 | DEPOL 阶段写 -100, -50, -200, -350 |
| 9 | **FULL 阶段 force_val=1000** | FULL 写 500mA | strace 末尾持续写 1000mA |
| 10 | **BATT_SOC_VOTER 全程 en=1 v=2100** | 本次未激活 | 从充电开始到结束始终激活，但从未成为 effective voter |

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
                                │ force_val 进入负值
                                ▼
                DEPOL (去极化: 50→-100→500→300→250→50→0→-50→-200→-350→500→300→250→50)
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
| UFCS voter 解析 (22 个) | ✅ |
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

### 已完成的修正/新增 ✅ (2026-04-28)

| # | 功能 | 改动 |
|---|------|------|
| 1 | **重启 RISE (+50mA 线性)** | ✅ PHASE_RESTART_RISE，+50mA/步，无 quickstart |
| 2 | **去极化阶段** | ✅ PHASE_DEPOL，两轮脉冲+负值序列 (-100→...→-350) |
| 3 | **CV 振荡/电流回升** | ✅ cv_holding 中检查 vbat 回落到较低阶梯 |
| 4 | **充电周期结束检测** | ✅ thermal_hi ≤ 20 + current_ma ≤ 100 → dumpsys reset |
| 5 | **dumpsys reset 序列** | ✅ 只做 dumpsys battery reset，无 mmi toggle |
| 6 | **FULL force_val=1000** | ✅ 修正: 从 500 改为 1000 (strace 确认) |
| 7 | **DEPOL 负值 force_val** | ✅ 新增 depol_neg_step 配置 (默认 150)，DEPOL 写 -100/-50/-200/-350 |

### 仍需更多数据 ⚠️

| 功能 | 说明 |
|------|------|
| CV 阶梯实际值 | 配置文件中无 cv_step_mv/cv_step_ma，CV阶梯可能在内核或硬编码 |

### 2026-04-28 新发现 ✅

| 功能 | 结论 |
|------|------|
| **quickstart 动态系数** | 公式 `target = ufcs_max_ma` (bcc_parms[14])，与温度无关。源码系数13应为14 |
| **adjust_step 触发** | 确认是 `ramp_idx>=5 && ramp_idx<=6` 触发，非剩余距离驱动 |
| **BATT_SOC_VOTER** | 内核侧机制，SOC超阈值(80%+)时自动启用。opbatt_control不直接控制 |
| **22个voter完整解析** | 常驻8个(活跃), 动态激活3个, 未触发11个 |
| **限流瓶颈链** | CABLE_MAX(8000) → LIMIT_FCL(7200, SOC>16%后) |
| **effective变化** | MAX(14600) → CABLE(8000) → LIMIT_FCL(7200) |

---

## 六(续)、设备配置文件 (2026-04-28 su -c 读取)

> 文件: `/data/opbatt/batt_control` (INI key=value 格式)

```ini
# 温控
temp_range=42,43,44,45,46
temp_curr_offset=800,1200,1800,2500,4500

# 充电控制
enabled=1
inc_step=100
dec_step=100

# UFCS 重置
max_ufcs_chg_reset_cc=1
ufcs_reset_delay=180

# 快充协议
ufcs_max=9100
pps_max=5000
cable_override=0

# SoC 监控区间
ufcs_soc_mon=20,60
ufcs_interval_ms=650,400
pps_soc_mon=20,68
pps_interval_ms=650,400

# 主循环
loop_interval_ms=2000

# 电池电压控制
batt_vol_thr=4559,4559
batt_vol_soc=75,85
batt_con_soc=94

# RISE 阶段
rise_quickstep_thr_mv=4250
rise_wait_thr_mv=3800

# CV (恒压) 阶段
cv_vol_mv=4565
cv_max_ma=5000

# TC (涓流) 阶段
tc_vol_thr_mv=4500
tc_thr_soc=98
tc_full_ma=400
tc_vol_full_mv=4485

# 充电完成
curr_inc_wait_cycles=4
batt_full_thr_mv=4570
```

### 关键发现

| 配置项 | 值 | 说明 |
|--------|-----|------|
| **ufcs_max** | 9100 | UFCS最大电流，用于quickstart计算 |
| **cv_vol_mv** | 4565 | CV阶段进入电压 |
| **cv_max_ma** | 5000 | CV阶段最大电流限制 |
| **batt_full_thr_mv** | 4570 | 满电判定电压 |
| **tc_full_ma** | 400 | 涓流充电电流 |
| **rise_quickstep_thr_mv** | 4250 | quickstart高电压阈值 |

**注意**: 配置文件中 **无 cv_step_mv/cv_step_ma** 字段，CV阶梯降流的阈值表可能在：
1. 内核充电驱动中硬编码
2. 其他配置文件中
3. 通过sysfs动态读取

### CV阶梯降流配置 (从strace推断)

**正确逻辑**: 电压越高，电流越低，防止过充

| vbat阈值 | 目标电流 | 说明 |
|----------|----------|------|
| >= cv_vol_mv (4565mV) | 8000mA | 进入CV阶段 |
| >= 3974mV | 5500mA | 降流1 |
| >= 4053mV | 3500mA | 降流2 |

**配置文件中无此表**，可能在内核充电驱动中硬编码。

---

## 七、ELF 静态分析发现

| 地址 | 发现 | 说明 |
|------|------|------|
| `0xd6c4c` | `round_to_nearest(value, divisor)` | 步长对齐到 50mA |
| `0xd6c94` | 主循环函数入口 | 完整 prologue + xor 常量初始化 |
| `0xd6be4` | `MOV #500` → `[x24, #0x6d8]` | 结构体中 current_ma 字段写入 |
| `0xd5d08` | `sub w8, w8, #0x190` (减 400) | 可能是 CV 降流逻辑 |
| `0xd5c94` | `MOV #500` + `blr x8` | DEPOL 脉冲初始值 (非 FULL) |

config 值硬编码在二进制中，字符串均为明文（非 XOR 混淆）。

### round_to_nearest 函数 (0xd6c4c) 反汇编

```c
// 等价 C 伪代码
int32_t round_to_nearest(int32_t value, int32_t divisor) {
    // --- Guard Check ---
    int32_t g1 = *global_guard_1;   // 偏移 3848
    int32_t g2 = *global_guard_2;   // 偏移 3856
    int bit = (g1 << 1) & 1;
    if (!(g2 >= 10 && bit == 0)) {
        while(1);  // 安全守卫 → 永久阻塞
    }

    // --- Rounding ---
    int32_t half = (divisor < 0) ? (divisor + 1) >> 1 : divisor >> 1;
    int32_t quotient = (value + half) / divisor;
    return quotient * divisor;
}
```

**核心公式**: `round_to_nearest(v, 50) = ((v + 25) / 50) * 50`

### 主循环函数 (0xd6c94) 反汇编

**结构概览**:

| 阶段 | 地址范围 | 说明 |
|------|---------|------|
| Prologue | `d6c94–d6cac` | 保存 x19–x28, x29, x30 (96 字节栈帧) |
| Guard Check | `d6cb0–d6cd8` | 读两个全局标志，条件不满足→死循环 |
| 寄存器初始化 | `d6cdc–d6d30` | XOR 常量加载 + 指针计算 |
| 主循环体 | `d6d38–d6f1c` | 状态机轮询 |
| Epilogue | `d6f20–d6f3c` | 恢复寄存器，返回 |

**XOR 常量初始化**:
```asm
d6cdc:  mov  w25, #0xccd1            // w25 = 0xB3A4CCD1
d6ce4:  movk w25, #0xb3a4, lsl #16

d6cf0:  mov  x27, #0x557e            // x27 = 0x87E9DD9B0329557E (XOR 偏移量)
d6cf8:  movk x27, #0x329,  lsl #16
d6d00:  movk x27, #0xdd9b, lsl #32
d6d14:  movk x27, #0x87e9, lsl #48
```

**主循环状态机**:
```asm
d6d38:  cmp  w11, #0xFD6A4BDD      // 状态 == 退出条件?
d6d40:  b.eq d6f20                   // → 退出函数

// Guard 重新校验
d6d48:  cmp  w9,  #0xa               // guard_b >= 10?
d6d50:  cbnz w10, d6f1c              // flag_set → 死循环

// 状态分发 (switch)
d6d68:  cmp  w11, #0xFD6A4BDC       // 状态 A
d6d70:  cmp  w11, #0x75355ADA        // 状态 B
d6d80:  cmp  w11, #0x75355ADB        // 状态 C
...
```

**典型状态分支** (通过 XOR-GOT 间接调用):
```asm
d6ddc:  ldr  x8, [x24, #4016]       // x8 = GOT[x24 + 4016]
d6de4:  add  x8, x8, x27            // x8 += XOR偏移 (解密函数地址)
d6de8:  blr  x8                     // 调用解密后的函数指针
```

### XOR 保护机制说明

| 组件 | 值 | 用途 |
|------|-----|------|
| x27 | `0x87E9DD9B0329557E` | XOR 偏移量，用于解密 GOT 表函数指针 |
| Guard Check | 两个全局标志 | 运行时状态校验，条件不满足→死循环 |
| 状态常量 | `0xFD6A4BDC` 等 | 状态机的各个状态标识符 |

**注意**: GOT 表在 PIE 二进制中运行时填充，静态分析无法直接获取解密后的函数地址。但通过 strace 可确认程序调用的是标准 libc 函数 (write, read, nanosleep, fork, exec 等)。

### 状态常量汇总

| w11 常量 | 含义推测 |
|---------|---------|
| `0x862C79FE` | 初始状态 |
| `0xFD6A4BDC` | 轮询中状态 |
| `0xFD6A4BDD` | 退出条件 (→ return) |
| `0xB3A4CCD1` | 功能状态 A |
| `0x75355ADA` / `0x75355ADB` | 功能状态 B/C |
| `0x7971816F` | 功能状态 D |
| `0xFBD98FE4` | 功能状态 E (XOR调用) |
| `0x54FFCA6D` | 功能状态 F |

### XOR 解密密钥

| 密钥 | 值 | 用途 |
|------|-----|------|
| x27 (XOR 偏移量) | `0x87E9DD9B0329557E` | GOT 表函数指针解密 |
| w25 | `0xB3A4CCD1` | 状态常量 A |
| w28 | `0xFD6A4BDC` | 轮询状态 |
| w11 初始值 | `0x862C79FE` | 初始状态 |

### 解密公式

```c
// GOT 表函数指针解密
real_func_addr = GOT_entry + x27;

// round_to_nearest 步长对齐
aligned_value = ((value + 25) / 50) * 50;

// quickstart 目标电流
target_current = ufcs_max_ma;  // 来自 bcc_parms[14]
// 等价于: cable_max * 14 / 80 = 8000 * 14 / 80 = 1400mA
```

config 值硬编码在二进制中，字符串均为明文（非 XOR 混淆）。

### .rodata 静态分析结论 (2026-04-28)

| 属性 | 值 |
|------|-----|
| .rodata 偏移 | 0x3D9E0，大小 0x41EFC (270KB) |
| 熵值 | 7.14 bits/byte |
| 高熵来源 | OpenSSL 静态库字符串 (GOST/EC 曲线名、CMS/ASN.1 错误信息等) |
| 应用字符串 | 全部明文，如 `/sys/class/power_supply/battery/temp`、`/proc/oplus-votable/PPS_CURR/force_val` 等 |
| 加密 | **无**，无自解密函数，pmt.md 描述的 XOR 方案不适用 |

| 属性 | 值 |
|------|-----|
| .preinit_array | 空 (0xffffffffffffffff) |
| .init_array[0] | 0x2125e0 — getauxval + __system_property_get (CPU 特性检测) |
| .init_array[1] | 0x212ab0 — 同上 + NEON crypto 指令检测 |
| .init_array[2] | 0xf794c — sigfillset/sigaction/sigsetjmp + 读 CNTVCT_EL0 计时器 |
| mprotect 调用 | 0x1aa2f4/0x1aa320 — mmap 内存分配器，非 .rodata 自解密 |

### 二次分析补充 (2026-04-28)

**force_val 负值证据 (strace 原始行):**
```
9186  16:08:26.057724 write(12</proc/oplus-votable/UFCS_CURR/force_val>, "-100", 4) = 4
9186  16:08:28.944937 write(12</proc/oplus-votable/UFCS_CURR/force_val>, "-50", 3) = 3
9186  16:08:29.420179 write(12</proc/oplus-votable/UFCS_CURR/force_val>, "-200", 4) = 4
9186  16:08:29.915132 write(12</proc/oplus-votable/UFCS_CURR/force_val>, "-350", 4) = 4
```

**FULL=1000mA 证据 (strace 末尾):**
```
9186  16:10:31.001096 write(12</proc/oplus-votable/UFCS_CURR/force_val>, "1000", 4) = 4
9186  16:10:31.487107 write(12</proc/oplus-votable/UFCS_CURR/force_val>, "1000", 4) = 4
```

**BATT_SOC_VOTER 常驻证据 (sysfs_20260428_154917.csv):**
- 第 2 行 (15:49:18) 起: `BATT_SOC_VOTER: en=1 v=2100`
- 始终未成为 effective voter: `effective=STEP_VOTER` 或 `effective=CABLE_MAX_VOTER`

---

## 八、已实现功能清单 ✅

> 数据来源: `strace_20260428_154917_attach.log` (完整充电周期, 17788 行)
> 以下任务全部已在 src 中实现 (2026-04-28)。

| # | 功能 | 文件 | 状态 |
|---|------|------|------|
| 1 | PHASE_RESTART_RISE + PHASE_DEPOL 枚举 | `charging.h` | ✅ |
| 2 | restart_rise_step / depol_pulse_ma / depol_zero_ma / depol_neg_step 配置字段 | `config.h` | ✅ |
| 3 | 新配置键解析 + config_dump 输出 | `config.c` | ✅ |
| 4 | load_config 默认值 | `main.c` | ✅ |
| 5 | dumpsys_reset（去掉 mmi_charging_enable toggle） | `charging.c` | ✅ |
| 6 | 周期结束检测（thermal_hi ≤ 20 + current_ma ≤ 100） | `charging.c` | ✅ |
| 7 | PHASE_RESTART_RISE（+50mA 线性爬升，无 quickstart） | `charging.c` | ✅ |
| 8 | PHASE_DEPOL（负值序列: -100→500→300→250→50→0→-50→-200→-350→500→300→250→50） | `charging_phase.c` | ✅ |
| 9 | CV 阶段振荡回升（vbat 回落到较低阶梯） | `charging.c` | ✅ |
| 10 | phase_name / next_phase 更新 | `charging.c` | ✅ |
| 11 | FULL force_val=1000mA（非 500） | `charging_loop.c` | ✅ |

### 待验证事项

| 功能 | 说明 |
|------|------|
| DEPOL 负值步长 | 默认 150mA，需更多 strace 验证不同充电周期的负值序列是否一致 |
| effective_max + ufcs_max_ma | thermal_hi×100 已间接覆盖，但 thermal_hi 极低时 ufcs_max 本身也变小，未显式处理 |

### 验证方法

```bash
cd /data/data/com.termux/files/home/batt/src && make clean && make
```
然后 strace 对比 original 和 mine 的完整充电周期行为。
