use crate::event_listener::is_known_emulator_or_game_name;
use crate::shizuku;
use parking_lot::Mutex;
use rustc_hash::FxHashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum PackageCategory {
    Game = 0,        // Highest priority
    UserApp = 1,     // Medium priority
    SystemSafe = 2,  // Lowest priority
}

/// Uses Box<str> instead of String: eliminates unused capacity field (saves 8 bytes/entry),
/// improving L2/L3 cache density for sequential iteration during compilation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileTarget {
    pub package: Box<str>,
    pub category: PackageCategory,
}

/// Lightweight handle for controlling the compiler from the event loop
/// without needing to lock the full AotCompiler.
pub struct CompilerHandle {
    pub suspended: Arc<AtomicBool>,
    pub abort_notify: Arc<Notify>,
    pub notify: Arc<Notify>,
    /// parking_lot::Mutex: spinlock fast-path (~10 cycles) before futex,
    /// vs std::sync::Mutex which goes directly to futex (~200+ cycles on contention).
    pub current_package: Arc<Mutex<Option<Box<str>>>>,
}

impl CompilerHandle {
    #[inline(always)]
    pub fn suspend(&self) {
        self.suspended.store(true, Ordering::Release);
        self.abort_notify.notify_waiters();
        let current = self.current_package.lock();
        if let Some(pkg) = &*current {
            println!("[AOT] Suspended compilation immediately. Interrupted: {}", pkg);
        } else {
            println!("[AOT] Suspended compilation immediately.");
        }

        // Fire detached kill command to immediately free CPU cores from ART compiler processes
        tokio::spawn(async {
            let _ = shizuku::exec_detached("killall -9 dex2oat dex2oat64 dex2oat32 2>/dev/null").await;
        });
    }

    #[inline(always)]
    pub fn resume(&self) {
        self.suspended.store(false, Ordering::Release);
        self.notify.notify_waiters();
        println!("[AOT] Resumed compilation.");
    }
}

pub struct AotCompiler {
    queue: Vec<CompileTarget>,
    suspended: Arc<AtomicBool>,
    abort_notify: Arc<Notify>,
    notify: Arc<Notify>,
    compiled_count: u32,
    total_count: u32,
    current_package: Arc<Mutex<Option<Box<str>>>>,
}

impl AotCompiler {
    pub async fn new() -> Self {
        let mut compiler = Self {
            queue: Vec::new(),
            suspended: Arc::new(AtomicBool::new(false)),
            abort_notify: Arc::new(Notify::new()),
            notify: Arc::new(Notify::new()),
            compiled_count: 0,
            total_count: 0,
            current_package: Arc::new(Mutex::new(None)),
        };
        compiler.discover_packages().await;
        compiler
    }

    #[inline(always)]
    pub fn get_suspended_flag(&self) -> Arc<AtomicBool> {
        self.suspended.clone()
    }

    #[inline(always)]
    pub fn get_abort_notify(&self) -> Arc<Notify> {
        self.abort_notify.clone()
    }

    #[inline(always)]
    pub fn get_notify(&self) -> Arc<Notify> {
        self.notify.clone()
    }

    #[inline(always)]
    pub fn get_current_package_arc(&self) -> Arc<Mutex<Option<Box<str>>>> {
        self.current_package.clone()
    }

    async fn discover_packages(&mut self) {
        const SAFE_SYSTEM_APPS: &[&str] = &[
            "com.android.settings",
            "com.android.systemui",
            "com.android.launcher3",
            "com.android.providers.contacts",
            "com.android.providers.media",
            "com.android.providers.telephony",
            "com.android.phone",
            "com.android.server.telecom",
            "com.android.nfc",
            "com.android.bluetooth",
        ];

        let mut game_packages: FxHashSet<Box<str>> = FxHashSet::default();
        if let Ok(dumpsys) = shizuku::exec("dumpsys package packages | grep -E 'Package \\[|category|appCategory|flags.*GAME'").await {
            game_packages = parse_package_categories_from_dumpsys(&dumpsys);
        }

        let mut queue = Vec::new();

        if let Ok(output) = shizuku::exec("cmd package list packages -3").await {
            for line in output.lines() {
                if let Some(pkg) = line.strip_prefix("package:") {
                    let pkg = pkg.trim();
                    if pkg.is_empty() {
                        continue;
                    }

                    let pkg_box: Box<str> = pkg.into();
                    if game_packages.contains(&*pkg_box) || is_known_emulator_or_game_name(pkg) {
                        queue.push(CompileTarget {
                            package: pkg_box,
                            category: PackageCategory::Game,
                        });
                    } else {
                        queue.push(CompileTarget {
                            package: pkg_box,
                            category: PackageCategory::UserApp,
                        });
                    }
                }
            }
        }

        if let Ok(output) = shizuku::exec("cmd package list packages -s").await {
            for line in output.lines() {
                if let Some(pkg) = line.strip_prefix("package:") {
                    let pkg = pkg.trim();
                    if SAFE_SYSTEM_APPS.contains(&pkg) {
                        queue.push(CompileTarget {
                            package: pkg.into(),
                            category: PackageCategory::SystemSafe,
                        });
                    }
                }
            }
        }

        queue.sort_by(|a, b| a.category.cmp(&b.category));

        self.total_count = queue.len() as u32;
        self.queue = queue;

        println!(
            "[AOT] Discovered {} packages ({} total) in single-pass batch",
            self.queue.len(),
            self.total_count
        );
    }

