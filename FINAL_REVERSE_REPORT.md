# payload.elf.no_license 逆向分析报告

> 目标: 开源重写 opbatt_control 充电控制核心，不包含许可证/网络通信部分

---

## 一、二进制基本信息

| 属性 | 值 |
|------|-----|
| 文件 | `tmp/payload.elf.no_license` |
| 文件大小 | 2,343,536 字节 (2.3 MB) |
| 架构 | ELF 64-bit ARM aarch64, PIE |
| 链接方式 | 动态链接 (liblog/libc/libm/libdl) |
| 构建工具链 | Android NDK r26d, clang |
| 目标平台 | Android 31+ (linker64) |
| 符号 | stripped，仅保留 129 个动态导入符号 |
| .text 段 | ~1.35 MB |
| .rodata 段 | ~263 KB (大部分为 OpenSSL 数据) |
| BuildID | sha1=00ab11aa1acdef2edcfb51f2737b85c4af9acaea |

---

## 二、字符串混淆

版本字符串含 `-OBF` 后缀（obfuscated）。几乎所有应用层字符串均为 XOR 混淆存储。

**明文保留的字符串**（仅 25 个）：
- 10 个 sysfs/proc 路径（充电控制核心）
- 15 个电池型号名（B-163 ~ P-521）
- OpenSSL 编译残留路径（`/root/opbattlic/openssl/...`）

**以下内容在二进制中均找不到明文**：
- 28 个配置键名（`temp_range`、`inc_step` 等）
- 日志格式串（`UFCS_CHG: AdpMAXma=...` 等）
- CLI 长选项名（`--charge`、`--temp` 等）
- UFCS voter 标签（`MAX_VOTER:`、`CABLE_MAX_VOTER:` 等）
- 配置文件路径 `/data/opbatt/batt_control`
- shell 命令串（`dumpsys`、`dmesg`、`OPLUS_CHG`）
- 许可证命令（`--generate-lic`、`--verify-lic`）

**XOR 解密函数**：位于 vaddr `0xe2900`-`0xe2a98`，使用 `output[i] = key[i] ^ data[i % 31]` 算法，密钥依赖运行时状态。

---

## 三、功能覆盖状态总览

### 3.1 ✅ 已确认一致的功能

| 功能 | 文件 | 验证方式 |
|------|------|----------|
| 3 线程架构 | main.c | strace 确认 |
| sysfs/proc 读写 (10 路径) | sysfs.c | 二进制明文完全匹配 |
| UFCS/PPS 电流控制 | charging.c | strace 确认 force_val/force_active 写入 |
| bcc_parms 解析 (19 字段) | charging.c | strace 实证 |
| UFCS voter 解析 (4 个) | charging.c | strace 实证 |
| 充电周期重启 | charging.c | strace 确认 charge_status 0→dumpsys reset |
| 协议选择 UFCS/PPS | charging.c | strace 确认 |
| 温控电流限制 | charging.c | strace 确认 temp_range + temp_curr_offset |
| 配置文件解析 (28 key) | config.c | strace 确认 config_dump 输出完全一致 |
| 电池型号表 (15 个) | cli.c | 与二进制完全一致 |
| CLI 12 个选项 | cli.c | 与二进制完全一致 |
| dumpsys 控制序列 | cli.c + charging.c | strace 确认 set ac 1 → set status 2 → reset |
| 内核日志采集 | cli.c | strace 确认 |
| USB 在线轮询 (2s) | monitor.c | strace 确认 |
| 电池日志轮询 (5s) | monitor.c | strace 确认 |
| 充电重置计数限制 | charging.c | strace + 反汇编确认，`restart_count >= max_ufcs_chg_reset_cc` |
| UFCS 重置延迟 | charging.c | 反汇编确认，默认 10s，配置值 180s |
| 满电判断 | charging.c | 反汇编确认，4570 常量存在于二进制 |

### 3.2 ⚠️ 已实现但与二进制不一致

| 功能 | src 实现 | 二进制实际 | 差异 |
|------|----------|------------|------|
| Android 属性读取 | 读 `ro.soc.model`/`ro.product.model`/`ro.build.version.sdk` 并打印 | 仅读 `ro.arch` 与 `exynos9810` 比较 | 属性名、用途完全不同 |
| dumpsys 执行机制 | `fork` + `execvp` | `dlopen`/`dlsym` 动态加载 | fork/execvp 不在 PLT 中 |

### 3.3 ⚠️ 已实现但逻辑简化（待验证）

