mod aot_compiler;
mod env_probe;
mod event_listener;
mod extreme_optimizer;
mod hw_probe;
mod shizuku;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::signal::unix::{signal, SignalKind};

use extreme_optimizer::OperationalMode;

fn get_pid_file_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/data/data/com.termux/files/home".to_string());
    PathBuf::from(home).join(".rust-android-optimizer.pid")
}

fn get_log_file_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/data/data/com.termux/files/home".to_string());
    PathBuf::from(home).join(".rust-android-optimizer.log")
}

fn rotate_log_if_needed(log_path: &std::path::Path) {
    if let Ok(metadata) = fs::metadata(log_path) {
        if metadata.len() > 2 * 1024 * 1024 {
            let mut old_path = log_path.to_path_buf();
            old_path.set_extension("log.old");
            let _ = fs::rename(log_path, old_path);
        }
    }
}

pub fn get_mode_file_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/data/data/com.termux/files/home".to_string());
    PathBuf::from(home).join(".rust-android-optimizer.mode")
}

pub fn read_operational_mode() -> OperationalMode {
    let mode_path = get_mode_file_path();
    if let Ok(content) = fs::read_to_string(&mode_path) {
        match content.trim().to_lowercase().as_str() {
            "performance" | "perf" | "2" => OperationalMode::Performance,
            _ => OperationalMode::Adaptive,
        }
    } else {
        OperationalMode::Adaptive
    }
}

fn read_running_pid() -> Option<i32> {
    let pid_path = get_pid_file_path();
    if !pid_path.exists() {
        return None;
    }
    let content = fs::read_to_string(&pid_path).ok()?;
    let pid: i32 = content.trim().parse().ok()?;
    // Check if process is alive via kill(pid, 0)
    let res = unsafe { libc::kill(pid, 0) };
    if res == 0 {
        Some(pid)
    } else {
        // Stale pid file
        let _ = fs::remove_file(&pid_path);
        None
    }
}

fn ensure_termux_boot_script() {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/data/data/com.termux/files/home".to_string());
    let boot_dir = PathBuf::from(home).join(".termux").join("boot");
    if boot_dir.exists() || fs::create_dir_all(&boot_dir).is_ok() {
        let script_path = boot_dir.join("start-rust-optimizer.sh");
        let content = r#"#!/data/data/com.termux/files/usr/bin/bash
# Rust Android Optimizer - Auto-Start on Boot
termux-wake-lock 2>/dev/null || true
for i in $(seq 1 30); do
    if [ "$(getprop sys.boot_completed 2>/dev/null)" = "1" ]; then
        break
    fi
    sleep 1
done
sleep 5
rust-android-optimizer start >/dev/null 2>&1
"#;
        if fs::write(&script_path, content).is_ok() {
            let _ = fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755));
        }
    }
}

async fn apply_daemon_protection(pid: i32) {
    let script = format!(
        "echo -900 > /proc/{pid}/oom_score_adj 2>/dev/null; \
         dumpsys deviceidle whitelist +com.termux 2>/dev/null; \
         dumpsys deviceidle whitelist +moe.shizuku.privileged.api 2>/dev/null; \
         cmd appops set com.termux RUN_IN_BACKGROUND allow 2>/dev/null; \
         cmd appops set com.termux RUN_ANY_IN_BACKGROUND allow 2>/dev/null; \
         cmd appops set moe.shizuku.privileged.api RUN_IN_BACKGROUND allow 2>/dev/null; \
         cmd appops set moe.shizuku.privileged.api RUN_ANY_IN_BACKGROUND allow 2>/dev/null"
    );
    let _ = shizuku::exec(&script).await;
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let subcommand = args.get(1).map(|s| s.as_str()).unwrap_or("daemon");

    match subcommand {
        "start" => handle_cmd_start().await,
        "stop" => handle_cmd_stop().await,
        "status" => handle_cmd_status().await,
        "bench" | "benchmark" => handle_cmd_bench().await,
        "daemon" | "run" => handle_cmd_daemon().await,
        "--help" | "-h" | "help" => print_help(),
        other => {
            eprintln!("[ERROR] Unknown command: {}", other);
            print_help();
            std::process::exit(1);
        }
    }
}

