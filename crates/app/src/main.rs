mod cli;

use std::sync::Arc;
use std::thread;

use batt_config::{BattConfig, CONFIG_PATH};
use batt_monitor::{SharedState, monitor_battery_log_thread, monitor_usb_thread};

use cli::{CliMode, charging_thread_wrapper, cli_exec, cli_parse};

/* ------------------------------------------------------------------ */
/* 信号处理                                                            */
/* ------------------------------------------------------------------ */

extern "C" fn sighandler(_sig: i32) {
    // 信号终止使用 exit(3) 区分于正常退出
    unsafe {
        libc::_exit(3);
    }
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
    BattConfig::parse(CONFIG_PATH).unwrap_or_else(|_| BattConfig {
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
    })
}

/* ------------------------------------------------------------------ */
/* main                                                                */
/* ------------------------------------------------------------------ */

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = cli_parse().map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    // 一次性命令模式: 执行后直接退出
    if args.mode != CliMode::Service {
        let cfg = load_config();
        cli_exec(&args, &cfg).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
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

    let state = Arc::new(SharedState::new(cfg));

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
            let cfg_snapshot = state_chg
                .config
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            charging_thread_wrapper(&cfg_snapshot, &state_chg.running);
        })
        .map_err(|e| format!("spawn charging: {e}"))?;

    // Thread 3: 电池日志监控
    let state_log = Arc::clone(&state);
    let handle_log = thread::Builder::new()
        .name("battery_log".into())
        .spawn(move || monitor_battery_log_thread(&state_log))
        .map_err(|e| format!("spawn battery_log: {e}"))?;

    let _ = handle_usb.join();
    let _ = handle_chg.join();
    let _ = handle_log.join();

    Ok(())
}