| 功能 | 问题 | 置信度 |
|------|------|--------|
| CV 恒压阶段 | 只限制电流上限，无主动递减逻辑 | 中 |
| TC 涓流阶段 | 与 CV 几乎相同，无涓流特征 | 中 |

### 3.4 ❌ 已实现但逻辑可能有误

| 功能 | 问题 | 置信度 |
|------|------|--------|
| SoC 区间动态轮询 | 区间内 650ms > 区间外 400ms，与注释"快轮询"矛盾 | 高 |

### 3.5 ⚠️ 已实现但参数未验证

| 功能 | 未验证参数 |
|------|------------|
| RISE 上升阶段 | 减速比例 `/4`、`/8`，最小值 `50`、`25` |
| RISE 上升阶段 | 首次启动跳转 `500→5000` 触发条件 `restart_count <= 1` |
| 充电阶段状态机 | `next_phase()` 转换条件全部基于配置字段名推断 |

### 3.6 ❌ src/ 未覆盖

| # | 功能 | 证据 | 重要度 |
|---|------|------|--------|
| 1 | **XOR 字符串解密** | 二进制中几乎无应用层明文，-OBF 后缀确认 | 🔴 高 |
| 2 | **popen/pclose** | 导入符号，可能用于执行外部命令 | 🟢 低 |
| 3 | **dlopen/dlsym** | 导入符号，可能用于加载扩展模块 | 🟢 低 |

### 3.7 ⚠️ 忽略（许可证/网络相关）

以下导入由 OpenSSL 静态链接引入，仅用于许可证校验，开源重写不涉及：

- socket/connect/bind/listen/accept (TCP 通信)
- getaddrinfo/gethostbyname (DNS)
- HTTP/1.0 + OCSP (在线证书查询)
- mmap/mprotect/mlock (OpenSSL 内存管理)
- sigsetjmp/siglongjmp (OpenSSL 错误恢复)
- pthread_key TLS (OpenSSL 线程安全)

---

## 四、不一致/简化实现详细分析

### 4.1 Android 属性读取 — 不一致

**二进制实际行为**（反汇编确认）：
- 仅读取 **`ro.arch`**（2 次调用 `__system_property_get@plt`，地址 `0x212620` 和 `0x212ae4`）
- 与 **`exynos9810`** 进行 `strncmp(len=10)` 比较
- 用途：硬件平台检测（Samsung Exynos 9810），非通用设备信息日志
- 调用上下文涉及 `getauxval(AT_HWCAP)` / `getauxval(AT_HWCAP2)`，确认为硬件能力检测

**src 实现** (charging.c:199-205)：
```c
get_prop("ro.soc.model", prop_buf)      // 不存在于二进制
get_prop("ro.product.model", prop_buf)  // 不存在于二进制
get_prop("ro.build.version.sdk", prop_buf)  // 不存在于二进制
```

**结论**：src 的属性读取是推测性实现，与二进制不一致。二进制的 `ro.arch` 读取可能仅用于许可证校验（设备指纹），开源重写可考虑移除或保留为调试用途。

---

### 4.2 dumpsys 执行机制 — 不一致

**src 实现**：使用 `fork` + `execvp` 执行 dumpsys 命令。

**二进制实际行为**：
- `fork`/`execvp`/`waitpid`/`_exit` 虽在 .dynstr 中存在，但**不在 .rela.plt 中**（未被实际导入）
- 二进制实际使用 `dlopen`/`dlsym`/`dlclose`（已确认在 PLT 中）动态加载功能模块
- dumpsys 控制可能通过 dlopen 加载的外部 OPLUS .so 实现

**影响**：当前 src 的 fork+execvp 实现功能上等价（都能执行 dumpsys），但机制不同。如果二进制通过 dlopen 加载的 .so 有额外逻辑（如权限提升、环境准备），src 实现会缺失这些。

---

### 4.3 CV 恒压阶段 — 简化实现

**源码** (charging.c:471-485)：
```c
case PHASE_CV: {
    int cv_max = cfg->cv_max_ma > 0 ? cfg->cv_max_ma : effective_max;
    if (cv_max > effective_max)
        cv_max = effective_max;
    if (current_ma > cv_max) {
        current_ma -= cfg->dec_step > 0 ? cfg->dec_step : 100;
        if (current_ma < cv_max)
            current_ma = cv_max;
    }
    write_current(fds, use_ufcs, current_ma);
    break;
}
```

