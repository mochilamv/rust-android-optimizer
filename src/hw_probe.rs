use libc::{c_void, close, open, pread, O_RDONLY};
use std::sync::LazyLock;

/// Persistent file descriptor for sysfs nodes.
/// Holds FD open for the daemon lifetime, using pread(offset=0)
/// to re-read on each poll without open/close syscall overhead.
struct SysfsFd(i32);

impl SysfsFd {
    fn open(path: &[u8]) -> Option<Self> {
        let fd = unsafe { open(path.as_ptr() as *const libc::c_char, O_RDONLY) };
        if fd >= 0 {
            Some(Self(fd))
        } else {
            None
        }
    }

    /// Read current value via pread (single syscall, no seek needed).
    #[inline(always)]
    fn read_u32(&self) -> Option<u32> {
        let mut buf = [0u8; 32];
        let bytes_read =
            unsafe { pread(self.0, buf.as_mut_ptr() as *mut c_void, buf.len(), 0) };
        if bytes_read <= 0 {
            return None;
        }
        parse_u32_branchless(&buf[..bytes_read as usize])
    }
}

impl Drop for SysfsFd {
    fn drop(&mut self) {
        unsafe {
            close(self.0);
        }
    }
}

// Safety: raw FDs are process-global and we only perform atomic pread ops
unsafe impl Send for SysfsFd {}
unsafe impl Sync for SysfsFd {}

static GPU_CLK_FD: LazyLock<Option<SysfsFd>> =
    LazyLock::new(|| SysfsFd::open(b"/sys/class/kgsl/kgsl-3d0/gpuclk\0"));

/// Branchless integer parse: reduces branch mispredictions on Cortex-A78.
/// Processes only ASCII digit bytes [0x30..0x39] terminated by newline/non-digit.
#[inline(always)]
fn parse_u32_branchless(buf: &[u8]) -> Option<u32> {
    let mut val: u32 = 0;
    let mut found_digit = false;
    let mut i = 0;
    while i < buf.len() {
        let b = buf[i];
        let is_digit = b.wrapping_sub(b'0') <= 9;
        if is_digit {
            val = val.wrapping_mul(10).wrapping_add((b - b'0') as u32);
            found_digit = true;
        } else if found_digit || b == b'\n' {
            break;
        }
        i += 1;
    }
    if found_digit {
        Some(val)
    } else {
        None
    }
}

#[inline(always)]
pub fn get_gpu_clock() -> Option<u32> {
    GPU_CLK_FD.as_ref().and_then(|fd| fd.read_u32())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_u32_branchless() {
        assert_eq!(parse_u32_branchless(b"42000\n"), Some(42000));
        assert_eq!(parse_u32_branchless(b"500000000\n"), Some(500_000_000));
        assert_eq!(parse_u32_branchless(b"0\n"), Some(0));
        assert_eq!(parse_u32_branchless(b"\n"), None);
        assert_eq!(parse_u32_branchless(b""), None);
        assert_eq!(parse_u32_branchless(b"  123\n"), Some(123));
    }
}
