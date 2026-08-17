# rust-android-optimizer

High-performance, low-latency gaming optimization daemon and ART AOT compilation engine for Android (10 to 16+). Built in native Rust for `aarch64-linux-android` utilizing Shizuku / ADB user-level framework services with zero root requirements.

Author: Mochilamv & IAs  
License: MIT  

---

## 1. Overview

`rust-android-optimizer` is a native background daemon engineered to eliminate micro-stuttering (1% low FPS drops), reduce touch and input latency, and optimize system-level resource allocation during gaming sessions on Android devices.

The system dynamically adapts to the underlying hardware architecture (Qualcomm Snapdragon, MediaTek Dimensity/Helio, Samsung Exynos, Google Tensor, Unisoc) and OEM flavor (Xiaomi MIUI/HyperOS, Samsung OneUI, OnePlus/Oppo/Realme ColorOS/OxygenOS, Motorola HelloUI/MyUX, Generic AOSP).

All optimizations operate strictly within standard Android userspace framework interfaces accessible via Shizuku (`dumpsys`, `cmd`, `settings`, `setprop debug.*`, `device_config`, `appops`), ensuring 100% functionality without requiring root permissions.

---

## 2. Core Architecture & Mechanisms

### A. Intelligent ART AOT Compilation Engine
* Discovers third-party user applications and critical system packages in a single pass.
* Compiles applications using ART's ahead-of-time compiler (`cmd package compile -m speed -f <pkg>`), converting bytecode directly into native machine instructions.
* Eliminates runtime JIT compilation overhead and eliminates CPU interpreter contention.
* Automatically prioritizes discovered games first.
* **Instant Interruption**: Suspends background compilation instantly whenever a game enters the foreground, executing immediate detached cleanup (`killall -9 dex2oat*`) to free CPU cores without losing queue progress.

### B. State Snapshot & Atomic Restoration Engine
* Captures a 25-property pre-optimization system state snapshot across `system`, `global`, and `secure` namespaces prior to applying tweaks.
* Reverts strictly to captured baseline values on shutdown or game exit, preventing blind defaults from overriding user preferences.

### C. Dual Operational Modes
* **Adaptive Mode (Default)**:
  * Full gaming tweaks applied exclusively while games are in the foreground.
  * Complete restoration to pre-daemon snapshot upon exiting the game.
* **Performance Mode**:
  * Unforces Doze on game exit, but retains maximum display refresh rate, zero touch debounce, disabled window blurs, and GPU latency optimizations globally.
* Operational mode is persistent via `~/.rust-android-optimizer.mode` and selectable during installation.

### D. Universal Multi-Window & Floating App Whitelisting
* Automatically inspects active window stacks via `dumpsys window visible-apps` and `dumpsys activity activities`.
* Dynamically detects Split-Screen, Freeform floating windows, and Picture-in-Picture (PiP) secondary apps (e.g. Discord, WhatsApp, Spotify, YouTube).
* Whitelists secondary visible user apps in Doze alongside Termux, ensuring background voice, chat, and music continue uninterrupted during gameplay.

### E. OEM Throttling Services Bypass (Non-Root AppOps)
* Temporarily ignores background execution for OEM thermal/throttling daemons during gaming sessions:
  * Samsung GOS (`com.samsung.android.game.gos`)
  * Xiaomi Joyose (`com.xiaomi.joyose`) & Powerkeeper (`com.miui.powerkeeper`)
  * OnePlus / Oppo Cosa (`com.oplus.cosa`) & Games (`com.oplus.games`)
  * Motorola GameMode (`com.motorola.gamemode.service`)
  * Transsion GameZone (`com.transsion.gamezone`)
* Restores normal AppOps state upon exiting the game.

### F. Extreme Gaming & Power State Tuning
* **Fixed Performance Mode**: Enables `cmd power set-fixed-performance-mode-enabled true` on supported Android versions (API 30+) to lock CPU/GPU operating frequencies.
* **Thermal Throttling Override**: Dispatches `cmd thermalservice override-status 0` to bypass userspace thermal governor downclocking during game sessions.
* **Android Game Mode API**: Enforces `cmd game mode performance` and target FPS limits on Android 12+ (API 31+) and `game_overlay` on Android 13+ (API 33+).
* **Aggressive Doze Freezing**: Whitelists active applications while forcing idle on non-essential background processes (`dumpsys deviceidle force-idle`).
* **RAM Expansion / Swap Control**: Mitigates virtual RAM / ZRAM storage thrashing (`settings put global ram_expand_size 0`).