**问题**：
- 注释说"电流逐步递减以维持电压"，但代码只在 `current_ma > cv_max` 时递减
- 如果 `current_ma <= cv_max`，代码直接 `write_current(current_ma)`，**电流保持不变**
- 真正的 CV 阶段应该是**持续递减电流**以维持电压不超标
- 当前实现只是"电流上限限制器"，不是"恒压控制器"

**结论**：有框架骨架，但核心逻辑（持续递减）缺失。可能是简化实现，也可能是错误。

---

### 4.4 TC 涓流阶段 — 简化实现

**源码** (charging.c:487-501)：
```c
case PHASE_TC: {
    int tc_max = cfg->tc_full_ma > 0 ? cfg->tc_full_ma : 500;
    if (tc_max > effective_max)
        tc_max = effective_max;
    if (current_ma > tc_max) {
        current_ma -= cfg->dec_step > 0 ? cfg->dec_step : 100;
        if (current_ma < tc_max)
            current_ma = tc_max;
    }
    write_current(fds, use_ufcs, current_ma);
    break;
}
```

**问题**：
- 与 CV 阶段**几乎完全相同**，只是用 `tc_full_ma` 替代 `cv_max_ma`
- 涓流充电的特征是**极低电流**（通常 < 100mA），但这里只是限制到 `tc_full_ma`
- 没有涓流充电特有的"脉冲式充电"或"极低电流维持"逻辑

**结论**：有框架骨架，但与 CV 阶段区分度不足，涓流特征缺失。

---

### 4.5 SoC 区间动态轮询 — 逻辑可能有误

**源码** (charging.c:183-195)：
```c
/*
 * 注释: SoC 在 [soc_mon[0], soc_mon[1]] 范围内时使用 interval_ms[0] (快轮询)
 *       范围外使用 interval_ms[1] (慢轮询)
 */
if (soc >= soc_mon[0] && soc <= soc_mon[1])
    return interval_ms[0] > 0 ? interval_ms[0] : interval_ms[1];
return interval_ms[1] > 0 ? interval_ms[1] : interval_ms[0];
```

**矛盾分析**：

配置值：`ufcs_soc_mon=20 60`, `ufcs_interval_ms=650 400`

按照代码逻辑：
- SoC 在 [20, 60] 时 → 返回 `interval_ms[0]` = **650ms**
- SoC 不在 [20, 60] 时 → 返回 `interval_ms[1]` = **400ms**

**650ms > 400ms，区间内反而是慢轮询！** 与注释"快轮询"矛盾。

**可能的解释**：
1. 注释错误，代码正确（区间内确实应该是慢轮询）
2. 代码错误，注释正确（区间内应该是快轮询，interval 顺序反了）
3. `interval_ms` 数组的语义与注释不同（例如 interval_ms[0]=650 是"慢"的默认值，interval_ms[1]=400 是"快"的目标值）

**结论**：逻辑与注释矛盾，需要运行时验证。

---

### 4.6 RISE 上升阶段 — 参数未验证

**源码** (charging.c:427-468)：
```c
case PHASE_RISE: {
    int step = inc_step;
    if (cfg->rise_quickstep_thr_mv > 0 &&
        parms.vbat_mv >= cfg->rise_quickstep_thr_mv) {
        step = inc_step / 4;    // 未验证
        if (step < 50) step = 50;  // 未验证
    } else if (cfg->rise_wait_thr_mv > 0 &&
               parms.vbat_mv >= cfg->rise_wait_thr_mv) {
        step = inc_step / 8;    // 未验证
        if (step < 25) step = 25;  // 未验证
    }
    // ...
    if (current_ma == 500 && restart_count <= 1) {  // 未验证
        write_current(fds, use_ufcs, 500);
        current_ma = 5000;  // 未验证
        write_current(fds, use_ufcs, current_ma);
    }
}
```

**未验证参数**：
- 减速比例：`inc_step / 4`、`inc_step / 8`
- 最小步长：`50`、`25`
- 首次启动跳转：`500 → 5000`
- 触发条件：`restart_count <= 1`

**结论**：框架完整，但具体参数均为推测，需要运行时验证。

---

### 4.7 充电阶段状态机转换 — 全部未验证

**源码** (charging.c:200-230)：
```c
static ChargePhase next_phase(ChargePhase cur, const BattConfig *cfg,
                               const BccParms *parms, int soc)
{
    // IDLE → RISE: charge_status != 0
    // RISE → CV:   vbat >= cv_vol_mv
    // CV   → TC:   soc >= tc_thr_soc || vbat >= tc_vol_thr_mv
    // TC   → FULL: ibat <= tc_full_ma && vbat >= tc_vol_full_mv
    // ANY  → IDLE: charge_status == 0
}
```

