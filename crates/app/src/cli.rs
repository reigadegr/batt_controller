use batt_charging::{dumpsys_reset, dumpsys_set_ac, dumpsys_set_status, run as charging_loop};
use batt_config::BattConfig;
use batt_sysfs::{SysfsFds, read_int, write_int, write_proc_int, write_proc_str};

/* ------------------------------------------------------------------ */
/* CLI 运行模式                                                        */
/* ------------------------------------------------------------------ */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliMode {
    /// `--service` / `-S`: 进入服务模式 (默认)
    Service,
    /// `-c <mA>`: 设置 BCC 充电电流
    Charge,
    /// `-t`: 读取电池温度
    Temp,
    /// `-s`: 读取芯片 `SoC`
    Soc,
    /// `-p`: 读取适配器功率
    Power,
    /// `-e <0|1>` / `-d`: 使能/禁用充电
    Enable,
    /// `-P <mA>`: 强制 PPS 电流
    Pps,
    /// `-u <mA>`: 强制 UFCS 电流
    Ufcs,
    /// `-l`: 抓取内核充电日志
    Log,
    /// `-D`: dumpsys 电池控制
    Dumpsys,
    /// `-A`: dumpsys battery set ac 1
    DumpsysSetAc,
    /// `-T`: dumpsys battery set status 2
    DumpsysSetStatus,
    /// `-m <name>`: 查询电池型号
    Model,
}

/* ------------------------------------------------------------------ */
/* CLI 解析结果                                                        */
/* ------------------------------------------------------------------ */

#[derive(Debug, Clone)]
pub struct CliArgs {
    pub mode: CliMode,
    pub value: i32,
    pub model: String,
}

impl Default for CliArgs {
    fn default() -> Self {
        Self {
            mode: CliMode::Service,
            value: 0,
            model: String::new(),
        }
    }
}

/* ------------------------------------------------------------------ */
/* 电池型号表 (15 个型号，来自 .rodata 逆向)                             */
/* ------------------------------------------------------------------ */

struct BatteryModel {
    name: &'static str,
    index: i32,
    param: i32,
}

const BATTERY_MODELS: &[BatteryModel] = &[
    BatteryModel {
        name: "B-163",
        index: 0,
        param: 163,
    },
    BatteryModel {
        name: "B-233",
        index: 1,
        param: 233,
    },
    BatteryModel {
        name: "B-283",
        index: 2,
        param: 283,
    },
    BatteryModel {
        name: "B-409",
        index: 3,
        param: 409,
    },
    BatteryModel {
        name: "B-571",
        index: 4,
        param: 571,
    },
    BatteryModel {
        name: "K-163",
        index: 5,
        param: 163,
    },
    BatteryModel {
        name: "K-233",
        index: 6,
        param: 233,
    },
    BatteryModel {
        name: "K-283",
        index: 7,
        param: 283,
    },
    BatteryModel {
        name: "K-409",
        index: 8,
        param: 409,
    },
    BatteryModel {
        name: "K-571",
        index: 9,
        param: 571,
    },
    BatteryModel {
        name: "P-192",
        index: 10,
        param: 192,
    },
    BatteryModel {
        name: "P-224",
        index: 11,
        param: 224,
    },
    BatteryModel {
        name: "P-256",
        index: 12,
        param: 256,
    },
    BatteryModel {
        name: "P-384",
        index: 13,
        param: 384,
    },
    BatteryModel {
        name: "P-521",
        index: 14,
        param: 521,
    },
];

/* ------------------------------------------------------------------ */
/* CLI 解析                                                            */
/* ------------------------------------------------------------------ */

/// 解析命令行参数。成功返回 `Ok(CliArgs)`，失败返回 `Err`。
///
/// # Errors
///
/// 当遇到未知选项或 `-c`/`-e`/`-P`/`-u`/`-m` 缺少必需参数时返回错误。
#[allow(clippy::cast_possible_wrap)]
pub fn cli_parse() -> Result<CliArgs, String> {
    let raw: Vec<String> = std::env::args().collect();
    let mut args = CliArgs::default();
    let mut pos = 1usize; // 跳过 argv[0]

    while pos < raw.len() {
        if raw[pos] == "--" {
            break;
        }
        if raw[pos].starts_with("--") {
            parse_long(&raw, &mut pos, &mut args)?;
        } else if raw[pos].starts_with('-') {
            parse_short_opts(&raw, &mut pos, &mut args)?;
        }
    }

    Ok(args)
}

