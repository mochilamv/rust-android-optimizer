use std::fmt;
use std::path::Path;
use std::process::{Command as StdCommand, Stdio};
use tokio::process::Command as TokioCommand;

pub const RISH_PATH: &str = "/data/data/com.termux/files/usr/bin/rish";

#[derive(Debug)]
pub enum ShizukuError {
    IoError(std::io::Error),
    NonZeroExit { code: i32, stderr: String },
    RishNotFound,
}

impl fmt::Display for ShizukuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShizukuError::IoError(e) => write!(f, "IO Error: {}", e),
            ShizukuError::NonZeroExit { code, stderr } => {
                write!(f, "Command failed with exit code {}: {}", code, stderr)
            }
            ShizukuError::RishNotFound => write!(f, "Shizuku rish binary not found at {}", RISH_PATH),
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

#[inline(always)]
pub fn is_available() -> bool {
    Path::new(RISH_PATH).exists()
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
        Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_string())
    } else {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
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
        Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_string())
    } else {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Err(ShizukuError::NonZeroExit { code, stderr })
    }
}