**问题**：
- 所有转换条件均基于配置字段名推断
- 28 个配置键名全部 XOR 加密，无法通过字符串搜索定位阶段转换代码
- 配置值为运行时加载，非硬编码常量
- 阶段名称（IDLE/RISE/CV/TC/FULL）在二进制中无明文

**结论**：`next_phase()` 是推测性实现，无法通过静态分析确认与二进制一致。

---

## 五、不明之处与待验证问题

### 5.1 XOR 解密函数定位

**不明**：二进制使用 XOR 混淆字符串，但解密函数的位置和密钥未知。

**已知**：XOR 解密函数位于 vaddr `0xe2900`-`0xe2a98`，使用 `output[i] = key[i] ^ data[i % 31]` 算法。

**验证方法**：
1. 在模拟器中用 `ltrace` 或 Frida hook `strcmp`/`strstr`，在运行时捕获已解密的字符串
2. 如果能找到 `config_dump` 的 stdout 输出（strace 已有），可以从输出格式串反推 XOR 密钥

### 5.2 3 个缺失的 sysfs 路径

**不明**：src/ 使用了以下路径但二进制中无明文：
- `/sys/class/power_supply/usb/online`
- `/sys/class/oplus_chg/battery/bcc_parms`
- `/sys/class/oplus_chg/battery/battery_log_content`

**可能原因**：这 3 个路径可能被 XOR 混淆，或通过 `dlopen` + `dlsym` 动态解析。

**验证方法**：`strace -e trace=openat -p <pid>` 直接看进程打开的文件。

### 5.3 电池型号表的使用方式

**不明**：15 个电池型号（B-163~P-521）在 src/ 中仅用于 `--model` CLI 查询，但二进制可能在服务模式中也使用。

**推测**：型号的 `param` 字段（163/233/283/409/571/192/224/256/384/521）可能是电池容量或充电参数，可能根据当前设备的电池型号自动选择充电参数。

### 5.4 dlopen 加载的外部 .so

**不明**：二进制使用 dlopen/dlsym 动态加载模块，但加载的 .so 文件名和功能未知。

**推测**：可能是 OPlus 充电控制相关的系统库。

---

## 六、建议验证优先级

```
Phase 1: XOR 解密（解锁所有字符串，后续分析效率倍增）
Phase 2: 运行时验证充电状态机（strace 抓 force_val 写入值，确认 CV/TC/Rise 行为）
Phase 3: 运行时验证 SoC 轮询（strace 抓 usleep 参数，确认 interval 顺序）
Phase 4: 运行时验证 Android 属性读取（Frida hook __system_property_get）
Phase 5: 电池型号自动匹配（如果服务模式使用）
```

### 验证方法汇总

| 功能 | 验证方法 | 预期结果 |
|------|----------|----------|
| CV/TC | strace 抓 write 到 force_val 的值 | 应该看到持续递减 |
| RISE | strace 抓 write 到 force_val 的值 | 应该看到减速模式 |
| SoC 轮询 | strace 抓 usleep 的参数 | 应该看到与 SoC 联动的间隔变化 |
| 属性读取 | Frida hook `__system_property_get` | 应该看到 `ro.arch` |
| dumpsys | strace 抓 dlopen 调用 | 应该看到加载的 .so 文件名 |

---

## 七、总结

### 已确认的实现（可信赖）

- sysfs/proc 路径读写
- bcc_parms 解析
- UFCS voter 解析
- 充电周期重启机制
- 协议选择 UFCS/PPS
- 温控电流限制
- 配置文件解析
- 充电重置计数限制
- UFCS 重置延迟
- 满电判断（batt_full_thr_mv）

### 不一致的实现（需修正）

- Android 属性读取（ro.arch vs 3 个不同属性）
- dumpsys 执行机制（dlopen vs fork+execvp）

### 简化实现（可能正确，需验证）

- CV 恒压阶段（只限制上限，无主动递减）
- TC 涓流阶段（与 CV 几乎相同）

### 逻辑可能有误（需修正）

- SoC 轮询间隔（区间内外 interval 顺序与注释矛盾）

### 参数未验证（需运行时确认）

- RISE 阶段减速比例（/4, /8）
- RISE 阶段最小步长（50, 25）
- RISE 阶段首次启动跳转（500→5000）
- 充电阶段状态机转换条件（next_phase 全部）

### 完全未知（需进一步逆向）

