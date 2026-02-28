use anyhow::Result;
use clap::Args;

use crate::state::{read_snapshot, write_snapshot, SnapshotV1};

use crate::cli::output::{print_json_ok, OutputMode};
use crate::cli::Cli;

use super::init::require_project_root;

#[derive(Args, Debug)]
pub struct CompactArgs {
    /// Also truncate the oplog to reclaim disk space (backs up original first)
    #[arg(long)]
    pub truncate: bool,
}

/// Force-write a snapshot of current state, optionally truncating the log.
pub fn run(args: &CompactArgs, cli: &Cli) -> Result<()> {
    let root = require_project_root(cli)?;
    let swarmit = root.join(".swarmit");
    let log_path = swarmit.join("operations.log");
    let snapshot_path = swarmit.join("state.snap");

    // Get current log size before replaying
    let log_len = std::fs::metadata(&log_path)
        .map(|m| m.len())
        .unwrap_or(0);

    // Load current state (uses snapshot + oplog tail for efficiency)
    let (state, _log_offset) = crate::load_state(&root)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Write snapshot at current log end offset
    write_snapshot(&snapshot_path, &SnapshotV1 {
        log_offset: log_len,
        state,
    })
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    if args.truncate {
        // Backup the log first
        let bak_path = swarmit.join("operations.log.bak");
        std::fs::copy(&log_path, &bak_path)?;

        // Truncate log to empty (all state is now in the snapshot)
        std::fs::write(&log_path, b"")?;

        // Rewrite snapshot with offset 0, since the log is now empty
        if let Ok(Some(mut snap)) = read_snapshot(&snapshot_path)
            .map_err(|e| anyhow::anyhow!("{}", e))
        {
            snap.log_offset = 0;
            write_snapshot(&snapshot_path, &snap)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
        }

        let mode = OutputMode::detect(cli.json, cli.plain);
        match mode {
            OutputMode::Json => print_json_ok(serde_json::json!({
                "snapshot_written": true,
                "log_truncated": true,
                "backup": bak_path.display().to_string(),
            })),
            OutputMode::Pretty => {
                println!("Snapshot written. Log truncated (backup at {}).", bak_path.display());
            }
        }
    } else {
        let mode = OutputMode::detect(cli.json, cli.plain);
        match mode {
            OutputMode::Json => print_json_ok(serde_json::json!({
                "snapshot_written": true,
                "log_offset": log_len,
            })),
            OutputMode::Pretty => {
                println!("Snapshot written at log offset {} bytes.", log_len);
            }
        }
    }

    Ok(())
}