    pub async fn run(&mut self) {
        let mut idx = 0;

        while idx < self.queue.len() {
            while self.suspended.load(Ordering::Acquire) {
                self.notify.notified().await;
            }

            let target = &self.queue[idx];

            {
                *self.current_package.lock() = Some(target.package.clone());
            }

            let compile_cmd = format!("cmd package compile -m speed -f {}", target.package);
            let abort = self.abort_notify.clone();

            tokio::select! {
                res = shizuku::exec(&compile_cmd) => {
                    {
                        *self.current_package.lock() = None;
                    }
                    match res {
                        Ok(_output) => {
                            self.compiled_count += 1;
                            println!(
                                "[AOT {}/{}] Compiled: {} ({:?})",
                                self.compiled_count, self.total_count, target.package, target.category
                            );
                        }
                        Err(e) => {
                            eprintln!("[AOT Error] Failed to compile {}: {}", target.package, e);
                        }
                    }
                    idx += 1;
                }
                _ = abort.notified() => {
                    {
                        *self.current_package.lock() = None;
                    }
                    println!("[AOT] Interrupted compilation of {}. Kept in queue for resumption.", target.package);
                    // Retain current idx to retry compiling this target upon resumption
                }
            }
        }

        println!(
            "[AOT] Compilation summary: {}/{} packages compiled successfully.",
            self.compiled_count, self.total_count
        );
    }
}

/// Zero-alloc case-insensitive substring search (same as event_listener).
/// Avoids to_lowercase() allocation on every category line in hot parse loop.
#[inline(always)]
fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        return true;
    }
    if h.len() < n.len() {
        return false;
    }
    for i in 0..=(h.len() - n.len()) {
        if h[i..i + n.len()].iter().zip(n.iter()).all(|(a, b)| a.eq_ignore_ascii_case(b)) {
            return true;
        }
    }
    false
}

/// Pure parser function to extract game packages from dumpsys package output.
pub fn parse_package_categories_from_dumpsys(dumpsys: &str) -> FxHashSet<Box<str>> {
    let mut game_packages: FxHashSet<Box<str>> = FxHashSet::default();
    let mut current_pkg: Option<&str> = None;

    for line in dumpsys.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Package [") {
            if let Some(start) = trimmed.find('[') {
                if let Some(end) = trimmed[start + 1..].find(']') {
                    let pkg = &trimmed[start + 1..start + 1 + end];
                    current_pkg = Some(pkg);
                    if is_known_emulator_or_game_name(pkg) {
                        game_packages.insert(pkg.into());
                    }
                }
            }
        } else if trimmed.contains("category=1")
            || trimmed.contains("category=GAME")
            || contains_ignore_ascii_case(trimmed, "category_game")
            || trimmed.contains("appCategory=\"1\"")
            || trimmed.contains("appCategory=\"GAME\"")
            || (trimmed.contains("flags=[") && trimmed.contains("GAME"))
        {
            if let Some(pkg) = current_pkg {
                game_packages.insert(pkg.into());
            }
        }
    }

    game_packages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_package_categories_from_dumpsys() {
        let sample = r#"
        Package [com.activision.callofduty.shooter] (54a8b7c):
            userId=10123
            category=1
            flags=[ HAS_CODE ALLOW_CLEAR_USER_DATA ]
        Package [org.ppsspp.ppsspp] (12a3b4c):
            userId=10124
            flags=[ HAS_CODE ALLOW_CLEAR_USER_DATA ]
        Package [com.google.android.youtube] (98f7e6d):
            userId=10125
            category=2
        Package [com.dts.freefireth] (33b2a1c):
            userId=10126
            appCategory="GAME"
        Package [com.example.customgame] (77a6c5e):
            userId=10127
            flags=[ HAS_CODE GAME ALLOW_CLEAR_USER_DATA ]
        "#;

        let games = parse_package_categories_from_dumpsys(sample);
        assert!(games.contains(&*Box::<str>::from("com.activision.callofduty.shooter")));
        assert!(games.contains(&*Box::<str>::from("org.ppsspp.ppsspp"))); // via known emulator name
        assert!(games.contains(&*Box::<str>::from("com.dts.freefireth"))); // via appCategory="GAME"
        assert!(games.contains(&*Box::<str>::from("com.example.customgame"))); // via flags GAME
        assert!(!games.contains(&*Box::<str>::from("com.google.android.youtube")));
    }

    #[test]
    fn test_queue_sorting_priority() {
        let mut queue = vec![
            CompileTarget {
                package: "com.android.settings".into(),
                category: PackageCategory::SystemSafe,
            },
            CompileTarget {
                package: "com.example.normalapp".into(),
                category: PackageCategory::UserApp,
            },
            CompileTarget {
                package: "com.tencent.ig".into(),
                category: PackageCategory::Game,
            },
        ];

        queue.sort_by(|a, b| a.category.cmp(&b.category));

        assert_eq!(queue[0].category, PackageCategory::Game);
        assert_eq!(queue[1].category, PackageCategory::UserApp);
        assert_eq!(queue[2].category, PackageCategory::SystemSafe);
    }

    #[test]
    fn test_contains_ignore_ascii_case() {
        assert!(contains_ignore_ascii_case("Category_Game", "category_game"));
        assert!(contains_ignore_ascii_case("CATEGORY_GAME", "category_game"));
        assert!(!contains_ignore_ascii_case("category=1", "category_game"));
    }
}