- XOR 解密密钥
- dlopen 加载的 .so 文件
- 电池型号表在服务模式中的使用

---

## 八、strace 运行时验证结论

> 数据来源: `tmp/strace_log/strace_20260427_162732.log` (21171 行, 26 分钟充电会话)
> 抓取时间: 2026-04-27 16:27:32 → 16:53:59
> 设备: PJZ110, UFCS 协议, 3 线程 (PID 15505/15506/15507)

### 8.1 三线程行为确认

| PID | 角色 | 间隔 | 行为 |
|-----|------|------|------|
| 15505 | USB 在线监控 | 2s | 读 `/sys/.../usb/online`，检测充电器插入/拔出 |
| 15506 | 充电控制主循环 | 2s/480ms | 读 bcc_parms+temp+soc → 写 force_val → 轮询 |
| 15507 | 电池日志监控 | 5s | 读 battery_log_content，解析电池状态 |

### 8.2 充电重置序列 (dumpsys 机制修正)

**原报告结论**: dumpsys 通过 dlopen 动态加载，不使用 fork+exec

**strace 修正**: dumpsys 通过 **fork+exec** 实现，非 dlopen

证据:
- PID 10879/10884 为 dumpsys 子进程，读取 `/proc/self/exe` → `/system/bin/dumpsys`
- PID 15506 收到 `SIGCHLD {si_pid=10879, si_status=0}` 和 `SIGCHLD {si_pid=10884, si_status=0}`
- 无 `clone` syscall 记录（因 strace 未跟踪子进程创建点），但 SIGCHLD 确认子进程存在

完整重置序列 (strace 确认):
```
1. write PPS_CURR/force_val = "0"
2. write PPS_CURR/force_active = "0"
3. write UFCS_CURR/force_val = "0"
4. write UFCS_CURR/force_active = "0"
5. fork+exec: dumpsys battery set ac 1
6. fork+exec: dumpsys battery set status 2
7. fork+exec: dumpsys battery reset
8. nanosleep(2s)
9. write mmi_charging_enable = "0"   ← 原报告未记录
10. nanosleep(1s)
11. write mmi_charging_enable = "1"
12. nanosleep(8s)   ← 等待充电重新初始化
```

### 8.3 完整充电电流时序

```
时间            电流(mA)  阶段
16:27:46.655    500       RISE #1 起始
16:27:46.655    1750      +1250
16:27:48.680    3550      +1800
16:27:50.717    3650      +100
16:27:52.744    5400      +1750
16:27:54.772    7550      +2150
16:27:55.252    8000      +450  (到达 MAX)
...
16:28:23.600    0         ← 充电重置触发
16:28:25.861    -         mmi_charging_enable = 0
16:28:26.880    -         mmi_charging_enable = 1
16:28:35.598    1300      RISE #2 起始 (步长 800)
16:28:36.074    2100
16:28:36.553    2900
16:28:37.043    3700
16:28:37.526    4500
16:28:38.005    5300
16:28:38.486    6100
16:28:38.979    6900
16:28:39.459    7700
16:28:39.939    8000      (到达 MAX)
16:31:19.832    6400      CV 衰减开始
16:34:20.208    5300
16:39:53.789    4300
16:52:06.077    4250      持续衰减...
16:52:09.073    4200
16:52:09.501    4150
16:52:11.642    4100
...             (50mA 步长递减)
16:53:35.945    1000      TC 保持
16:53:36~42     1000×28   持续 1000mA (约 8 秒)
16:53:44.165    1050      新 RISE 开始
16:53:46.095    1100
16:53:48.045    1150
16:53:49.970    1200
16:53:51.903    1250
16:53:53.839    1300      日志结束
```

### 8.4 代码修正对照

| 问题 | 原实现 | strace 确认 | 修正状态 |
|------|--------|-------------|----------|
| dumpsys 机制 | fork+execvp | fork+execvp ✅ | 已一致 |
| mmi_charging_enable 重置 | 缺失 | 0→sleep(1)→1→sleep(8) | ✅ 已修正 |
| bcc_parms offset | 动态检测前导逗号 | 19 字段无前导逗号 | ✅ 已简化 |
| Android 属性读取 | ro.soc.model 等 3 个 | 仅 ro.arch (用于硬件检测) | ✅ 已移除 |
| 日志输出 | 仅 stdout | stdout + /data/opbatt/battchg.log | ✅ 已添加 |
| PPS_CURR 使用 | 有 force_val 写入逻辑 | 全程写 "0"，UFCS 为唯一协议 | 框架保留 |

