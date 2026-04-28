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
| BATT_SOC_VOTER | SoC 限制 | en=1 v=2100 (全程常驻) | ❌ |
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
| quickstart 动态系数 | 公式 `cable_max * X / 80`, X=9~21, 可能与温度相关 |
| adjust_step 触发条件 | 可能是剩余距离驱动而非固定 ramp_idx |
| BATT_SOC_VOTER 限流逻辑 | CSV 确认全程 en=1 v=2100，但从未成为 effective voter；需验证其他场景下是否成为 effective |
| CV 阶梯实际值 | 配置文件 /data/opbatt/batt_control 设备上不存在，需安装模块后获取 |

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
| quickstart 系数 | 公式 `cable_max * X / 80`，X=9~21 可能与温度相关 |
| adjust_step 触发 | 可能是剩余距离驱动而非固定 ramp_idx |
| BATT_SOC_VOTER 限流 | CSV 确认 en=1 v=2100 全程常驻，但从未成为 effective；需验证其他场景 |
| effective_max + ufcs_max_ma | thermal_hi×100 已间接覆盖，但 thermal_hi 极低时 ufcs_max 本身也变小，未显式处理 |

### 验证方法

```bash
cd /data/data/com.termux/files/home/batt/src && make clean && make
```
然后 strace 对比 original 和 mine 的完整充电周期行为。
