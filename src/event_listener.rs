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
/// Cortex-A78 L1 cache line = 64 bytes.
#[repr(C, align(64))]
pub(crate) struct CacheAligned<T>(T);

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

/// NEON-vectorized package name extraction from logcat lines.
/// Returns a zero-copy &str slice (no heap allocation).
#[inline(always)]
fn extract_package(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let start = memchr::memmem::find(bytes, b"com.")?;
    let rest = &bytes[start..];
    let end = rest
        .iter()
        .position(|&b| matches!(b, b'/' | b',' | b']' | b'}' | b':' | b';' | b' ' | b'\n' | b'\r'))
        .unwrap_or(rest.len());
    Some(&line[start..start + end])
}

async fn check_if_game(pkg: &str, cache: &Arc<Mutex<HashMap<String, bool>>>) -> bool {
    {
        let cache_lock = cache.lock().unwrap();
        if let Some(&is_game) = cache_lock.get(pkg) {
            return is_game;
        }
    }

    let cmd = format!("dumpsys package {} | grep -i category", pkg);
    let output = shizuku::exec(&cmd).await.unwrap_or_default();

    let is_game = output.contains("category=1") || output.contains("category=GAME");

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
    fn test_extract_package_valid() {
        let line = "[0,1234,com.activision.callofduty.shooter/com.epicgames.ue4.SplashActivity]";
        assert_eq!(
            extract_package(line),
            Some("com.activision.callofduty.shooter")
        );

        let line2 =
            "am_resume_activity: [0,9876,com.tencent.ig/com.tencent.ig.MainActivity,flg=0x10000000]";
        assert_eq!(extract_package(line2), Some("com.tencent.ig"));

        let line3 = "ActivityRecord{com.miHoYo.GenshinImpact} resumed";
        assert_eq!(extract_package(line3), Some("com.miHoYo.GenshinImpact"));
    }

    #[test]
    fn test_extract_package_invalid() {
        let line = "am_resume_activity: invalid line with no package";
        assert_eq!(extract_package(line), None);
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
