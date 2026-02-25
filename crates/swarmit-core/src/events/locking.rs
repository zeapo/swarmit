use std::fs::OpenOptions;
use std::path::Path;
use std::time::{Duration, Instant};

use fd_lock::RwLock;

use crate::models::{Result, SwarmitError};

const DEFAULT_TIMEOUT_MS: u64 = 5_000;
const RETRY_INTERVAL_MS: u64 = 10;

/// Acquires an exclusive write lock on `lock_path`, calls `f`, then releases.
/// Retries every 10ms for up to `timeout_ms` milliseconds.
pub fn with_exclusive_lock<F, T>(lock_path: &Path, timeout_ms: u64, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    let lock_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(lock_path)?;

    let mut lock = RwLock::new(lock_file);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    loop {
        match lock.try_write() {
            Ok(_guard) => {
                return f();
            }
            Err(_) => {
                if Instant::now() >= deadline {
                    return Err(SwarmitError::LockTimeout(timeout_ms));
                }
                std::thread::sleep(Duration::from_millis(RETRY_INTERVAL_MS));
            }
        }
    }
}

/// Convenience wrapper using the default 5-second timeout.
pub fn try_append_with_timeout<F, T>(lock_path: &Path, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    with_exclusive_lock(lock_path, DEFAULT_TIMEOUT_MS, f)
}
