mod cli;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use batt_config::{BattConfig, CONFIG_PATH};
use batt_monitor::{SharedState, monitor_battery_log_thread, monitor_usb_thread};

use cli::{CliMode, charging_thread_wrapper, cli_exec, cli_parse};

/* ------------------------------------------------------------------ */
/* 信号处理                                                            */
/* ------------------------------------------------------------------ */

/// 全局运行标志，信号处理函数可直接访问
static RUNNING: AtomicBool = AtomicBool::new(true);

extern "C" fn sighandler(_sig: i32) {
    RUNNING.store(false, Ordering::Release);
}

/// 注册 SIGINT / SIGTERM / SIGPIPE 信号处理。
///
/// # Safety
///
/// 调用 `libc::signal` 属于 unsafe FFI，仅在 `main` 入口处调用一次。
unsafe fn setup_signals() {
    unsafe {
        libc::signal(libc::SIGINT, sighandler as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, sighandler as *const () as libc::sighandler_t);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
}

/* ------------------------------------------------------------------ */
/* 配置加载                                                            */
/* ------------------------------------------------------------------ */

fn load_config() -> BattConfig {
    match BattConfig::parse(CONFIG_PATH) {
        Ok(cfg) => cfg,
        Err(_) => BattConfig {
            enabled: 1,
            adjust_step: 50,
            inc_step: 100,
            ufcs_max: 9100,
            pps_max: 5000,
            loop_interval_ms: 2000,
            restart_rise_step: 50,
            depol_pulse_ma: 500,
            depol_zero_ma: 0,
            depol_neg_step: 150,
            ..Default::default()
        },
    }
}

/* ------------------------------------------------------------------ */
/* main                                                                */
/* ------------------------------------------------------------------ */

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = cli_parse().map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    // 一次性命令模式: 执行后直接退出
    if args.mode != CliMode::Service {
        let cfg = load_config();
        cli_exec(&args, &cfg).map_err(Into::<Box<dyn std::error::Error>>::into)?;
        return Ok(());
    }

    // 服务模式
    let cfg = load_config();
    cfg.dump();

    // 安装信号处理
    // SAFETY: 仅在 main 入口处调用一次
    unsafe {
        setup_signals();
    }

    let state = Arc::new(SharedState::new(cfg, &RUNNING));

    // Thread 1: USB 在线监控
    let state_usb = Arc::clone(&state);
    let handle_usb = thread::Builder::new()
        .name("usb_monitor".into())
        .spawn(move || monitor_usb_thread(&state_usb))
        .map_err(|e| format!("spawn usb_monitor: {e}"))?;

    // Thread 2: 充电控制
    let state_chg = Arc::clone(&state);
    let handle_chg = thread::Builder::new()
        .name("charging".into())
        .spawn(move || {
            // 从共享状态取出 config 的快照（避免长期持有 Mutex 锁）
            let cfg_snapshot = match state_chg.config.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            }
            .clone();
            charging_thread_wrapper(&cfg_snapshot, state_chg.running);
        })
        .map_err(|e| format!("spawn charging: {e}"))?;

    // Thread 3: 电池日志监控
    let state_log = Arc::clone(&state);
    let handle_log = thread::Builder::new()
        .name("battery_log".into())
        .spawn(move || monitor_battery_log_thread(&state_log))
        .map_err(|e| format!("spawn battery_log: {e}"))?;

    join_and_report("usb_monitor", handle_usb);
    join_and_report("charging", handle_chg);
    join_and_report("battery_log", handle_log);

    Ok(())
}

fn join_and_report(name: &str, h: thread::JoinHandle<()>) {
    if let Err(e) = h.join() {
        eprintln!("thread '{name}' panicked: {e:?}");
    }
}