### 8.5 网络行为

- 仅 3 次 connect 调用，全部为 `AF_UNIX` → `/dev/socket/logdw`（Android 日志守护进程）
- **无 TCP/TLS 连接**（符合预期：`payload.elf.no_license` 已去除许可证/网络模块）

### 8.6 sysfs/procfs 路径补充

strace 发现的额外路径（原报告未列出）:

| 路径 | 用途 |
|------|------|
| `/sys/devices/platform/soc/soc:oplus,mms_wired/oplus_mms/wired/usb/online` | usb/online 的实际设备路径 |
| `/sys/devices/platform/soc/soc:oplus,mms_gauge/oplus_mms/gauge/battery/temp` | battery/temp 的实际设备路径 |
| `/sys/devices/virtual/oplus_chg/battery/bcc_parms` | bcc_parms 的实际设备路径 |
| `/sys/devices/virtual/oplus_chg/battery/battery_log_content` | battery_log_content 的实际设备路径 |

这些是 `/sys/class/...` 符号链接的实际解析目标。

### 8.7 仍未验证的推测性代码

| 功能 | 状态 | 说明 |
|------|------|------|
| ChargePhase 状态机 (IDLE/RISE/CV/TC/FULL) | ❌ 推测 | strace 无阶段名输出，仅观测到 RISE→衰减→保持→新 RISE 的电流模式 |
| RISE 减速阈值 (rise_quickstep_thr_mv) | ❌ 推测 | strace 显示 RISE #1 步长不均匀 (100~2150)，RISE #2 均匀 (800) |
| calc_poll_interval SoC 联动 | ❓ 未确认 | strace 显示 RISE 期间 2s、CV 期间 ~480ms，可能与 SoC 无关 |
| cv_vol_mv/tc_thr_soc 转换条件 | ❌ 推测 | 无直接证据，电流衰减可能由其他机制触发 |

---

## 九、待完成审查任务

> Agent 2 (sysfs+monitor) 和 Agent 3 (config+cli+main) 已完成审查，发现的 bug 已修复。
> Agent 1 (charging.c) 和 Agent 4 (交叉验证) 因 API 限流失败，任务未完成。
> 下一个 agent 请按本节指引继续。

### 9.1 Agent 1 未完成任务: charging.c 逐行审查

**目标**: 逐函数审查 `src/charging.c`，对照 strace 验证每个逻辑分支的正确性。

**输入文件**:
- `src/charging.c` + `src/charging.h` (待审查代码)
- `tmp/strace_log/strace_20260427_162732.log` (21171 行, 每次最多读 30 行)
- `FINAL_REVERSE_REPORT.md` 第八节 (strace 结论)

**审查清单 (逐项完成，标注 ✅/❓/❌)**:

#### A. `charging_parse_bcc_parms()` (charging.c 约 47-76 行)
- [ ] 解析逻辑是否与 strace 中 bcc_parms 的 19 字段无前导逗号格式一致
- [ ] 用 strace 行 75 验证: `"5896,5888,1175,2637,2621,1580,3643,409,-1740,91,80,3623,0,0,1900,1,0,0,0"` → fields[0]=5896(vbat), fields[1]=5888(ibat?), fields[2]=1175(temp?)
- [ ] 字段含义标注是否合理 (vbat_mv/ibat_ma/temp_01c 等)

#### B. `charging_parse_ufcs_voters()` (charging.c 约 78-105 行)
- [ ] 是否正确解析 4 个 voter: MAX_VOTER, CABLE_MAX_VOTER, STEP_VOTER, BCC_VOTER
- [ ] 用 strace 行 14 的 UFCS_CURR/status 内容验证解析结果:
  ```
  MAX_VOTER: en=1 v=9100
  CABLE_MAX_VOTER: en=1 v=8000
  STEP_VOTER: en=1 v=9100
  BCC_VOTER: en=0 v=0
  ```
- [ ] effective=CABLE_MAX_VOTER type=Min v=8000 是否被正确处理

#### C. `run_dumpsys()` (charging.c 约 115-130 行)
- [ ] fork+execvp 实现是否与 strace SIGCHLD 证据一致 (PID 10879/10884)
- [ ] argv 构造是否正确: `dumpsys battery set ac 1` 等