/// 从 `raw[pos]` 解析一个长选项，推进 `pos`。
fn parse_long(raw: &[String], pos: &mut usize, args: &mut CliArgs) -> Result<(), String> {
    let long = &raw[*pos][2..];
    *pos += 1;
    match long {
        "charge" => {
            let v = next_arg(raw, pos, "--charge")?
                .parse::<i32>()
                .map_err(|_| "--charge: invalid integer")?;
            args.mode = CliMode::Charge;
            args.value = v;
        }
        "temp" => args.mode = CliMode::Temp,
        "soc" => args.mode = CliMode::Soc,
        "power" => args.mode = CliMode::Power,
        "enable" => {
            let v = next_arg(raw, pos, "--enable")?
                .parse::<i32>()
                .map_err(|_| "--enable: invalid integer")?;
            args.mode = CliMode::Enable;
            args.value = v;
        }
        "disable" => {
            args.mode = CliMode::Enable;
            args.value = 0;
        }
        "pps" => {
            let v = next_arg(raw, pos, "--pps")?
                .parse::<i32>()
                .map_err(|_| "--pps: invalid integer")?;
            args.mode = CliMode::Pps;
            args.value = v;
        }
        "ufcs" => {
            let v = next_arg(raw, pos, "--ufcs")?
                .parse::<i32>()
                .map_err(|_| "--ufcs: invalid integer")?;
            args.mode = CliMode::Ufcs;
            args.value = v;
        }
        "log" => args.mode = CliMode::Log,
        "dumpsys" => args.mode = CliMode::Dumpsys,
        "set-ac" => args.mode = CliMode::DumpsysSetAc,
        "set-status" => args.mode = CliMode::DumpsysSetStatus,
        "model" => {
            next_arg(raw, pos, "--model")?.clone_into(&mut args.model);
            args.mode = CliMode::Model;
        }
        "service" => args.mode = CliMode::Service,
        other => return Err(format!("unknown option: --{other}")),
    }
    Ok(())
}

/// 从 `raw[pos]` 解析短选项（支持合并），推进 `pos`。
fn parse_short_opts(raw: &[String], pos: &mut usize, args: &mut CliArgs) -> Result<(), String> {
    let item = &raw[*pos];
    let flag = item[1..].chars().next().ok_or("empty short flag")?;
    // 带参数的选项: 如果同一 token 有剩余字符则用之，否则取下一个 token
    let arg_str = |pos: &mut usize| -> Result<&str, String> {
        let rest = &item[2..];
        *pos += 1;
        if rest.is_empty() {
            // 空格分隔: -c 500 → 先跳过选项 token，再取下一个
            next_arg(raw, pos, &flag.to_string())
        } else {
            // 合并: -c500 → 直接用 rest
            Ok(rest)
        }
    };
    match flag {
        'c' => {
            args.mode = CliMode::Charge;
            args.value = arg_str(pos)?
                .parse::<i32>()
                .map_err(|_| "-c: invalid integer")?;
        }
        't' => {
            args.mode = CliMode::Temp;
            *pos += 1;
        }
        's' => {
            args.mode = CliMode::Soc;
            *pos += 1;
        }
        'p' => {
            args.mode = CliMode::Power;
            *pos += 1;
        }
        'e' => {
            args.mode = CliMode::Enable;
            args.value = arg_str(pos)?
                .parse::<i32>()
                .map_err(|_| "-e: invalid integer")?;
        }
        'd' => {
            args.mode = CliMode::Enable;
            args.value = 0;
            *pos += 1;
        }
        'P' => {
            args.mode = CliMode::Pps;
            args.value = arg_str(pos)?
                .parse::<i32>()
                .map_err(|_| "-P: invalid integer")?;
        }
        'u' => {
            args.mode = CliMode::Ufcs;
            args.value = arg_str(pos)?
                .parse::<i32>()
                .map_err(|_| "-u: invalid integer")?;
        }
        'l' => {
            args.mode = CliMode::Log;
            *pos += 1;
        }
        'D' => {
            args.mode = CliMode::Dumpsys;
            *pos += 1;
        }
        'A' => {
            args.mode = CliMode::DumpsysSetAc;
            *pos += 1;
        }
        'T' => {
            args.mode = CliMode::DumpsysSetStatus;
            *pos += 1;
        }
        'm' => {
            arg_str(pos)?.clone_into(&mut args.model);
            args.mode = CliMode::Model;
        }
        'S' => {
            args.mode = CliMode::Service;
            *pos += 1;
        }
        other => return Err(format!("unknown option: -{other}")),
    }
    Ok(())
}

/// 取 `raw[pos]` 作为参数值并推进 `pos`。
fn next_arg<'a>(raw: &'a [String], pos: &mut usize, flag: &str) -> Result<&'a str, String> {
    *pos += 1;
    raw.get(*pos - 1)
        .map(String::as_str)
        .ok_or_else(|| format!("{flag} requires an argument"))
}

/* ------------------------------------------------------------------ */
/* 一次性命令执行                                                      */
/* ------------------------------------------------------------------ */