fn print_help() {
    println!("=== Rust Android Optimizer v0.3.0 ===");
    println!("Usage: rust-android-optimizer <command>");
    println!();
    println!("Commands:");
    println!("  start      Start the optimizer daemon in background");
    println!("  stop       Stop the running daemon and restore system");
    println!("  status     Check daemon running state, mode and PID");
    println!("  bench      Run hardware probe, display detection and latency benchmark");
    println!("  daemon     Run daemon directly in foreground");
    println!("  help       Show this help message");
}

async fn handle_cmd_start() {
    if let Some(pid) = read_running_pid() {
        println!("[INFO] Optimizer daemon is already running (PID: {}).", pid);
        return;
    }

    if !shizuku::is_available() {
        eprintln!("[FATAL] Shizuku rish not found at {}", shizuku::RISH_PATH);
        eprintln!("        Please install Shizuku and grant Termux access.");
        std::process::exit(1);
    }

    let exe_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("rust-android-optimizer"));
    let log_path = get_log_file_path();
    let mode = read_operational_mode();

    // Prevent Android from sleeping Termux CPU while daemon is active
    let _ = std::process::Command::new("termux-wake-lock").spawn();
    ensure_termux_boot_script();
    rotate_log_if_needed(&log_path);

    let log_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open(&log_path)
        .expect("Failed to open daemon log file");

    let mut cmd = std::process::Command::new(exe_path);
    cmd.arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(log_file.try_clone().unwrap())
        .stderr(log_file);

    // Completely detach child process from controlling terminal session via setsid()
    // and ignore SIGHUP so closing the terminal never kills the daemon.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
            Ok(())
        });
    }

    let child = cmd.spawn();

    match child {
        Ok(child) => {
            let pid = child.id() as i32;
            let pid_path = get_pid_file_path();
            let _ = fs::write(&pid_path, pid.to_string());

            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            println!("==================================================");
            println!("[OK] Rust Android Optimizer daemon started!");
            println!("     PID     : {}", pid);
            println!("     Mode    : {}", mode);
            println!("     Log File: {}", log_path.display());
            println!("==================================================");
            println!("Commands available:");
            println!("  rust-optimizer-status  -> Check daemon state");
            println!("  rust-optimizer-stop    -> Stop daemon & restore system");
        }
        Err(e) => {
            eprintln!("[ERROR] Failed to start daemon in background: {}", e);
            std::process::exit(1);
        }
    }
}

async fn handle_cmd_stop() {
    let pid_path = get_pid_file_path();
    if let Some(pid) = read_running_pid() {
        println!("[INFO] Stopping daemon (PID: {})...", pid);
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }

        // Wait up to 3 seconds for clean exit
        for _ in 0..30 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            let res = unsafe { libc::kill(pid, 0) };
            if res != 0 {
                break;
            }
        }

        let _ = fs::remove_file(&pid_path);
        let _ = std::process::Command::new("termux-wake-unlock").spawn();
        println!("[OK] Daemon stopped successfully. System settings restored.");
    } else {
        let _ = fs::remove_file(&pid_path);
        println!("[INFO] Daemon is not running.");
    }
}

async fn handle_cmd_status() {
    println!("=== Rust Android Optimizer - Status ===");
    let mode = read_operational_mode();
    if let Some(pid) = read_running_pid() {
        println!("State   : RUNNING");
        println!("PID     : {}", pid);
        println!("Mode    : {}", mode);
        println!("Log File: {}", get_log_file_path().display());
        println!("Binary  : rust-android-optimizer v0.3.0");
    } else {
        println!("State   : STOPPED");
        println!("Mode    : {}", mode);
        println!("PID file: None");
    }
}