#### D. `charging_dumpsys_reset()` (charging.c 约 146-158 行)
- [ ] 完整序列是否与 strace 一致:
  ```
  dumpsys set ac 1 → set status 2 → reset
  sleep(2)                    ← strace 行 868→874 (16:28:23.861→16:28:25.861)
  mmi_charging_enable = "0"   ← strace 行 874
  sleep(1)                    ← strace 行 874→880 (→16:28:26.880)
  mmi_charging_enable = "1"   ← strace 行 880
  sleep(8)                    ← strace 行 881→1076 (→16:28:35.598)
  ```

#### E. `charging_loop()` 主循环 (charging.c 约 277-558 行)

**E1. 初始化阶段** (strace 行 1-76):
- [ ] votable 重置 (写 "0" 到 4 个节点) 是否与 strace 行 39-42 一致
- [ ] voter 读取 3 次是否与 strace 行 61-65 一致
- [ ] 日志输出格式是否与 strace 行 72-73 的 battchg.log 内容匹配:
  ```
  [2026-04-27-16:27:46]: UFCS_CHG: AdpMAXma=9100ma, CableMAXma=8000ma, Maxallow=9100ma, Maxset=8000ma, OP_chg=1
  [2026-04-27-16:27:46]: ==== Charger type UFCS, set max current 8000ma ====
  ```

**E2. RISE 阶段** (strace 行 77-143, 1076-1155):
- [ ] RISE #1 电流序列 `500→1750→3550→3650→5400→7550→8000` 能否由当前 RISE 逻辑产生
  - 步长不均匀 (1250/1800/100/1750/2150/450)，当前 inc_step=100 的代码如何产生这些步长?
  - `curr_inc_wait_cycles` 和 `rise_quickstep_thr_mv` 是否能解释这种模式?
- [ ] RISE #2 电流序列 `1300→2100→2900→3700→4500→5300→6100→6900→7700→8000` (步长恒定 800)
  - 为什么 RISE #1 和 RISE #2 步长差异如此大?
  - `restart_count` 和 `ufcs_en` 状态变化是否能解释?
- [ ] RISE 期间 2s 间隔 vs CV 期间 ~480ms 间隔的差异原因

**E3. CV/衰减阶段** (strace 行 2945-19488):
- [ ] 电流从 8000 逐步降到 1000，步长 50-100mA，间隔 ~480ms
- [ ] 当前 CV 逻辑 (只限制上限，无主动递减) 能否产生这种衰减模式?
- [ ] 还是说衰减由 bcc_parms 中的某个字段驱动?

**E4. TC 保持阶段** (strace 行 20869-20978):
- [ ] 1000mA 持续约 8 秒 (28 次写入 × ~480ms)
- [ ] 当前 TC 逻辑是否匹配?

**E5. 充电周期重启** (strace 行 521-880):
- [ ] 触发条件 `charge_status == 0 && in_charge_cycle` 是否与 strace 一致
- [ ] 重启后 current_ma=1000 是否与 RISE #2 起始 1300mA 一致?

**E6. 温控逻辑**:
- [ ] strace 中 temp 值变化: 342→343→344→352 (0.1°C 单位, 即 34.2→35.2°C)
- [ ] 温控偏移是否在 strace 电流变化中可见?

#### F. `log_write()` (charging.c 约 25-33 行)
- [ ] 是否正确写入 stdout + /data/opbatt/battchg.log
- [ ] strace 行 72 确认 write(1</data/opbatt/battchg.log>, ...)

**输出格式**:
```markdown
# Agent 1 审查报告: charging.c

## 总结
(一句话: 整体一致/存在 N 处不一致/推测性代码过多)

## 逐项审查结果
### A. charging_parse_bcc_parms — ✅/❓/❌
(具体发现)

### B. charging_parse_ufcs_voters — ✅/❓/❌
...

## 发现汇总表
| # | 函数 | 行号 | 严重度 | 类型 | strace 证据 | 建议 |
|---|------|------|--------|------|------------|------|

## 置信度统计
- ✅ strace 确认: N 处
- ❓ 推测性代码: N 处
- ❌ 与 strace 不一致: N 处
```

---

### 9.2 Agent 4 未完成任务: 代码 vs strace 交叉验证

**目标**: 模拟代码执行路径，逐步对照 strace 实际 syscall 序列，找出所有不一致。

**输入文件**:
- `src/` 全部源文件
- `tmp/strace_log/strace_20260427_162732.log` (21171 行)
- `FINAL_REVERSE_REPORT.md` 第八节

**方法**:

#### Phase 1: 提取 strace 按 PID 分组的 syscall 序列

