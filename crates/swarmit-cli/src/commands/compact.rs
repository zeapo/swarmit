use anyhow::Result;
use clap::Args;
use std::fs;

use swarmit_core::events::log::{append_operation, read_operations};
use swarmit_core::events::locking::try_append_with_timeout;
use swarmit_core::events::operations::{Operation, OperationKind};
use swarmit_core::models::AgentId;
use swarmit_core::state::ProjectState;

use crate::output::{print_json_ok, OutputMode};
use crate::Cli;

use super::init::{require_project_root, resolve_agent};

#[derive(Args, Debug)]
pub struct CompactArgs {
    #[arg(long)]
    pub agent: Option<String>,
}

/// Compact the operations log by writing a snapshot and rotating the log.
/// The original log is preserved as operations.log.bak.
pub fn run(args: &CompactArgs, cli: &Cli) -> Result<()> {
    let agent_str = resolve_agent(cli, &args.agent)?;
    let agent = AgentId::new(&agent_str).map_err(|e| anyhow::anyhow!("{}", e))?;
    let root = require_project_root(cli)?;
    let swarmit = root.join(".swarmit");
    let log_path = swarmit.join("operations.log");
    let lock_path = swarmit.join("operations.lock");
    let bak_path = swarmit.join("operations.log.bak");

    let (original_count, sequence) = try_append_with_timeout(&lock_path, || {
        let ops = read_operations(&log_path).map_err(|e| {
            swarmit_core::SwarmitError::CorruptedLog(e.to_string())
        })?;
        let count = ops.len();

        let state = {
            let mut s = ProjectState::new();
            for op in ops {
                s.apply(op)?;
            }
            s
        };

        // Backup the current log
        if log_path.exists() {
            fs::copy(&log_path, &bak_path).map_err(swarmit_core::SwarmitError::Io)?;
        }

        // Write new log with just the snapshot marker
        if log_path.exists() {
            fs::remove_file(&log_path).map_err(swarmit_core::SwarmitError::Io)?;
        }

        let snapshot_op = Operation::new(
            agent,
            OperationKind::Snapshot {
                sequence: state.sequence,
            },
        );
        append_operation(&log_path, &snapshot_op)?;

        Ok((count, state.sequence))
    })
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => print_json_ok(serde_json::json!({
            "compacted_operations": original_count,
            "sequence": sequence,
            "backup": bak_path.display().to_string(),
        })),
        OutputMode::Pretty => {
            println!(
                "Compacted {} operations → snapshot at sequence {}",
                original_count, sequence
            );
            println!("Backup saved to: {}", bak_path.display());
        }
    }

    Ok(())
}