/// 执行一次性 CLI 命令（非服务模式）。
///
/// # Errors
///
/// 当命令执行失败时返回错误信息。
#[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
pub fn cli_exec(args: &CliArgs, _cfg: &BattConfig) -> Result<(), String> {
    match args.mode {
        CliMode::Temp => {
            let fds = SysfsFds::open_all().map_err(|_| "sysfs_open_all failed")?;
            let val = read_int(fds.battery_temp).unwrap_or(-1);
            println!("{val}");
        }
        CliMode::Soc => {
            let fds = SysfsFds::open_all().map_err(|_| "sysfs_open_all failed")?;
            let val = read_int(fds.chip_soc).unwrap_or(-1);
            println!("{val}");
        }
        CliMode::Power => {
            let fds = SysfsFds::open_all().map_err(|_| "sysfs_open_all failed")?;
            let val = read_int(fds.adapter_power).unwrap_or(-1);
            println!("{val}");
        }
        CliMode::Charge => {
            let fds = SysfsFds::open_all().map_err(|_| "sysfs_open_all failed")?;
            write_int(fds.bcc_current, args.value).map_err(|_| "write bcc_current failed")?;
            println!("bcc_current set to {} mA", args.value);
        }
        CliMode::Enable => {
            let fds = SysfsFds::open_all().map_err(|_| "sysfs_open_all failed")?;
            write_int(fds.mmi_charging_enable, args.value)
                .map_err(|_| "write mmi_charging_enable failed")?;
            println!("mmi_charging_enable set to {}", args.value);
        }
        CliMode::Pps => {
            write_proc_int(batt_sysfs::PROC_PPS_FORCE_VAL, args.value)
                .map_err(|_| "write PPS force_val failed")?;
            write_proc_str(batt_sysfs::PROC_PPS_FORCE_ACTIVE, "1")
                .map_err(|_| "write PPS force_active failed")?;
            println!("PPS force_val set to {} mA", args.value);
        }
        CliMode::Ufcs => {
            write_proc_int(batt_sysfs::PROC_UFCS_FORCE_VAL, args.value)
                .map_err(|_| "write UFCS force_val failed")?;
            write_proc_str(batt_sysfs::PROC_UFCS_FORCE_ACTIVE, "1")
                .map_err(|_| "write UFCS force_active failed")?;
            println!("UFCS force_val set to {} mA", args.value);
        }
        CliMode::Log => {
            let status = std::process::Command::new("sh")
                .args([
                    "-c",
                    "echo '== == == == == == == == == ==' \
                     >> /data/opbatt/kernellog/klog_$(date +%Y-%m-%d).log && \
                     dmesg -T | grep OPLUS_CHG \
                     >> /data/opbatt/kernellog/klog_$(date +%Y-%m-%d).log",
                ])
                .status()
                .map_err(|_| "failed to exec sh")?;
            if status.success() {
                println!("kernel log saved to /data/opbatt/kernellog/");
            } else {
                let code = status.code().unwrap_or(-1);
                return Err(format!("kernel log collection failed (exit {code})"));
            }
        }
        CliMode::Dumpsys => {
            dumpsys_reset();
            println!("dumpsys battery reset sequence complete");
        }
        CliMode::DumpsysSetAc => {
            dumpsys_set_ac();
            println!("dumpsys battery set ac 1");
        }
        CliMode::DumpsysSetStatus => {
            dumpsys_set_status();
            println!("dumpsys battery set status 2");
        }
        CliMode::Model => {
            if let Some(i) = BATTERY_MODELS.iter().position(|m| m.name == args.model) {
                let m = &BATTERY_MODELS[i];
                println!("model: {} (index={}, param={})", m.name, m.index, m.param);
            } else {
                let available: Vec<&str> = BATTERY_MODELS.iter().map(|m| m.name).collect();
                let joined = available.join(" ");
                return Err(format!(
                    "unknown battery model: {}\navailable models: {joined}",
                    args.model
                ));
            }
        }
        CliMode::Service => {
            // 服务模式由 main.rs 处理，不应到达这里
        }
    }
    Ok(())
}

/* ------------------------------------------------------------------ */
/* 服务模式充电线程包装                                                 */
/* ------------------------------------------------------------------ */

/// 充电控制线程包装函数。等待 USB 在线后打开 sysfs fd 并进入充电循环。
pub fn charging_thread_wrapper(config: &BattConfig, running: &std::sync::atomic::AtomicBool) {
    while running.load(std::sync::atomic::Ordering::Relaxed) {
        // 等待 USB 在线 (与原始二进制一致: 2s 轮询)
        while running.load(std::sync::atomic::Ordering::Relaxed)
            && !batt_sysfs::read_usb_online().unwrap_or(false)
        {
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        if !running.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }

        // USB 插入 → 打开 sysfs fd
        let Ok(mut fds) = SysfsFds::open_all() else {
            eprintln!("sysfs_open_all failed");
            std::thread::sleep(std::time::Duration::from_secs(2));
            continue;
        };

        // 进入充电控制主循环
        charging_loop(&mut fds, config, running);

        // USB 拔出后 run() 返回，fds 离开作用域时 Drop 自动关闭
    }
}