async fn handle_cmd_bench() {
    println!("========================================================================");
    println!("            RUST ANDROID OPTIMIZER - BENCHMARK & HARDWARE PROBE        ");
    println!("========================================================================");

    if !shizuku::is_available() {
        eprintln!("[FATAL] Shizuku rish not found at {}", shizuku::RISH_PATH);
        eprintln!("        Please install Shizuku and grant Termux access.");
        return;
    }

    let profile = env_probe::HardwareProfile::probe().await;
    let mode = read_operational_mode();

    println!("[DEVICE IDENTIFICATION]");
    println!("  Manufacturer : {}", profile.manufacturer);
    println!("  Model        : {}", profile.model);
    println!("  SoC Vendor   : {}", profile.soc_vendor);
    println!("  Platform     : {}", profile.platform);
    println!("  OEM / ROM    : {}", profile.oem_flavor);
    println!("  Android OS   : Android {} (API Level {})", profile.android_release, profile.android_api);
    println!("  Active Mode  : {}", mode);
    println!();

    println!("[DISPLAY DETECTION]");
    println!("  Supported Hz : {:?}", profile.display.supported_rates);
    println!("  Max Lock Hz  : {:.1} Hz", profile.display.max_refresh_rate);
    println!();

    println!("[FEATURE COMPATIBILITY MATRIX]");
    println!("  [+] Shizuku / ADB Access     : {}", if profile.features.shizuku_active { "ACTIVE (uid=2000/0)" } else { "DISABLED" });
    println!("  [+] Fixed Performance Mode   : {}", if profile.features.fixed_performance_mode { "SUPPORTED (API >= 30)" } else { "UNSUPPORTED" });
    println!("  [+] Thermal Override (0)     : {}", if profile.features.thermal_override { "SUPPORTED" } else { "UNSUPPORTED" });
    println!("  [+] Android Game Mode API    : {}", if profile.features.game_mode_api { "SUPPORTED (API >= 31)" } else { "UNSUPPORTED" });
    println!("  [+] Doze Background Freeze   : {}", if profile.features.doze_force_idle { "SUPPORTED" } else { "UNSUPPORTED" });
    println!("  [+] Dynamic Refresh Lock     : {}", if profile.features.display_rate_lock { "SUPPORTED" } else { "UNSUPPORTED" });
    println!("  [+] Touch Latency Reduction  : {}", if profile.features.touch_latency_flags { "SUPPORTED" } else { "UNSUPPORTED" });
    println!("  [+] WiFi Latency Tuning      : {}", if profile.features.wifi_power_save_flag { "SUPPORTED" } else { "UNSUPPORTED" });
    println!("  [+] RAM Plus / Swap Control  : {}", if profile.features.ram_expansion_control { "SUPPORTED" } else { "UNSUPPORTED" });
    println!();

    println!("[MICRO-BENCHMARK]");
    // Shizuku IPC latency test
    let start = Instant::now();
    for _ in 0..5 {
        let _ = shizuku::exec("id").await;
    }
    let ipc_avg = start.elapsed().as_micros() as f64 / 5.0;
    println!("  Shizuku IPC Round-trip (avg) : {:.2} ms", ipc_avg / 1000.0);

    // Sysfs direct pread latency
    let start_sysfs = Instant::now();
    for _ in 0..100 {
        let _ = hw_probe::get_gpu_clock();
    }
    let sysfs_avg = start_sysfs.elapsed().as_nanos() as f64 / 100.0;
    println!("  Sysfs Direct pread Latency   : {:.2} ns", sysfs_avg);

    println!("========================================================================");
}