用 grep 按 PID 分组提取关键 syscall:
```bash
# PID 15505 (USB 监控)
grep '^15505 ' strace.log | grep -E 'read|write|nanosleep|openat' | head -50

# PID 15506 (充电控制)
grep '^15506 ' strace.log | grep -E 'read|write|nanosleep|openat' | head -80

# PID 15507 (电池日志)
grep '^15507 ' strace.log | grep -E 'read|write|nanosleep|openat' | head -30
```

#### Phase 2: 模拟代码执行，逐步对照

**2a. 启动序列** (strace 行 1-42):
- 3 个线程几乎同时启动
- PID 15505 先运行 (打开所有 fd + 重置 votable)
- PID 15506 等待 charging_active (通过 nanosleep 2s 表现)
- PID 15507 开始 5s 轮询
- **对照 main.c**: 线程创建顺序、charging_thread_wrapper 等待逻辑

**2b. 充电器插入** (strace 行 27-28):
- PID 15505 读 usb/online 从 "0" 变 "1"
- PID 15505 立即打开 sysfs 节点 (openat 行 29-38)
- **对照 monitor.c**: usb_online=1 → charging_active=1 的逻辑

**2c. 首次充电初始化** (strace 行 39-76):
- PID 15505: votable 重置 (写 "0" × 4)
- PID 15506: 开始运行 (之前在 sleep)
- PID 15506: 读 battery_log → chip_soc → ufcs_status(×3) → adapter_power → bcc_current → mmi_charging_enable → PPS/UFCS force
- PID 15506: 读 bcc_parms → temp → 写 force_val=500 → force_val=1750
- **对照 charging.c**: charging_loop 初始化阶段

**2d. 充电重置** (strace 行 521-880):
- 逐行对照 charging_dumpsys_reset() 的每个 syscall
- 特别关注 sleep 时序

**2e. RISE #2** (strace 行 1076-1155):
- 每 ~480ms 一次 read(bcc_parms)+read(temp)+write(force_val)
- 步长恒定 800mA
- **对照**: 代码的 RISE 逻辑能否产生这个模式

**2f. CV 衰减** (strace 行 2945-19488):
- 提取 force_val 值序列和时间戳
- 计算衰减速率和步长
- **对照**: 代码的 CV 逻辑

#### Phase 3: 输出差异表

```markdown
# Agent 4 交叉验证报告

## 时序验证

### 启动序列
| strace 行为 | 代码对应 | 一致? |
|------------|---------|-------|
| ... | ... | ✅/❌ |

### 充电重置 (strace 行 521-880)
| 步骤 | strace syscall | 时间 | 代码函数 | 一致? |
|------|---------------|------|---------|-------|
| 1 | write force_val="0" | 16:28:23.600 | sysfs_reset_votables | ✅ |
| ... | ... | ... | ... | ... |

### RISE #2 (strace 行 1076-1155)
| strace 时间 | force_val | 代码预期 | 差异 |
|------------|-----------|---------|------|
| 16:28:35.598 | 1300 | ? | ? |
| ... | ... | ... | ... |

### CV 衰减 (strace 行 2945-19488)
(衰减速率、步长、间隔分析)

## 差异汇总
| # | 严重度 | strace 行为 | 代码行为 | 影响 | 建议 |
|---|--------|------------|---------|------|------|
| 1 | 高/中/低 | ... | ... | ... | ... |

## 置信度统计
```

---

### 9.3 已完成的审查 (Agent 2+3) 及修复

以下 bug 已在 commit `4baaf0e` 中修复:

| 发现 | 来源 | 修复 |
|------|------|------|
| dumpsys reset 后缺少 sleep(2) | Agent 3 | ✅ 已添加 |
| ufcs_status 持久 fd 从未使用 | Agent 2 | ✅ 已移除 |

Agent 2 的误报: "缺少充电控制线程 PID 15506" — 实际在 `charging.c` 的 `charging_loop()` 中，由 `main.c` 的 `charging_thread_wrapper` 创建。

### 9.4 Agent 2+3 发现的待处理项 (低优先级)

| 发现 | 来源 | 严重度 | 说明 |
|------|------|--------|------|
| config_dump 遗漏 13 个已解析键 | Agent 3 | 低 | 不影响功能，仅影响调试输出 |
| config key 前缀匹配可能误匹配 | Agent 3 | 低 | extract_value 用 strncmp，如 "inc_step" 可能匹配 "batt_inc_step" |
| 线程退出未重置 charging_active | Agent 3 | 中 | 信号处理场景可能残留状态 |
| sysfs_close_all 依赖 struct 内存布局 | Agent 3 | 低 | 当前正确但脆弱 |
