use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::{sleep, Duration};

use crate::aot_compiler::CompilerHandle;
use crate::extreme_optimizer::ExtremeOptimizer;
use crate::shizuku;

/// Cache-line aligned wrapper to prevent false sharing between cores.
/// Cortex-A78/X1 L1 cache line = 64 bytes.
#[repr(C, align(64))]
pub(crate) struct CacheAligned<T>(pub(crate) T);

pub struct GameState {
    pub active_games: Mutex<HashMap<String, bool>>,
    /// Padded to own cache line: written by resume_task, read by pause_task
    pub any_game_foreground: CacheAligned<AtomicBool>,
    /// Padded to own cache line: written/read concurrently by both tasks
    pub pause_generation: CacheAligned<AtomicU64>,
}

impl GameState {
    pub fn new() -> Self {
        Self {
            active_games: Mutex::new(HashMap::new()),
            any_game_foreground: CacheAligned(AtomicBool::new(false)),
            pause_generation: CacheAligned(AtomicU64::new(0)),
        }
    }
}

/// Known emulator and game namespaces/keywords for rapid heuristic classification.
pub fn is_known_emulator_or_game_name(pkg: &str) -> bool {
    let lower = pkg.to_lowercase();
    const PATTERNS: &[&str] = &[
        // Emulators & Virtualization
        "retroarch",
        "dolphinemu",
        "ppsspp",
        "aethersx2",
        "nethersx2",
        "citra",
        "yuzu",
        "sudachi",
        "skyline",
        "strato",
        "vita3k",
        "winlator",
        "mobox",
        "box64",
        "exagear",
        "mupen64",
        "drastic",
        "snes9x",
        "duckstation",
        "pcsx2",
        "melonds",
        "flycast",
        "redream",
        "scummvm",
        "dosbox",
        "fbalpha",
        "mame4droid",
        "yabasanshiro",
        "epsxe",
        "lemuroid",
        "limbo",
        "bochs",
        "cemu",
        "rpcs3",
        "eden",
        "fpx",
        "net.flyingfatiguedetective",
        // Cloud & Remote Gaming
        "geforcenow",
        "xcloud",
        "moonlight",
        "parsec",
        "steamlink",
        "boosteroid",
        "chiaki",
        "shadow.pc",
        // Known Major Game Studios & Titles
        "mihoyo",
        "hoyoverse",
        "epicgames",
        "riotgames",
        "pubg",
        "freefire",
        "minecraft",
        "roblox",
        "supercell",
        "gameloft",
        "krafton",
        "tencent.ig",
        "activision",
        "rockstargames",
        "square_enix",
        "square.enix",
        "bandainamco",
        "capcom",
        "konami",
        "nintendo",
    ];

    for pattern in PATTERNS {
        if lower.contains(pattern) {
            return true;
        }
    }

    false
}

#[inline(always)]
fn is_valid_package_name(s: &str) -> bool {
    if s.len() < 3 || !s.contains('.') {
        return false;
    }
    let bytes = s.as_bytes();
    if !bytes[0].is_ascii_alphabetic() {
        return false;
    }
    if bytes[bytes.len() - 1] == b'.' {
        return false;
    }
    bytes.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_')
}

/// Generic, zero-allocation package name extraction from logcat events.
/// Correctly parses prefixes (com.*, org.*, net.*, io.*, xyz.*, app.*, etc.)
/// and delimiters ('/', ',', ']', '}', ':', ';', ' ').
pub fn extract_package(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();

    // Fast path: Component separator '/'
    if let Some(slash_idx) = memchr::memchr(b'/', bytes) {
        let mut start_idx = slash_idx;
        while start_idx > 0 {
            let b = bytes[start_idx - 1];
            if b.is_ascii_alphanumeric() || b == b'.' || b == b'_' {
                start_idx -= 1;
            } else {
                break;
            }
        }

        let pkg = &line[start_idx..slash_idx];
        if is_valid_package_name(pkg) {
            return Some(pkg);
        }
    }

    // Secondary path: Bracket/brace enclosed package
    if let Some(start_brace) = memchr::memchr(b'{', bytes) {
        let rest = &line[start_brace + 1..];
        let end_idx = rest.find(|c: char| matches!(c, '}' | ' ' | '/' | ',' | ':')).unwrap_or(rest.len());
        let candidate = &rest[..end_idx];
        if is_valid_package_name(candidate) {
            return Some(candidate);
        }
    }

    // Tertiary path: Scan tokens split by common delimiters
    for token in line.split(|c: char| matches!(c, ' ' | '[' | ']' | '{' | '}' | '(' | ')' | ',' | ':' | ';' | '\n' | '\r')) {
        let trimmed = token.trim();
        let cleaned = trimmed.strip_prefix("package:").unwrap_or(trimmed);
        let pkg = if let Some(slash) = cleaned.find('/') {
            &cleaned[..slash]
        } else {
            cleaned
        };

        if is_valid_package_name(pkg) {
            return Some(pkg);
        }
    }

    None
}

