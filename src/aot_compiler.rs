use crate::shizuku;
use rustc_hash::FxHashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
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
pub struct CompileTarget {
    pub package: Box<str>,
    pub category: PackageCategory,
}

/// Lightweight handle for controlling the compiler from the event loop
/// without needing to lock the full AotCompiler.
pub struct CompilerHandle {
    pub suspended: Arc<AtomicBool>,
    pub notify: Arc<Notify>,
    pub current_package: Arc<Mutex<Option<Box<str>>>>,
}

impl CompilerHandle {
    #[inline(always)]
    pub fn suspend(&self) {
        self.suspended.store(true, Ordering::Release);
        let current = self.current_package.lock().unwrap();
        if let Some(pkg) = &*current {
            println!("[AOT] Suspended compilation. Currently compiling/interrupted: {}", pkg);
        } else {
            println!("[AOT] Suspended compilation.");
        }
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

        // FxHashSet: 3-5x faster than SipHash for non-adversarial package name keys
        let mut game_packages: FxHashSet<Box<str>> = FxHashSet::default();
        if let Ok(dumpsys) = shizuku::exec("dumpsys package packages | grep -E 'Package \\[|category'").await {
            let mut current_pkg: Option<&str> = None;
            for line in dumpsys.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("Package [") {
                    if let Some(start) = trimmed.find('[') {
                        if let Some(end) = trimmed[start + 1..].find(']') {
                            current_pkg = Some(&trimmed[start + 1..start + 1 + end]);
                        }
                    }
                } else if trimmed.contains("category=1")
                    || trimmed.contains("category=GAME")
                    || trimmed.to_lowercase().contains("category_game")
                {
                    if let Some(pkg) = current_pkg {
                        game_packages.insert(pkg.into());
                    }
                }
            }
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
                    if game_packages.contains(&*pkg_box) {
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
        let queue_len = self.queue.len();

        for i in 0..queue_len {
            while self.suspended.load(Ordering::Acquire) {
                self.notify.notified().await;
            }

            let target = &self.queue[i];

            {
                let mut current = self.current_package.lock().unwrap();
                *current = Some(target.package.clone());
            }

            match shizuku::exec(&format!("cmd package compile -m speed -f {}", target.package))
                .await
            {
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

            {
                let mut current = self.current_package.lock().unwrap();
                *current = None;
            }
        }

        println!(
            "[AOT] Compilation summary: {}/{} packages compiled successfully.",
            self.compiled_count, self.total_count
        );
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub fn is_suspended(&self) -> bool {
        self.suspended.load(Ordering::Acquire)
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub fn get_current_package(&self) -> Option<Box<str>> {
        let current = self.current_package.lock().unwrap();
        current.clone()
    }
}