async fn handle_cmd_daemon() {
    // Ignore SIGHUP so terminal closing/hangup never kills the daemon
    unsafe {
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
    }

    println!("=== Rust Android Optimizer v0.3.0 - Daemon ===");
    println!("Target: aarch64-linux-android (native)");

    // Pre-flight checks
    if !shizuku::is_available() {
        eprintln!("[FATAL] Shizuku rish not found at {}", shizuku::RISH_PATH);
        eprintln!("        Please install Shizuku and grant Termux access.");
        std::process::exit(1);
    }
    println!("[OK] Shizuku rish available");

    // Write PID file
    let pid = std::process::id() as i32;
    let pid_path = get_pid_file_path();
    let _ = fs::write(&pid_path, pid.to_string());

    // Hardware, mode and environment probe
    println!("[INIT] Probing hardware, display and OS environment...");
    let profile = env_probe::HardwareProfile::probe().await;
    let mode = read_operational_mode();
    println!("[HW] Device: {} {} ({})", profile.manufacturer, profile.model, profile.soc_vendor);
    println!("[HW] OS    : Android {} (API Level {}) - {}", profile.android_release, profile.android_api, profile.oem_flavor);
    println!("[HW] Screen: Max refresh rate detected at {:.1} Hz", profile.display.max_refresh_rate);
    println!("[INIT] Operational Mode: {}", mode);

    // Disable Phantom Process Killer on Android 12+ (API 31+) to safeguard background daemon and compiler workers
    if profile.android_api >= 31 {
        let _ = shizuku::exec("/system/bin/device_config put activity_manager max_phantom_processes 2147483647; /system/bin/device_config set_sync_disabled_for_tests persistent; setprop persist.sys.fflag.override.settings_enable_monitor_phantom_procs false").await;
        println!("[OK] Phantom Process Killer disabled (max_phantom_processes=2147483647)");
    }

    // Apply Anti-OOM Kernel Protection (-900) and Battery Whitelisting for Termux & Shizuku
    apply_daemon_protection(pid).await;
    ensure_termux_boot_script();
    let _ = std::process::Command::new("termux-wake-lock").spawn();
    println!("[OK] Anti-OOM Kernel Protection (-900) and Battery Keepalive active");

    // Watchdog keepalive task: periodically refreshes OOM protection & wake lock every 15 minutes
    let watchdog_pid = pid;
    let _watchdog_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(900));
        loop {
            interval.tick().await;
            let _ = std::process::Command::new("termux-wake-lock").spawn();
            apply_daemon_protection(watchdog_pid).await;
            let log_path = get_log_file_path();
            rotate_log_if_needed(&log_path);
        }
    });

    // Game category cache (shared lock-free reads via RwLock + FxHashMap)
    let game_cache: Arc<parking_lot::RwLock<rustc_hash::FxHashMap<Box<str>, bool>>> =
        Arc::new(parking_lot::RwLock::new(rustc_hash::FxHashMap::default()));

    // Initialize AOT compiler (discovers packages via Shizuku)
    println!("[INIT] Discovering packages for AOT compilation...");
    let compiler = Arc::new(tokio::sync::Mutex::new(aot_compiler::AotCompiler::new().await));

    // Initialize Extreme Optimizer with detected profile & operational mode
    let optimizer = Arc::new(extreme_optimizer::ExtremeOptimizer::new(profile, mode));

    // In Performance mode, ensure system baseline tweaks are applied immediately
    if mode == OperationalMode::Performance {
        println!("[INIT] Performance mode: Applying baseline optimizations...");
        optimizer.apply_optimizations(None).await;
    }

    let compiler_handle = {
        let lock = compiler.lock().await;
        aot_compiler::CompilerHandle {
            suspended: lock.get_suspended_flag(),
            abort_notify: lock.get_abort_notify(),
            notify: lock.get_notify(),
            current_package: lock.get_current_package_arc(),
        }
    };
    let compiler_handle = Arc::new(compiler_handle);

    // Spawn the AOT compilation task (runs in background)
    let compiler_run = compiler.clone();
    let _compile_task = tokio::spawn(async move {
        let mut lock = compiler_run.lock().await;
        lock.run().await;
    });

    // Spawn the event loop (monitors logcat, controls compiler + doze)
    let event_handle = compiler_handle.clone();
    let event_optimizer = optimizer.clone();
    let event_cache = game_cache.clone();
    let event_task = tokio::spawn(async move {
        event_listener::run_event_loop(event_handle, event_optimizer, event_cache).await;
    });

    println!("[DAEMON] Running... Monitoring game activity.");
    println!("[DAEMON] AOT compilation started in background.");

    // Signal listeners for graceful termination
    let mut sigterm = signal(SignalKind::terminate()).expect("Failed to register SIGTERM handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("Failed to register SIGINT handler");

    let optimizer_cleanup = optimizer.clone();

    tokio::select! {
        _ = sigterm.recv() => {
            println!("[DAEMON] SIGTERM received. Restoring system and shutting down...");
        }
        _ = sigint.recv() => {
            println!("[DAEMON] SIGINT (Ctrl+C) received. Restoring system and shutting down...");
        }
        _ = event_task => {
            println!("[DAEMON] Event loop terminated unexpectedly.");
        }
    }

    // Teardown: ensure full system settings are restored strictly to snapshot
    optimizer_cleanup.full_restore_on_shutdown().await;
    let _ = fs::remove_file(&pid_path);
    println!("[DAEMON] Exited cleanly.");
}