### G. Touch & Compositor Latency Reduction
* **SurfaceFlinger & HWUI**: Enables touch boosting (`debug.sf.boost_sf_on_touch`), disables render queue ahead (`debug.hwui.render_ahead 0`), and configures zero phase offsets.
* **Touch Subsystem**: Configures low-latency touch response (`touch_performance_mode 1`, `input_latency_reduction 1`, `pointer_speed 7`, zero tap duration threshold).

### H. 24/7 Overnight Persistence & Anti-Kill Architecture
* **Kernel Anti-OOM Protection**: Lowers daemon process `oom_score_adj` to `-900` via Shizuku, protecting against Low Memory Killer termination.
* **Battery Optimization Bypass**: Whitelists Termux (`com.termux`) and Shizuku (`moe.shizuku.privileged.api`) in `deviceidle` and grants background execution AppOps.
* **Phantom Process Killer**: Disables Android 12+ phantom killer (`max_phantom_processes=2147483647`).
* **TTY Session Detachment & SIGHUP Immunity**: Daemon detaches from terminal sessions via `setsid()` and ignores `SIGHUP`, surviving terminal window closure.
* **Resilient Event Stream Recovery**: Logcat event monitors automatically reconnect upon buffer rotations or Shizuku service restarts.
* **Auto-Start on Boot**: Supports `Termux:Boot` via `~/.termux/boot/start-rust-optimizer.sh`.
* **Automatic Log Rotation**: Caps log file size at 2MB with automatic rotation to `.log.old`.

---

## 3. Prerequisites

* **Architecture**: Android `aarch64` / `arm64`.
* **Android OS**: Android 10 (API 29) up to Android 16+ (API 36+).
* **Shizuku**: Active Shizuku service with `rish` binary installed in Termux (`/data/data/com.termux/files/usr/bin/rish`).
* **Termux Environment**: Standard Termux installation with `rust` / `cargo` (automatically handled by `install.sh`).
* **Root Required?**: **NO**. 100% non-root via Shizuku ADB privileged API.

---

## 4. Installation

Run the automated native installer inside the repository directory:

```bash
chmod +x install.sh
./install.sh
```

The installer will:
1. Validate architecture (`aarch64`) and Shizuku `rish` authorization.
2. Disable Phantom Process Killer and whitelist Termux & Shizuku in battery settings.
3. Compile the binary with host-native vectorization (`-C target-cpu=native -C opt-level=3`).
4. Install the executable to `$PREFIX/bin/rust-android-optimizer`.
5. Configure shell aliases in `~/.bashrc` and `~/.zshrc`.
6. Prompt for automatic start on device boot (`Termux:Boot`).
7. Prompt for operational mode selection (`Adaptive` or `Performance`).
8. Optionally execute the hardware capability benchmark.

---

## 5. Usage & Commands

After installation, the following shell commands are available:

### Start Daemon (Background)
```bash
rust-optimizer-start
```
*Starts the daemon detached in background, acquires CPU wake lock, enforces anti-OOM priority (-900), and redirects output to `~/.rust-android-optimizer.log`.*

### Check Daemon Status
```bash
rust-optimizer-status
```
*Displays current operational state (RUNNING/STOPPED), active mode, process PID, binary version, and log path.*

### Stop Daemon & Restore System
```bash
rust-optimizer-stop
```
*Sends termination signal to the daemon, triggers full snapshot restoration of system settings, releases wake lock, and cleans up PID tracking.*

### Run Hardware Benchmark & Feature Probe
```bash
rust-android-optimizer bench
```
*Performs an in-depth probe of SoC vendor, OEM ROM, Android API level, supported/max screen refresh rates, Shizuku IPC round-trip latency, and feature compatibility matrix.*

---

## 6. License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

Copyright (c) 2026 Mochilamv
