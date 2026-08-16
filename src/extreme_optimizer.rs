use std::sync::atomic::{AtomicBool, Ordering};
use crate::env_probe::{HardwareProfile, OemFlavor};
use crate::shizuku;

pub struct ExtremeOptimizer {
    active: AtomicBool,
    profile: HardwareProfile,
}

impl ExtremeOptimizer {
    pub fn new(profile: HardwareProfile) -> Self {
        Self {
            active: AtomicBool::new(false),
            profile,
        }
    }

    #[allow(dead_code)]
    pub fn get_profile(&self) -> &HardwareProfile {
        &self.profile
    }

    fn build_optimize_script(&self, target_pkg: Option<&str>) -> String {
        let max_hz = self.profile.display.max_refresh_rate;
        let mut cmds = Vec::new();

        // 1. Whitelist Termux and Force Doze for background tasks
        cmds.push("dumpsys deviceidle whitelist +com.termux");
        cmds.push("dumpsys deviceidle force-idle");

        // 2. Power and Performance modes
        if self.profile.android_api >= 30 {
            cmds.push("cmd power set-fixed-performance-mode-enabled true");
        }
        cmds.push("cmd power set-adaptive-power-saver-enabled false");

        // 3. Thermal Throttling override
        cmds.push("cmd thermalservice override-status 0");

        // 4. Android Game Mode API (Android 12+ / API 31+)
        let game_mode_cmd;
        let game_fps_cmd;
        if let Some(pkg) = target_pkg {
            if self.profile.android_api >= 31 {
                game_mode_cmd = format!("cmd game mode performance {}", pkg);
                cmds.push(&game_mode_cmd);
                game_fps_cmd = format!("cmd game set --fps {} {}", max_hz as u32, pkg);
                cmds.push(&game_fps_cmd);
            }
        }

        // 5. Compositor & Render Latency properties
        cmds.push("setprop debug.hwui.render_ahead 0");
        cmds.push("setprop debug.sf.boost_sf_on_touch true");
        cmds.push("setprop debug.sf.latch_unsignaled 1");
        cmds.push("setprop debug.sf.high_fps_early_phase_offset_ns 0");
        cmds.push("setprop debug.sf.high_fps_early_gl_phase_offset_ns 0");
        cmds.push("setprop debug.egl.hw 1");
        cmds.push("setprop debug.egl.profiler 0");

        // 6. Dynamic Display Refresh Rate lock
        let min_rate_cmd = format!("settings put system min_refresh_rate {:.1}", max_hz);
        let peak_rate_cmd = format!("settings put system peak_refresh_rate {:.1}", max_hz);
        let user_rate_cmd = format!("settings put system user_refresh_rate {:.1}", max_hz);
        cmds.push(&min_rate_cmd);
        cmds.push(&peak_rate_cmd);
        cmds.push(&user_rate_cmd);
        cmds.push("settings put system force_peak_refresh_rate 1");

        // OEM specific display settings
        let miui_rate_cmd;
        match self.profile.oem_flavor {
            OemFlavor::XiaomiMiuiHyperOs => {
                miui_rate_cmd = format!("settings put system miui_refresh_rate {:.1}", max_hz);
                cmds.push(&miui_rate_cmd);
            }
            OemFlavor::OplusColorRealmeOxygen => {
                cmds.push("settings put global oneplus_screen_refresh_rate 2");
            }
            _ => {}
        }

        // 7. Touch & Input latency reduction
        cmds.push("settings put global gpu_latency_mode 1");
        cmds.push("settings put secure long_press_timeout 200");
        cmds.push("settings put secure multi_press_timeout 200");
        cmds.push("settings put system pointer_speed 7");
        cmds.push("settings put global touch_performance_mode 1");
        cmds.push("settings put global touch_response_high 1");
        cmds.push("settings put global input_latency_reduction 1");

        // 8. Animation scales to 0 for instant frame flip
        cmds.push("settings put global window_animation_scale 0");
        cmds.push("settings put global transition_animation_scale 0");
        cmds.push("settings put global animator_duration_scale 0");

        // 9. Network and WiFi latency stabilization
        cmds.push("settings put global wifi_scan_throttle_enabled 1");
        cmds.push("settings put global wifi_power_save 0");
        cmds.push("settings put global network_boost 1");

        // 10. Virtual RAM / Swap disk thrashing mitigation
        cmds.push("settings put global ram_expand_size 0");

        cmds.join("; ")
    }

    fn build_restore_script(&self, target_pkg: Option<&str>) -> String {
        let mut cmds = Vec::new();

        cmds.push("dumpsys deviceidle unforce");
        cmds.push("cmd thermalservice reset");
        if self.profile.android_api >= 30 {
            cmds.push("cmd power set-fixed-performance-mode-enabled false");
        }
        cmds.push("cmd power set-adaptive-power-saver-enabled true");

        let game_reset_cmd;
        if let Some(pkg) = target_pkg {
            if self.profile.android_api >= 31 {
                game_reset_cmd = format!("cmd game reset {}", pkg);
                cmds.push(&game_reset_cmd);
            }
        }

        // Restore animation scale to default
        cmds.push("settings put global window_animation_scale 1");
        cmds.push("settings put global transition_animation_scale 1");
        cmds.push("settings put global animator_duration_scale 1");

        // Restore WiFi power saving
        cmds.push("settings put global wifi_power_save 1");

        cmds.join("; ")
    }

    pub async fn apply_optimizations(&self, target_pkg: Option<&str>) {
        if self.active.load(Ordering::Relaxed) {
            return;
        }

        let script = self.build_optimize_script(target_pkg);
        match shizuku::exec_detached(&script).await {
            Ok(_) => {
                self.active.store(true, Ordering::Relaxed);
                println!(
                    "[OPTIMIZER] EXTREME optimizations applied (Game: {})",
                    target_pkg.unwrap_or("unknown")
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

        let script = self.build_restore_script(target_pkg);
        match shizuku::exec_detached(&script).await {
            Ok(_) => {
                self.active.store(false, Ordering::Relaxed);
                println!("[OPTIMIZER] System thermals, power and refresh rate restored.");
            }
            Err(e) => {
                eprintln!("[OPTIMIZER] Error restoring system: {}", e);
            }
        }
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }
}
