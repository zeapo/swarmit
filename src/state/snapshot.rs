use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::state::ProjectState;
use crate::SwarmitError;

pub const MAGIC: &[u8; 8] = b"SWMSNAP\0";
pub const VERSION_V1: u16 = 1;
/// Minimum lag in bytes before auto-snapshot triggers (~50 ops floor)
pub const MIN_LAG_BYTES: u64 = 8 * 1024; // 8 KiB

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SnapshotV1 {
    pub log_offset: u64,
    pub state: ProjectState,
}

/// Writes a snapshot atomically to `snapshot_path` via a temp file + rename.
pub fn write_snapshot(snapshot_path: &Path, payload: &SnapshotV1) -> crate::models::Result<()> {
    let tmp_path = snapshot_path.parent().unwrap().join("state.snap.tmp");

    let serialized = rmp_serde::to_vec(payload).map_err(|e| {
        SwarmitError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            e.to_string(),
        ))
    })?;

    let mut tmp = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp_path)?;

    tmp.write_all(MAGIC)?;
    tmp.write_all(&VERSION_V1.to_be_bytes())?;
    tmp.write_all(&serialized)?;
    tmp.flush()?;
    tmp.sync_data()?;

    std::fs::rename(&tmp_path, snapshot_path)?;

    Ok(())
}

/// Reads a snapshot from `snapshot_path`. Returns `None` if the file is absent,
/// corrupt, or has an unknown version.
pub fn read_snapshot(snapshot_path: &Path) -> crate::models::Result<Option<SnapshotV1>> {
    if !snapshot_path.exists() {
        return Ok(None);
    }

    let bytes = std::fs::read(snapshot_path)?;

    if bytes.len() < 10 {
        eprintln!("Snapshot file too short, falling back to full log replay");
        return Ok(None);
    }

    if &bytes[0..8] != MAGIC {
        eprintln!("Snapshot magic mismatch, falling back to full log replay");
        return Ok(None);
    }

    let version = u16::from_be_bytes([bytes[8], bytes[9]]);

    match version {
        VERSION_V1 => rmp_serde::from_slice::<SnapshotV1>(&bytes[10..])
            .map(Some)
            .map_err(|e| {
                SwarmitError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                ))
            }),
        _ => {
            eprintln!("Unknown snapshot version {version}, falling back to full log replay");
            Ok(None)
        }
    }
}

/// Returns `true` when the log has grown enough beyond the last snapshot
/// offset to warrant writing a new snapshot.
pub fn should_snapshot(log_len: u64, snapshot_offset: u64) -> bool {
    let lag = log_len.saturating_sub(snapshot_offset);
    let threshold = (log_len / 10).max(MIN_LAG_BYTES) + 4096;
    lag > threshold
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_state() -> ProjectState {
        ProjectState::default()
    }

    #[test]
    fn test_round_trip() {
        let dir = tempdir().unwrap();
        let snap_path = dir.path().join("state.snap");

        let state = make_state();
        let payload = SnapshotV1 {
            log_offset: 12345,
            state,
        };

        write_snapshot(&snap_path, &payload).expect("write_snapshot should succeed");

        let loaded = read_snapshot(&snap_path)
            .expect("read_snapshot should succeed")
            .expect("snapshot should be present");

        assert_eq!(loaded.log_offset, 12345);
    }

    #[test]
    fn test_bad_magic() {
        let dir = tempdir().unwrap();
        let snap_path = dir.path().join("state.snap");

        // Write garbage bytes that are long enough but have wrong magic
        std::fs::write(&snap_path, b"BADMAGIC\x00\x01some_garbage_data").unwrap();

        let result = read_snapshot(&snap_path).expect("read_snapshot should not error");
        assert!(result.is_none(), "bad magic should return None");
    }

    #[test]
    fn test_missing_file() {
        let dir = tempdir().unwrap();
        let snap_path = dir.path().join("nonexistent.snap");

        let result = read_snapshot(&snap_path).expect("read_snapshot should not error");
        assert!(result.is_none(), "missing file should return None");
    }

    #[test]
    fn test_should_snapshot() {
        // Small log below MIN_LAG_BYTES — should not trigger snapshot
        // log_len = 1000, snapshot_offset = 0
        // lag = 1000, threshold = max(1000/10=100, 8192) + 4096 = 8192 + 4096 = 12288
        // lag (1000) <= threshold (12288) => false
        assert!(!should_snapshot(1000, 0));

        // Large lag — should trigger snapshot
        // log_len = 200_000, snapshot_offset = 0
        // lag = 200_000, threshold = max(200_000/10=20_000, 8_192) + 4096 = 20_000 + 4_096 = 24_096
        // lag (200_000) > threshold (24_096) => true
        assert!(should_snapshot(200_000, 0));

        // Log close to snapshot_offset — should not trigger
        // log_len = 100_000, snapshot_offset = 95_000
        // lag = 5_000, threshold = max(10_000, 8_192) + 4096 = 10_000 + 4_096 = 14_096
        // lag (5_000) <= threshold (14_096) => false
        assert!(!should_snapshot(100_000, 95_000));

        // Log well ahead of snapshot_offset — should trigger
        // log_len = 100_000, snapshot_offset = 0
        // lag = 100_000, threshold = max(10_000, 8_192) + 4096 = 14_096
        // lag (100_000) > threshold (14_096) => true
        assert!(should_snapshot(100_000, 0));
    }
}
