use crate::shizuku;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocVendor {
    QualcommSnapdragon,
    MediaTekDimensity,
    SamsungExynos,
    GoogleTensor,
    Unisoc,
    GenericArm,
}

impl fmt::Display for SocVendor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SocVendor::QualcommSnapdragon => write!(f, "Qualcomm Snapdragon"),
            SocVendor::MediaTekDimensity => write!(f, "MediaTek Dimensity/Helio"),
            SocVendor::SamsungExynos => write!(f, "Samsung Exynos"),
            SocVendor::GoogleTensor => write!(f, "Google Tensor"),
            SocVendor::Unisoc => write!(f, "Unisoc"),
            SocVendor::GenericArm => write!(f, "Generic ARM SoC"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OemFlavor {
    XiaomiMiuiHyperOs,
    SamsungOneUi,
    OplusColorRealmeOxygen,
    MotorolaMyUxHelloUi,
    GooglePixelStock,
    GenericAosp,
}

impl fmt::Display for OemFlavor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OemFlavor::XiaomiMiuiHyperOs => write!(f, "Xiaomi (MIUI / HyperOS)"),
            OemFlavor::SamsungOneUi => write!(f, "Samsung (OneUI)"),
            OemFlavor::OplusColorRealmeOxygen => write!(f, "OPPO / Realme / OnePlus (ColorOS / OxygenOS)"),
            OemFlavor::MotorolaMyUxHelloUi => write!(f, "Motorola (MyUX / Hello UI)"),
            OemFlavor::GooglePixelStock => write!(f, "Google Pixel (Stock AOSP)"),
            OemFlavor::GenericAosp => write!(f, "Generic AOSP / Custom ROM"),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DisplayInfo {
    pub max_refresh_rate: f32,
    pub current_refresh_rate: f32,
    pub supported_rates: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct FeatureSupport {
    pub shizuku_active: bool,
    pub fixed_performance_mode: bool,
    pub thermal_override: bool,
    pub game_mode_api: bool,
    pub doze_force_idle: bool,
    pub display_rate_lock: bool,
    pub touch_latency_flags: bool,
    pub wifi_power_save_flag: bool,
    pub ram_expansion_control: bool,
}

#[derive(Debug, Clone)]
pub struct HardwareProfile {
    pub android_api: u32,
    pub android_release: String,
    pub manufacturer: String,
    pub model: String,
    pub platform: String,
    pub soc_vendor: SocVendor,
    pub oem_flavor: OemFlavor,
    pub display: DisplayInfo,
    pub features: FeatureSupport,
}

impl HardwareProfile {
    pub async fn probe() -> Self {
        let api_str = get_prop("ro.build.version.sdk").await;
        let android_api = api_str.trim().parse::<u32>().unwrap_or(30);
        let android_release = get_prop("ro.build.version.release").await;
        let manufacturer = get_prop("ro.product.manufacturer").await;
        let model = get_prop("ro.product.model").await;
        let platform = get_prop("ro.board.platform").await;

        let soc_vendor = detect_soc(&platform, &manufacturer).await;
        let oem_flavor = detect_oem(&manufacturer).await;
        let display = detect_display_info().await;
        let features = probe_features(android_api).await;

        Self {
            android_api,
            android_release,
            manufacturer,
            model,
            platform,
            soc_vendor,
            oem_flavor,
            display,
            features,
        }
    }
}

pub async fn get_prop(prop_name: &str) -> String {
    // Fast local query via Termux /system/bin/getprop (0.2 ms)
    if let Ok(output) = tokio::process::Command::new("getprop")
        .arg(prop_name)
        .stdin(std::process::Stdio::null())
        .output()
        .await
    {
        let val = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !val.is_empty() {
            return val;
        }
    }

    // Shizuku fallback if local getprop returned empty
    let cmd = format!("getprop {}", prop_name);
    if let Ok(val) = shizuku::exec(&cmd).await {
        let trimmed = val.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }

    String::new()
}

async fn detect_soc(platform: &str, manufacturer: &str) -> SocVendor {
    let plat_lower = platform.to_lowercase();
    let soc_mfg = get_prop("ro.soc.manufacturer").await.to_lowercase();
    let soc_model = get_prop("ro.soc.model").await.to_lowercase();
    let hardware = get_prop("ro.hardware").await.to_lowercase();

    let combined = format!("{} {} {} {} {}", plat_lower, soc_mfg, soc_model, hardware, manufacturer.to_lowercase());

    if combined.contains("qcom")
        || combined.contains("qualcomm")
        || combined.contains("snapdragon")
        || combined.starts_with("sm")
        || combined.starts_with("sdm")
        || combined.starts_with("msm")
        || combined.contains("taro")
        || combined.contains("lahaina")
        || combined.contains("kona")
    {
        SocVendor::QualcommSnapdragon
    } else if combined.contains("mtk")
        || combined.contains("mediatek")
        || combined.contains("dimensity")
        || combined.starts_with("mt6")
        || combined.starts_with("mt8")
    {
        SocVendor::MediaTekDimensity
    } else if combined.contains("exynos") || combined.contains("s5e") || combined.contains("samsungexynos") {
        SocVendor::SamsungExynos
    } else if combined.contains("tensor") || combined.contains("zuma") || combined.contains("gs101") || combined.contains("gs201") {
        SocVendor::GoogleTensor
    } else if combined.contains("unisoc") || combined.contains("sprd") || combined.contains("ums") {
        SocVendor::Unisoc
    } else {
        SocVendor::GenericArm
    }
}

async fn detect_oem(manufacturer: &str) -> OemFlavor {
    let mfg_lower = manufacturer.to_lowercase();

    let miui_ver = get_prop("ro.miui.ui.version.name").await;
    let hyperos_ver = get_prop("ro.hyperos.version").await;
    if !miui_ver.is_empty() || !hyperos_ver.is_empty() || mfg_lower.contains("xiaomi") || mfg_lower.contains("redmi") || mfg_lower.contains("poco") {
        return OemFlavor::XiaomiMiuiHyperOs;
    }

    let oneui_ver = get_prop("ro.build.version.oneui").await;
    let sep_ver = get_prop("ro.build.version.sep").await;
    if !oneui_ver.is_empty() || !sep_ver.is_empty() || mfg_lower.contains("samsung") {
        return OemFlavor::SamsungOneUi;
    }

    let oplus_rom = get_prop("ro.build.version.oplusrom").await;
    let color_rom = get_prop("ro.coloros.version").await;
    if !oplus_rom.is_empty() || !color_rom.is_empty() || mfg_lower.contains("oppo") || mfg_lower.contains("realme") || mfg_lower.contains("oneplus") {
        return OemFlavor::OplusColorRealmeOxygen;
    }

    let mot_sdk = get_prop("ro.mot.build.version.sdk_int").await;
    if !mot_sdk.is_empty() || mfg_lower.contains("motorola") {
        return OemFlavor::MotorolaMyUxHelloUi;
    }

    if mfg_lower.contains("google") {
        return OemFlavor::GooglePixelStock;
    }

    OemFlavor::GenericAosp
}

async fn detect_display_info() -> DisplayInfo {
    let mut rates = Vec::new();

    // Query dumpsys display with server-side grep filter
    if let Ok(dumpsys) = shizuku::exec("dumpsys display | grep -E 'mSupportedModes|supportedModes|fps|renderFrameRate'").await {
        rates = parse_refresh_rates_from_dumpsys(&dumpsys);
    }

    // Fallback: check dumpsys SurfaceFlinger
    if rates.is_empty() {
        if let Ok(sf_out) = shizuku::exec("dumpsys SurfaceFlinger --display-modes").await {
            rates = parse_refresh_rates_from_dumpsys(&sf_out);
        }
    }

    // Fallback: check system settings
    if rates.is_empty() {
        if let Ok(peak) = shizuku::exec("settings get system peak_refresh_rate").await {
            if let Ok(hz) = peak.trim().parse::<f32>() {
                if hz >= 30.0 && hz <= 360.0 {
                    rates.push(hz);
                }
            }
        }
    }

    // Sort ascending
    rates.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    rates.dedup();

    let max_refresh_rate = rates.last().copied().unwrap_or(60.0);
    let current_refresh_rate = max_refresh_rate;

    DisplayInfo {
        max_refresh_rate,
        current_refresh_rate,
        supported_rates: rates,
    }
}

pub fn parse_refresh_rates_from_dumpsys(dumpsys: &str) -> Vec<f32> {
    let mut rates = Vec::new();

    for line in dumpsys.lines() {
        if line.contains("fps=") || line.contains("fps:") || line.contains("supportedRefreshRates") || line.contains("renderFrameRate") {
            let bytes = line.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                // Find "fps="
                if i + 4 <= bytes.len() && &bytes[i..i + 4] == b"fps=" {
                    let start = i + 4;
                    let mut end = start;
                    while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
                        end += 1;
                    }
                    if let Ok(val) = line[start..end].parse::<f32>() {
                        if val >= 30.0 && val <= 360.0 {
                            rates.push(val.round());
                        }
                    }
                    i = end;
                } else {
                    i += 1;
                }
            }

            // Also check for array patterns like [120.0, 90.0, 60.0]
            if line.contains("supportedRefreshRates") {
                if let Some(start) = line.find('[') {
                    if let Some(end) = line[start..].find(']') {
                        let inner = &line[start + 1..start + end];
                        for part in inner.split(',') {
                            if let Ok(val) = part.trim().parse::<f32>() {
                                if val >= 30.0 && val <= 360.0 {
                                    rates.push(val.round());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    rates.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    rates.dedup();
    rates
}

async fn probe_features(android_api: u32) -> FeatureSupport {
    let shizuku_active = shizuku::is_available();

    let fixed_performance_mode = if shizuku_active && android_api >= 30 {
        let res = shizuku::exec("cmd power set-fixed-performance-mode-enabled true").await.is_ok();
        let _ = shizuku::exec("cmd power set-fixed-performance-mode-enabled false").await;
        res
    } else {
        false
    };

    let thermal_override = if shizuku_active {
        let res = shizuku::exec("cmd thermalservice override-status 0").await.is_ok();
        let _ = shizuku::exec("cmd thermalservice reset").await;
        res
    } else {
        false
    };

    let game_mode_api = if shizuku_active && android_api >= 31 {
        shizuku::exec("cmd game").await.is_ok()
    } else {
        false
    };

    let doze_force_idle = if shizuku_active {
        shizuku::exec("dumpsys deviceidle whitelist +com.termux").await.is_ok()
    } else {
        false
    };

    let display_rate_lock = if shizuku_active {
        shizuku::exec("settings get system peak_refresh_rate").await.is_ok()
    } else {
        false
    };

    FeatureSupport {
        shizuku_active,
        fixed_performance_mode,
        thermal_override,
        game_mode_api,
        doze_force_idle,
        display_rate_lock,
        touch_latency_flags: shizuku_active,
        wifi_power_save_flag: shizuku_active,
        ram_expansion_control: shizuku_active,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_refresh_rates_from_dumpsys() {
        let sample = r#"
            supportedModes [{id=1, width=1080, height=2400, fps=120.00001, vsync=120.00001}, {id=2, width=1080, height=2400, fps=90.0}, {id=3, width=1080, height=2400, fps=60.0}]
            supportedRefreshRates [120.00001, 90.0, 60.0, 45.0, 30.0]
        "#;
        let rates = parse_refresh_rates_from_dumpsys(sample);
        assert!(rates.contains(&120.0));
        assert!(rates.contains(&90.0));
        assert!(rates.contains(&60.0));
    }
}