pub async fn check_if_game(pkg: &str, cache: &Arc<Mutex<HashMap<String, bool>>>) -> bool {
    {
        let cache_lock = cache.lock().unwrap();
        if let Some(&is_game) = cache_lock.get(pkg) {
            return is_game;
        }
    }

    // Heuristic 1: Known emulator/game naming pattern
    if is_known_emulator_or_game_name(pkg) {
        let mut cache_lock = cache.lock().unwrap();
        cache_lock.insert(pkg.to_string(), true);
        return true;
    }

    // Heuristic 2: Query Android package manifest category via dumpsys
    let cmd = format!("dumpsys package {} | grep -E -i 'category|appCategory|flags.*GAME'", pkg);
    let output = shizuku::exec(&cmd).await.unwrap_or_default();

    let is_game = output.contains("category=1")
        || output.contains("category=GAME")
        || output.to_lowercase().contains("category_game")
        || output.contains("appCategory=\"1\"")
        || output.contains("appCategory=\"GAME\"")
        || (output.contains("flags=[") && output.contains("GAME"));

    let mut cache_lock = cache.lock().unwrap();
    cache_lock.insert(pkg.to_string(), is_game);
    is_game
}

pub async fn run_event_loop(
    compiler: Arc<CompilerHandle>,
    optimizer: Arc<ExtremeOptimizer>,
    game_cache: Arc<Mutex<HashMap<String, bool>>>,
) {
    let state = Arc::new(GameState::new());

    // Spawn resume monitor
    let state_resume = state.clone();
    let compiler_resume = compiler.clone();
    let optimizer_resume = optimizer.clone();
    let game_cache_resume = game_cache.clone();

    let resume_task = tokio::spawn(async move {
        let mut cmd = Command::new("/data/data/com.termux/files/usr/bin/rish")
            .arg("-c")
            .arg("logcat -b events -v raw -s am_resume_activity")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn logcat resume monitor");

        let stdout = cmd.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout).lines();

        while let Ok(Some(line)) = reader.next_line().await {
            if let Some(pkg) = extract_package(&line) {
                if check_if_game(pkg, &game_cache_resume).await {
                    println!("[EVENT] Game resumed: {}", pkg);

                    // Cancel pending pause debounces immediately
                    state_resume.pause_generation.0.fetch_add(1, Ordering::SeqCst);

                    {
                        let mut active = state_resume.active_games.lock().unwrap();
                        active.insert(pkg.to_string(), true);
                    }
                    state_resume
                        .any_game_foreground
                        .0
                        .store(true, Ordering::Release);

                    compiler_resume.suspend();
                    optimizer_resume.apply_optimizations(Some(pkg)).await;
                }
            }
        }
    });

    // Spawn pause monitor
    let state_pause = state.clone();
    let compiler_pause = compiler.clone();
    let optimizer_pause = optimizer.clone();
    let game_cache_pause = game_cache.clone();

    let pause_task = tokio::spawn(async move {
        let mut cmd = Command::new("/data/data/com.termux/files/usr/bin/rish")
            .arg("-c")
            .arg("logcat -b events -v raw -s am_pause_activity")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn logcat pause monitor");

        let stdout = cmd.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout).lines();

        while let Ok(Some(line)) = reader.next_line().await {
            if let Some(pkg) = extract_package(&line) {
                if check_if_game(pkg, &game_cache_pause).await {
                    println!("[EVENT] Game paused: {}", pkg);

                    {
                        let mut active = state_pause.active_games.lock().unwrap();
                        active.remove(pkg);
                    }

                    // Increment generation and spawn non-blocking debounce task
                    let generation_id =
                        state_pause.pause_generation.0.fetch_add(1, Ordering::SeqCst) + 1;
                    let state_worker = state_pause.clone();
                    let compiler_worker = compiler_pause.clone();
                    let optimizer_worker = optimizer_pause.clone();

                    tokio::spawn(async move {
                        sleep(Duration::from_secs(5)).await;

                        if state_worker.pause_generation.0.load(Ordering::SeqCst) == generation_id {
                            let any_active = {
                                let active = state_worker.active_games.lock().unwrap();
                                !active.is_empty()
                            };

                            if any_active {
                                println!("[EVENT] Another game is active, keeping compilation suspended and doze forced");
                            } else {
                                state_worker
                                    .any_game_foreground
                                    .0
                                    .store(false, Ordering::Release);
                                println!("[EVENT] No game active, resuming compilation and restoring system thermals");
                                compiler_worker.resume();
                                optimizer_worker.restore_system(None).await;
                            }
                        }
                    });
                }
            }
        }
    });

    let _ = tokio::join!(resume_task, pause_task);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_package_slash_format() {
        let line1 = "[0,1234,com.activision.callofduty.shooter/com.epicgames.ue4.SplashActivity]";
        assert_eq!(extract_package(line1), Some("com.activision.callofduty.shooter"));

        let line2 = "am_resume_activity: [0,9876,org.ppsspp.ppsspp/.PpssppActivity,flg=0x10000000]";
        assert_eq!(extract_package(line2), Some("org.ppsspp.ppsspp"));

        let line3 = "ActivityRecord{45a8b7c u0 xyz.aethersx2.android/xyz.aethersx2.android.MainActivity t123}";
        assert_eq!(extract_package(line3), Some("xyz.aethersx2.android"));

        let line4 = "[0,1234567,10123,net.kuribo64.melonDS/net.kuribo64.melonDS.ui.MainActivity,...]";
        assert_eq!(extract_package(line4), Some("net.kuribo64.melonDS"));

        let line5 = "[0,1234567,10123,app.sudachi.sudachi/app.sudachi.sudachi.ui.EmulationActivity]";
        assert_eq!(extract_package(line5), Some("app.sudachi.sudachi"));
    }

    #[test]
    fn test_extract_package_brace_and_token_format() {
        let line1 = "ActivityRecord{com.miHoYo.GenshinImpact} resumed";
        assert_eq!(extract_package(line1), Some("com.miHoYo.GenshinImpact"));

        let line2 = "package:org.dolphinemu.dolphinemu";
        assert_eq!(extract_package(line2), Some("org.dolphinemu.dolphinemu"));

        let line3 = "u0 io.github.vita3k,taskId=123";
        assert_eq!(extract_package(line3), Some("io.github.vita3k"));
    }

    #[test]
    fn test_extract_package_invalid() {
        assert_eq!(extract_package("am_resume_activity: invalid line with no package"), None);
        assert_eq!(extract_package(""), None);
        assert_eq!(extract_package("123/456"), None);
        assert_eq!(extract_package("no_dots_here/something"), None);
    }

    #[test]
    fn test_is_known_emulator_or_game_name() {
        assert!(is_known_emulator_or_game_name("org.ppsspp.ppsspp"));
        assert!(is_known_emulator_or_game_name("xyz.aethersx2.android"));
        assert!(is_known_emulator_or_game_name("org.dolphinemu.dolphinemu"));
        assert!(is_known_emulator_or_game_name("app.sudachi.sudachi"));
        assert!(is_known_emulator_or_game_name("com.retroarch.aarch64"));
        assert!(is_known_emulator_or_game_name("com.miHoYo.GenshinImpact"));
        assert!(is_known_emulator_or_game_name("com.dts.freefireth"));
        assert!(is_known_emulator_or_game_name("com.mojang.minecraftpe"));

        assert!(!is_known_emulator_or_game_name("com.android.settings"));
        assert!(!is_known_emulator_or_game_name("com.google.android.youtube"));
        assert!(!is_known_emulator_or_game_name("com.whatsapp"));
    }

    #[test]
    fn test_game_state_lifecycle() {
        let state = GameState::new();
        assert!(!state.any_game_foreground.0.load(Ordering::Acquire));
        assert_eq!(state.pause_generation.0.load(Ordering::Acquire), 0);

        let test_gen = state.pause_generation.0.fetch_add(1, Ordering::SeqCst) + 1;
        assert_eq!(test_gen, 1);
        assert_eq!(state.pause_generation.0.load(Ordering::Acquire), 1);
    }

    #[test]
    fn test_cache_alignment() {
        assert_eq!(std::mem::align_of::<CacheAligned<AtomicBool>>(), 64);
        assert_eq!(std::mem::align_of::<CacheAligned<AtomicU64>>(), 64);
    }
}
