use std::fmt;
use std::path::Path;
use std::process::{Command as StdCommand, Stdio};
use std::sync::LazyLock;
use tokio::process::Command as TokioCommand;

pub const RISH_PATH: &str = "/data/data/com.termux/files/usr/bin/rish";

/// Cached at daemon startup: eliminates stat64 syscall (~1-3µs each) on every exec.
/// Safe invariant: rish is not removed while the daemon runs.
static RISH_AVAILABLE: LazyLock<bool> = LazyLock::new(|| Path::new(RISH_PATH).exists());

#[derive(Debug)]
pub enum ShizukuError {
    IoError(std::io::Error),
    NonZeroExit { code: i32, stderr: String },
    RishNotFound,
    Utf8Error,
}

impl fmt::Display for ShizukuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShizukuError::IoError(e) => write!(f, "IO Error: {}", e),
            ShizukuError::NonZeroExit { code, stderr } => {
                write!(f, "Command failed with exit code {}: {}", code, stderr)
            }
            ShizukuError::RishNotFound => write!(f, "Shizuku rish binary not found at {}", RISH_PATH),
            ShizukuError::Utf8Error => write!(f, "rish output contained invalid UTF-8"),
        }
    }
}

impl std::error::Error for ShizukuError {}

impl From<std::io::Error> for ShizukuError {
    #[inline(always)]
    fn from(error: std::io::Error) -> Self {
        ShizukuError::IoError(error)
    }
}

/// Cached via LazyLock: zero-cost after first call (L1 cache read, ~3 cycles vs stat64 ~1-3µs).
#[inline(always)]
pub fn is_available() -> bool {
    *RISH_AVAILABLE
}

pub async fn exec(cmd: &str) -> Result<String, ShizukuError> {
    if !is_available() {
        return Err(ShizukuError::RishNotFound);
    }

    let output = TokioCommand::new(RISH_PATH)
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::null())
        .output()
        .await?;

    if output.status.success() {
        // Single allocation path: from_utf8 avoids Cow overhead and the double-alloc
        // of from_utf8_lossy(...).trim_end().to_string(). rish always emits valid UTF-8.
        let mut s = String::from_utf8(output.stdout).map_err(|_| ShizukuError::Utf8Error)?;
        // trim_end in-place: no new allocation, adjusts len only
        let trimmed_len = s.trim_end().len();
        s.truncate(trimmed_len);
        Ok(s)
    } else {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        Err(ShizukuError::NonZeroExit { code, stderr })
    }
}

#[allow(dead_code)]
pub async fn exec_detached(cmd: &str) -> Result<(), ShizukuError> {
    if !is_available() {
        return Err(ShizukuError::RishNotFound);
    }

    TokioCommand::new(RISH_PATH)
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    Ok(())
}

#[allow(dead_code)]
pub fn exec_blocking(cmd: &str) -> Result<String, ShizukuError> {
    if !is_available() {
        return Err(ShizukuError::RishNotFound);
    }

    let output = StdCommand::new(RISH_PATH)
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::null())
        .output()?;

    if output.status.success() {
        let mut s = String::from_utf8(output.stdout).map_err(|_| ShizukuError::Utf8Error)?;
        let trimmed_len = s.trim_end().len();
        s.truncate(trimmed_len);
        Ok(s)
    } else {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        Err(ShizukuError::NonZeroExit { code, stderr })
    }
}
