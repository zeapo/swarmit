use anyhow::Result;
use clap::Args;

use crate::cli::output::{print_json_ok, OutputMode};
use crate::cli::Cli;

use super::init::require_project_root;

#[derive(Args, Debug)]
pub struct CompactArgs {
    /// Also truncate the operations log to reclaim disk space
    #[arg(long)]
    pub truncate: bool,
}

/// Delete operations from the DB (materialized state tables are preserved). VACUUM to reclaim space.
pub fn run(args: &CompactArgs, cli: &Cli) -> Result<()> {
    let root = require_project_root(cli)?;

    let conn = crate::open_db(&root).map_err(|e| anyhow::anyhow!("{}", e))?;

    let op_count = crate::count_operations(&conn).map_err(|e| anyhow::anyhow!("{}", e))?;

    if args.truncate {
        crate::compact_db(&conn).map_err(|e| anyhow::anyhow!("{}", e))?;
    }

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => print_json_ok(serde_json::json!({
            "operations_before": op_count,
            "truncated": args.truncate,
        })),
        OutputMode::Pretty => {
            if args.truncate {
                println!(
                    "Compacted: deleted {} operations, VACUUM complete.",
                    op_count
                );
            } else {
                println!(
                    "{} operations in log. Use --truncate to remove them (state tables are preserved).",
                    op_count
                );
            }
        }
    }

    Ok(())
}
