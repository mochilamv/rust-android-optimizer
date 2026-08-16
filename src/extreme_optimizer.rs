use std::fmt::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;
use crate::env_probe::{HardwareProfile, OemFlavor};
use crate::shizuku;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OperationalMode {
    #[default]
    Adaptive,
    Performance,
}

impl std::fmt::Display for OperationalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperationalMode::Adaptive => write!(f, "Adaptive (Game-focused, full restore on exit)"),
            OperationalMode::Performance => write!(f, "Performance (Persistent high refresh rate & low latency)"),
        }
    }
}

pub const OEM_THROTTLE_PACKAGES: &[&str] = &[
    "com.samsung.android.game.gos",
    "com.xiaomi.joyose",
    "com.miui.powerkeeper",
    "com.oplus.cosa",
    "com.oplus.games",
    "com.motorola.gamemode.service",
    "com.transsion.gamezone",
];

pub const SYSTEM_EXCLUDED_PACKAGES: &[&str] = &[
    "com.android.systemui",
    "com.android.launcher3",
    "com.google.android.apps.nexuslauncher",
    "com.sec.android.app.launcher",
    "com.miui.home",
    "com.mi.android.globallauncher",
    "com.oppo.launcher",
    "com.oneplus.launcher",
    "com.motorola.launcher3",
    "com.huawei.android.launcher",
    "com.transsion.hilauncher",
    "com.transsion.XOSLauncher",
    "com.termux",
    "android",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingSnapshotEntry {
    pub namespace: Box<str>,
    pub key: Box<str>,
    pub value: Option<Box<str>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemSnapshot {
    pub entries: Vec<SettingSnapshotEntry>,
}

pub struct ExtremeOptimizer {
    active: AtomicBool,
    profile: HardwareProfile,
    mode: OperationalMode,
    snapshot: Mutex<Option<SystemSnapshot>>,
    secondary_whitelisted_apps: Mutex<Vec<Box<str>>>,
}

impl ExtremeOptimizer {
    pub fn new(profile: HardwareProfile, mode: OperationalMode) -> Self {
        Self {
            active: AtomicBool::new(false),
            profile,
            mode,
            snapshot: Mutex::new(None),
            secondary_whitelisted_apps: Mutex::new(Vec::new()),
        }
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub fn get_profile(&self) -> &HardwareProfile {
        &self.profile
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub fn get_mode(&self) -> OperationalMode {
        self.mode
    }

    pub fn build_snapshot_capture_script() -> &'static str {
        concat!(
            "echo \"system min_refresh_rate=$(settings get system min_refresh_rate 2>/dev/null)\"; ",
            "echo \"system peak_refresh_rate=$(settings get system peak_refresh_rate 2>/dev/null)\"; ",
            "echo \"system user_refresh_rate=$(settings get system user_refresh_rate 2>/dev/null)\"; ",
            "echo \"system force_peak_refresh_rate=$(settings get system force_peak_refresh_rate 2>/dev/null)\"; ",
            "echo \"system miui_refresh_rate=$(settings get system miui_refresh_rate 2>/dev/null)\"; ",
            "echo \"global oneplus_screen_refresh_rate=$(settings get global oneplus_screen_refresh_rate 2>/dev/null)\"; ",
            "echo \"global window_animation_scale=$(settings get global window_animation_scale 2>/dev/null)\"; ",
            "echo \"global transition_animation_scale=$(settings get global transition_animation_scale 2>/dev/null)\"; ",
            "echo \"global animator_duration_scale=$(settings get global animator_duration_scale 2>/dev/null)\"; ",
            "echo \"global wifi_scan_throttle_enabled=$(settings get global wifi_scan_throttle_enabled 2>/dev/null)\"; ",
            "echo \"global wifi_power_save=$(settings get global wifi_power_save 2>/dev/null)\"; ",
            "echo \"global network_boost=$(settings get global network_boost 2>/dev/null)\"; ",
            "echo \"system pointer_speed=$(settings get system pointer_speed 2>/dev/null)\"; ",
            "echo \"secure long_press_timeout=$(settings get secure long_press_timeout 2>/dev/null)\"; ",
            "echo \"secure multi_press_timeout=$(settings get secure multi_press_timeout 2>/dev/null)\"; ",
            "echo \"secure tap_duration_threshold=$(settings get secure tap_duration_threshold 2>/dev/null)\"; ",
            "echo \"secure touch_blocking_period=$(settings get secure touch_blocking_period 2>/dev/null)\"; ",
            "echo \"global touch_performance_mode=$(settings get global touch_performance_mode 2>/dev/null)\"; ",
            "echo \"global touch_response_high=$(settings get global touch_response_high 2>/dev/null)\"; ",
            "echo \"global input_latency_reduction=$(settings get global input_latency_reduction 2>/dev/null)\"; ",
            "echo \"global gpu_latency_mode=$(settings get global gpu_latency_mode 2>/dev/null)\"; ",
            "echo \"global disable_window_blurs=$(settings get global disable_window_blurs 2>/dev/null)\"; ",
            "echo \"secure accessibility_reduce_transparency=$(settings get secure accessibility_reduce_transparency 2>/dev/null)\"; ",
            "echo \"global heads_up_notifications_enabled=$(settings get global heads_up_notifications_enabled 2>/dev/null)\"; ",
            "echo \"global ram_expand_size=$(settings get global ram_expand_size 2>/dev/null)\""
        )
    }

    pub async fn ensure_snapshot_captured(&self) {
        let mut lock = self.snapshot.lock().await;
        if lock.is_none() {
            let script = Self::build_snapshot_capture_script();
            if let Ok(output) = shizuku::exec(script).await {
                let snap = parse_snapshot_output(&output);
                println!("[SNAPSHOT] Captured {} system settings before optimization.", snap.entries.len());
                *lock = Some(snap);
            }
        }
    }

    pub async fn detect_secondary_visible_apps(&self, game_pkg: Option<&str>) -> Vec<Box<str>> {
        let game = game_pkg.unwrap_or("");
        let cmd = "dumpsys window visible-apps 2>/dev/null; dumpsys activity activities | grep -E 'mResumedActivity|topResumedActivity|visible=true' 2>/dev/null";
        if let Ok(output) = shizuku::exec(cmd).await {
            parse_visible_packages_from_dumpsys(&output, game)
        } else {
            Vec::new()
        }
    }

    fn build_optimize_script(&self, target_pkg: Option<&str>, secondary_apps: &[Box<str>]) -> String {
        let max_hz = self.profile.display.max_refresh_rate;
        let mut script = String::with_capacity(2048);

        // 1. Whitelist Termux and secondary visible user apps (Split-Screen / Floating Windows / PiP)
        script.push_str("dumpsys deviceidle whitelist +com.termux");
        for secondary in secondary_apps {
            let _ = write!(script, "; dumpsys deviceidle whitelist +{secondary}");
        }
        script.push_str("; dumpsys deviceidle force-idle");

        // 2. Power and Performance modes
        if self.profile.android_api >= 30 {
            script.push_str("; cmd power set-fixed-performance-mode-enabled true");
        }
        script.push_str("; cmd power set-adaptive-power-saver-enabled false");

        // 3. Thermal Throttling override
        script.push_str("; cmd thermalservice override-status 0");

        // 4. Android Game Mode API & Device Config Overlay
        if let Some(pkg) = target_pkg {
            if self.profile.android_api >= 31 {
                let _ = write!(script, "; cmd game mode performance {pkg}; cmd game set --fps {} {pkg}", max_hz as u32);
            }
            if self.profile.android_api >= 33 {
                let _ = write!(script, "; device_config put game_overlay {pkg} mode=2,fps={}", max_hz as u32);
            }
        }

        // 5. Compositor, Graphics & FPS Unlock properties
        script.push_str("; setprop debug.hwui.render_ahead 0; setprop debug.sf.boost_sf_on_touch true; setprop debug.sf.latch_unsignaled 1; setprop debug.sf.high_fps_early_phase_offset_ns 0; setprop debug.sf.high_fps_early_gl_phase_offset_ns 0; setprop debug.egl.hw 1; setprop debug.egl.profiler 0; setprop debug.graphics.game_default_frame_rate.disabled 1; setprop debug.sf.disable_backpressure 1");

        // 6. Dynamic Display Refresh Rate lock
        let _ = write!(
            script,
            "; settings put system min_refresh_rate {max_hz:.1}; settings put system peak_refresh_rate {max_hz:.1}; settings put system user_refresh_rate {max_hz:.1}; settings put system force_peak_refresh_rate 1"
        );

        match self.profile.oem_flavor {
            OemFlavor::XiaomiMiuiHyperOs => {
                let _ = write!(script, "; settings put system miui_refresh_rate {max_hz:.1}");
            }
            OemFlavor::OplusColorRealmeOxygen => {
                script.push_str("; settings put global oneplus_screen_refresh_rate 2");
            }
            _ => {}
        }

        // 7. Touch & Input latency reduction + zero debounce
        script.push_str("; settings put global gpu_latency_mode 1; settings put secure long_press_timeout 200; settings put secure multi_press_timeout 200; settings put secure tap_duration_threshold 0.0; settings put secure touch_blocking_period 0.0; settings put system pointer_speed 7; settings put global touch_performance_mode 1; settings put global touch_response_high 1; settings put global input_latency_reduction 1");

        // 8. GPU Blurs, Transparency Overhead & Instant Animations
        script.push_str("; settings put global disable_window_blurs 1; settings put secure accessibility_reduce_transparency 1; settings put global window_animation_scale 0; settings put global transition_animation_scale 0; settings put global animator_duration_scale 0");

        // 9. Heads-up Anti-Stutter & Network Tweaks
        script.push_str("; settings put global heads_up_notifications_enabled 0; settings put global wifi_scan_throttle_enabled 1; settings put global wifi_power_save 0; settings put global network_boost 1");

        // 10. Virtual RAM / Swap disk thrashing mitigation
        script.push_str("; settings put global ram_expand_size 0");

        // 11. OEM Throttling Services Bypass (AppOps ignore)
        for pkg in OEM_THROTTLE_PACKAGES {
            let _ = write!(script, "; cmd appops set {pkg} RUN_IN_BACKGROUND ignore; cmd appops set {pkg} RUN_ANY_IN_BACKGROUND ignore");
        }

        script
    }

    fn build_restore_script(&self, target_pkg: Option<&str>, snapshot: Option<&SystemSnapshot>, secondary_apps: &[Box<str>], full_restore: bool) -> String {
        let mut script = String::with_capacity(2048);

        // Always unforce doze on game exit or shutdown
        script.push_str("dumpsys deviceidle unforce");

        // Remove secondary visible apps from whitelist
        for secondary in secondary_apps {
            let _ = write!(script, "; dumpsys deviceidle whitelist -{secondary}");
        }

        // Target package game mode reset
        if let Some(pkg) = target_pkg {
            if self.profile.android_api >= 31 {
                let _ = write!(script, "; cmd game reset {pkg}");
            }
            if self.profile.android_api >= 33 {
                let _ = write!(script, "; device_config delete game_overlay {pkg}");
            }
        }

        // If in Performance mode and NOT a full daemon shutdown, keep global performance tweaks
        if self.mode == OperationalMode::Performance && !full_restore {
            return script;
        }

        // --- FULL RESTORE (Adaptive mode or daemon shutdown) ---
        script.push_str("; cmd thermalservice reset");
        if self.profile.android_api >= 30 {
            script.push_str("; cmd power set-fixed-performance-mode-enabled false");
        }
        script.push_str("; cmd power set-adaptive-power-saver-enabled true");

        // Restore OEM throttling services AppOps
        for pkg in OEM_THROTTLE_PACKAGES {
            let _ = write!(script, "; cmd appops set {pkg} RUN_IN_BACKGROUND allow; cmd appops set {pkg} RUN_ANY_IN_BACKGROUND allow");
        }

        // Revert all settings from snapshot
        if let Some(snap) = snapshot {
            for entry in &snap.entries {
                if let Some(val) = &entry.value {
                    let _ = write!(script, "; settings put {} {} {val}", entry.namespace, entry.key);
                } else {
                    let _ = write!(script, "; settings delete {} {}", entry.namespace, entry.key);
                }
            }
        } else {
            script.push_str("; settings put global window_animation_scale 1; settings put global transition_animation_scale 1; settings put global animator_duration_scale 1; settings put global wifi_power_save 1; settings put global heads_up_notifications_enabled 1; settings put global disable_window_blurs 0; settings put secure accessibility_reduce_transparency 0");
        }

        script
    }

    pub async fn apply_optimizations(&self, target_pkg: Option<&str>) {
        self.ensure_snapshot_captured().await;

        if self.active.load(Ordering::Relaxed) {
            return;
        }

        // Dynamically detect any secondary visible apps (Split-Screen, Freeform, PiP)
        let secondary_apps = self.detect_secondary_visible_apps(target_pkg).await;
        if !secondary_apps.is_empty() {
            println!(
                "[OPTIMIZER] Multi-window / Floating app detected: {:?}. Whitelisted from background throttling.",
                secondary_apps
            );
        }

        let script = self.build_optimize_script(target_pkg, &secondary_apps);

        // Save secondary apps for un-whitelisting on exit
        {
            let mut sec_lock = self.secondary_whitelisted_apps.lock().await;
            *sec_lock = secondary_apps;
        }

        match shizuku::exec_detached(&script).await {
            Ok(_) => {
                self.active.store(true, Ordering::Relaxed);
                println!(
                    "[OPTIMIZER] Mode [{:?}]: Tweaks applied (Game: {})",
                    self.mode,
                    target_pkg.unwrap_or("general")
                );
                println!(
                    "[DISPLAY] Refresh rate locked to {:.1} Hz",
                    self.profile.display.max_refresh_rate
                );
            }
            Err(e) => {
                eprintln!("[OPTIMIZER] Error applying optimizations: {}", e);
            }
        }
    }

    pub async fn restore_system(&self, target_pkg: Option<&str>) {
        if !self.active.load(Ordering::Relaxed) {
            return;
        }

        let lock = self.snapshot.lock().await;
        let mut sec_lock = self.secondary_whitelisted_apps.lock().await;
        let secondary_apps = std::mem::take(&mut *sec_lock);

        let script = self.build_restore_script(target_pkg, lock.as_ref(), &secondary_apps, false);
        drop(lock);
        drop(sec_lock);

        match shizuku::exec_detached(&script).await {
            Ok(_) => {
                self.active.store(false, Ordering::Relaxed);
                if self.mode == OperationalMode::Adaptive {
                    println!("[OPTIMIZER] Adaptive mode: full system settings restored to snapshot.");
                } else {
                    println!("[OPTIMIZER] Performance mode: background unforced; persistent high refresh rate retained.");
                }
            }
            Err(e) => {
                eprintln!("[OPTIMIZER] Error during system restoration: {}", e);
            }
        }
    }

    pub async fn full_restore_on_shutdown(&self) {
        let lock = self.snapshot.lock().await;
        let mut sec_lock = self.secondary_whitelisted_apps.lock().await;
        let secondary_apps = std::mem::take(&mut *sec_lock);

        let script = self.build_restore_script(None, lock.as_ref(), &secondary_apps, true);
        drop(lock);
        drop(sec_lock);

        let _ = shizuku::exec_detached(&script).await;
        self.active.store(false, Ordering::Relaxed);
        println!("[OPTIMIZER] Shutdown: All settings reverted strictly to pre-daemon snapshot.");
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }
}

/// Parses batch settings output into a strongly typed SystemSnapshot.
pub fn parse_snapshot_output(raw: &str) -> SystemSnapshot {
    let mut entries = Vec::new();

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some((namespace, rest)) = trimmed.split_once(' ') {
            if let Some((key, raw_val)) = rest.split_once('=') {
                let val_trimmed = raw_val.trim();
                let value = if val_trimmed.is_empty()
                    || val_trimmed == "null"
                    || val_trimmed.contains("Failed to find")
                    || val_trimmed.contains("Invalid")
                {
                    None
                } else {
                    Some(val_trimmed.into())
                };

                entries.push(SettingSnapshotEntry {
                    namespace: namespace.into(),
                    key: key.trim().into(),
                    value,
                });
            }
        }
    }

    SystemSnapshot { entries }
}

/// Pure parser function to dynamically identify secondary visible user packages from dumpsys.
pub fn parse_visible_packages_from_dumpsys(dumpsys: &str, exclude_pkg: &str) -> Vec<Box<str>> {
    let mut packages = Vec::new();

    for line in dumpsys.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let candidate = if let Some(pkg) = trimmed.strip_prefix("Package: ") {
            Some(pkg.trim())
        } else if trimmed.contains("ActivityRecord{")
            || trimmed.contains("Window{")
            || trimmed.contains("mResumedActivity")
            || trimmed.contains("topResumedActivity")
        {
            crate::event_listener::extract_package(trimmed)
        } else {
            None
        };

        if let Some(pkg) = candidate {
            if pkg.is_empty()
                || pkg == exclude_pkg
                || SYSTEM_EXCLUDED_PACKAGES.contains(&pkg)
                || SYSTEM_EXCLUDED_PACKAGES.iter().any(|&sys| pkg.starts_with(sys))
            {
                continue;
            }

            let boxed: Box<str> = pkg.into();
            if !packages.contains(&boxed) {
                packages.push(boxed);
            }
        }
    }

    packages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_snapshot_output_clean() {
        let sample = r#"
            system min_refresh_rate=60.0
            system peak_refresh_rate=120.0
            global window_animation_scale=0.5
            secure tap_duration_threshold=null
            global disable_window_blurs=
            global heads_up_notifications_enabled=1
        "#;

        let snapshot = parse_snapshot_output(sample);
        assert_eq!(snapshot.entries.len(), 6);

        assert_eq!(
            snapshot.entries[0],
            SettingSnapshotEntry {
                namespace: "system".into(),
                key: "min_refresh_rate".into(),
                value: Some("60.0".into()),
            }
        );

        assert_eq!(
            snapshot.entries[1],
            SettingSnapshotEntry {
                namespace: "system".into(),
                key: "peak_refresh_rate".into(),
                value: Some("120.0".into()),
            }
        );

        assert_eq!(
            snapshot.entries[2],
            SettingSnapshotEntry {
                namespace: "global".into(),
                key: "window_animation_scale".into(),
                value: Some("0.5".into()),
            }
        );

        assert_eq!(
            snapshot.entries[3],
            SettingSnapshotEntry {
                namespace: "secure".into(),
                key: "tap_duration_threshold".into(),
                value: None,
            }
        );

        assert_eq!(
            snapshot.entries[4],
            SettingSnapshotEntry {
                namespace: "global".into(),
                key: "disable_window_blurs".into(),
                value: None,
            }
        );
    }

    #[test]
    fn test_parse_visible_packages_single_game() {
        let sample = r#"
            Package: com.dts.freefireth
            Package: com.android.systemui
            mResumedActivity: ActivityRecord{45a8b7c u0 com.dts.freefireth/com.dts.freefireth.FFMainActivity t101}
        "#;

        let visible = parse_visible_packages_from_dumpsys(sample, "com.dts.freefireth");
        assert!(visible.is_empty());
    }

    #[test]
    fn test_parse_visible_packages_multi_window_and_floating() {
        let sample = r#"
            Package: com.dts.freefireth
            Package: com.discord
            Package: com.spotify.music
            Package: com.android.systemui
            Package: com.sec.android.app.launcher
            mResumedActivity: ActivityRecord{45a8b7c u0 com.dts.freefireth/com.dts.freefireth.FFMainActivity t101}
            mResumedActivity: ActivityRecord{89f1e2d u0 com.discord/.MainActivity t102}
        "#;

        let visible = parse_visible_packages_from_dumpsys(sample, "com.dts.freefireth");
        assert_eq!(visible.len(), 2);
        assert!(visible.contains(&Box::<str>::from("com.discord")));
        assert!(visible.contains(&Box::<str>::from("com.spotify.music")));
        assert!(!visible.contains(&Box::<str>::from("com.android.systemui")));
        assert!(!visible.contains(&Box::<str>::from("com.sec.android.app.launcher")));
    }

    #[test]
    fn test_build_optimize_script_with_secondary_whitelist() {
        let profile = HardwareProfile {
            android_api: 34,
            android_release: "14".to_string(),
            manufacturer: "Xiaomi".to_string(),
            model: "POCO F5".to_string(),
            platform: "taro".to_string(),
            soc_vendor: crate::env_probe::SocVendor::QualcommSnapdragon,
            oem_flavor: OemFlavor::XiaomiMiuiHyperOs,
            display: crate::env_probe::DisplayInfo {
                max_refresh_rate: 120.0,
                current_refresh_rate: 120.0,
                supported_rates: vec![60.0, 120.0],
            },
            features: crate::env_probe::FeatureSupport {
                shizuku_active: true,
                fixed_performance_mode: true,
                thermal_override: true,
                game_mode_api: true,
                doze_force_idle: true,
                display_rate_lock: true,
                touch_latency_flags: true,
                wifi_power_save_flag: true,
                ram_expansion_control: true,
            },
        };

        let optimizer = ExtremeOptimizer::new(profile, OperationalMode::Adaptive);
        let secondary = vec![Box::<str>::from("com.discord"), Box::<str>::from("com.spotify.music")];
        let script = optimizer.build_optimize_script(Some("com.dts.freefireth"), &secondary);

        assert!(script.contains("dumpsys deviceidle whitelist +com.termux"));
        assert!(script.contains("dumpsys deviceidle whitelist +com.discord"));
        assert!(script.contains("dumpsys deviceidle whitelist +com.spotify.music"));
        assert!(script.contains("dumpsys deviceidle force-idle"));
    }

    #[test]
    fn test_restore_script_generation_with_snapshot_and_secondary_unwhitelist() {
        let profile = HardwareProfile {
            android_api: 34,
            android_release: "14".to_string(),
            manufacturer: "Xiaomi".to_string(),
            model: "POCO F5".to_string(),
            platform: "taro".to_string(),
            soc_vendor: crate::env_probe::SocVendor::QualcommSnapdragon,
            oem_flavor: OemFlavor::XiaomiMiuiHyperOs,
            display: crate::env_probe::DisplayInfo {
                max_refresh_rate: 120.0,
                current_refresh_rate: 120.0,
                supported_rates: vec![60.0, 120.0],
            },
            features: crate::env_probe::FeatureSupport {
                shizuku_active: true,
                fixed_performance_mode: true,
                thermal_override: true,
                game_mode_api: true,
                doze_force_idle: true,
                display_rate_lock: true,
                touch_latency_flags: true,
                wifi_power_save_flag: true,
                ram_expansion_control: true,
            },
        };

        let optimizer = ExtremeOptimizer::new(profile, OperationalMode::Adaptive);

        let snapshot = SystemSnapshot {
            entries: vec![
                SettingSnapshotEntry {
                    namespace: "system".into(),
                    key: "min_refresh_rate".into(),
                    value: Some("60.0".into()),
                },
                SettingSnapshotEntry {
                    namespace: "global".into(),
                    key: "disable_window_blurs".into(),
                    value: None,
                },
            ],
        };

        let secondary = vec![Box::<str>::from("com.discord")];
        let restore_script = optimizer.build_restore_script(Some("com.dts.freefireth"), Some(&snapshot), &secondary, true);
        assert!(restore_script.contains("dumpsys deviceidle whitelist -com.discord"));
        assert!(restore_script.contains("settings put system min_refresh_rate 60.0"));
        assert!(restore_script.contains("settings delete global disable_window_blurs"));
        assert!(restore_script.contains("cmd appops set com.xiaomi.joyose RUN_IN_BACKGROUND allow"));
        assert!(restore_script.contains("device_config delete game_overlay com.dts.freefireth"));
    }

    #[test]
    fn test_performance_mode_retention_on_game_exit() {
        let profile = HardwareProfile {
            android_api: 34,
            android_release: "14".to_string(),
            manufacturer: "Samsung".to_string(),
            model: "Galaxy S23".to_string(),
            platform: "kalama".to_string(),
            soc_vendor: crate::env_probe::SocVendor::QualcommSnapdragon,
            oem_flavor: OemFlavor::SamsungOneUi,
            display: crate::env_probe::DisplayInfo {
                max_refresh_rate: 120.0,
                current_refresh_rate: 120.0,
                supported_rates: vec![60.0, 120.0],
            },
            features: crate::env_probe::FeatureSupport {
                shizuku_active: true,
                fixed_performance_mode: true,
                thermal_override: true,
                game_mode_api: true,
                doze_force_idle: true,
                display_rate_lock: true,
                touch_latency_flags: true,
                wifi_power_save_flag: true,
                ram_expansion_control: true,
            },
        };

        let optimizer = ExtremeOptimizer::new(profile, OperationalMode::Performance);
        let secondary = vec![Box::<str>::from("com.spotify.music")];
        let restore_script = optimizer.build_restore_script(Some("com.tencent.ig"), None, &secondary, false);

        assert!(restore_script.contains("dumpsys deviceidle unforce"));
        assert!(restore_script.contains("dumpsys deviceidle whitelist -com.spotify.music"));
        assert!(restore_script.contains("cmd game reset com.tencent.ig"));
        assert!(!restore_script.contains("cmd thermalservice reset"));
    }
}
