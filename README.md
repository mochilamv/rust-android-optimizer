# rust-android-optimizer

High-performance, low-latency gaming optimization daemon and ART AOT compilation engine for Android (10 to 16+). Built in native Rust for `aarch64-linux-android` utilizing Shizuku / ADB privileged services.

Author: Mochilamv & IAs  
License: MIT  

---

## 1. Overview

`rust-android-optimizer` is a background daemon engineered to eliminate micro-stuttering (1% low FPS drops), reduce touch and input latency, and optimize system-level resource allocation during gaming sessions on Android devices.

The system dynamically adapts to the underlying hardware architecture (Qualcomm Snapdragon, MediaTek Dimensity/Helio, Samsung Exynos, Google Tensor, Unisoc) and OEM flavor (Xiaomi MIUI/HyperOS, Samsung OneUI, OnePlus/Oppo/Realme ColorOS/OxygenOS, Motorola HelloUI, Generic AOSP).

---

## 2. Core Architecture & Mechanisms

### A. Intelligent ART AOT Compilation Engine
* Discovers third-party user applications and critical system packages in a single pass.
* Compiles applications using ART's ahead-of-time compiler (`cmd package compile -m speed -f <pkg>`), converting bytecode directly to native machine instructions.
* Eliminates runtime JIT compilation overhead, reducing CPU interpreter contention.
* Automatically prioritizes discovered games first.
* Suspends background compilation instantly whenever a game enters the foreground.

### B. Adaptive Display & Refresh Rate Lock
* Probes physical display modes via `dumpsys display` and `dumpsys SurfaceFlinger`.
* Detects the maximum hardware refresh rate supported by the panel (60Hz, 90Hz, 120Hz, 144Hz, 165Hz+).
* Enforces minimum, peak, and user refresh rates to prevent dynamic downclocking of the display controller during gameplay.

### C. Extreme Gaming & Power State Tuning
* **Fixed Performance Mode**: Enables `cmd power set-fixed-performance-mode-enabled true` on supported Android versions (API 30+) to lock CPU/GPU operating frequencies.
* **Thermal Throttling Override**: Dispatches `cmd thermalservice override-status 0` to bypass userspace thermal governor downclocking during game sessions.
* **Android Game Mode API**: Enforces `cmd game mode performance` and target FPS limits on Android 12+ (API 31+).
* **Aggressive Doze Freezing**: Whitelists Termux (`dumpsys deviceidle whitelist +com.termux`) while forcing idle on all non-essential background processes (`dumpsys deviceidle force-idle`).
* **RAM Expansion / Swap Control**: Mitigates virtual RAM / ZRAM storage thrashing (`settings put global ram_expand_size 0`).

### D. Touch & Compositor Latency Reduction
* **SurfaceFlinger & HWUI**: Enables touch boosting (`debug.sf.boost_sf_on_touch`), disables render queue ahead (`debug.hwui.render_ahead 0`), and sets zero phase offsets (`debug.sf.high_fps_early_phase_offset_ns 0`).
* **Touch Subsystem**: Configures low-latency touch response (`touch_performance_mode 1`, `input_latency_reduction 1`, `pointer_speed 7`).

### E. Zero-overhead SIMD Event Loop
* Asynchronously monitors Android lifecycle events (`am_resume_activity` and `am_pause_activity`) via `logcat -b events`.
* Utilizes zero-copy string slicing and SIMD/NEON memory scanning (`memchr`) with branchless algorithms for minimal CPU footprint.

### F. Safe System Teardown & Signal Handling
* Intercepts `SIGINT` (Ctrl+C) and `SIGTERM` signals.
* Guaranteed execution of `restore_system()` on exit to revert thermal overrides, power modes, animation scales, and refresh rate constraints.

---

## 3. Prerequisites

* **Architecture**: Android `aarch64` / `arm64`.
* **Android OS**: Android 10 (API 29) up to Android 16+ (API 36+).
* **Shizuku**: Active Shizuku service with `rish` binary installed in Termux (`/data/data/com.termux/files/usr/bin/rish`).
* **Termux Environment**: Standard Termux installation with `rust` / `cargo` (automatically handled by `install.sh`).

---

## 4. Installation

Run the automated native installer inside the repository directory:

```bash
chmod +x install.sh
./install.sh
```

The installer will:
1. Validate architecture and Shizuku `rish` connectivity.
2. Compile the binary using native CPU vector optimizations (`RUSTFLAGS="-C target-cpu=native -C opt-level=3 -C lto=fat"`).
3. Install the executable to `$PREFIX/bin/rust-android-optimizer`.
4. Inject convenient shell aliases into `~/.bashrc` and `~/.zshrc`.
5. Optionally run the hardware capability benchmark.

---

## 5. Usage & Commands

After installation, the following shell commands are available:

### Start Daemon (Background)
```bash
rust-optimizer-start
```
*Starts the daemon in the background, writes PID to `~/.rust-android-optimizer.pid`, and redirects logs to `~/.rust-android-optimizer.log`.*

### Check Daemon Status
```bash
rust-optimizer-status
```
*Displays current operational state, process PID, binary version, and log path.*

### Stop Daemon & Restore System
```bash
rust-optimizer-stop
```
*Sends termination signal to the daemon, ensures full restoration of system thermal and power settings, and cleans up PID tracking.*

### Run Hardware Benchmark & Feature Probe
```bash
rust-android-optimizer bench
```
*Performs an in-depth probe of SoC, OEM ROM, Android API level, maximum screen refresh rate, Shizuku IPC round-trip latency, and feature compatibility matrix.*

---

## 6. License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

Copyright (c) 2026 Mochilamv
