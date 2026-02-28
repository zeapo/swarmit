use anyhow::Result;
use clap::Args;

use crate::events::log::read_operations;

use crate::cli::output::{print_json_ok, OutputMode};
use crate::cli::Cli;

use super::init::require_project_root;

#[derive(Args, Debug)]
pub struct LogArgs {
    /// Show last N operations
    #[arg(long, short = 'n', default_value = "20")]
    pub tail: usize,

    /// Filter by agent ID
    #[arg(long)]
    pub agent: Option<String>,

    /// Show operations since timestamp (RFC3339)
    #[arg(long)]
    pub since: Option<String>,
}

pub fn run(args: &LogArgs, cli: &Cli) -> Result<()> {
    let root = require_project_root(cli)?;
    let log_path = root.join(".swarmit").join("operations.log");

    let all_ops = read_operations(&log_path).map_err(|e| anyhow::anyhow!("{}", e))?;

    let ops: Vec<_> = all_ops
        .iter()
        .filter(|op| {
            args.agent.as_deref().map_or(true, |a| op.agent.as_str() == a)
        })
        .rev()
        .take(args.tail)
        .collect();

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => {
            let data: Vec<_> = ops
                .iter()
                .map(|op| {
                    serde_json::json!({
                        "id": op.id.to_string(),
                        "agent": op.agent.to_string(),
                        "timestamp": op.timestamp.to_rfc3339(),
                        "type": format!("{:?}", op.kind).split('{').next().unwrap_or("").trim().to_string(),
                    })
                })
                .collect();
            print_json_ok(data);
        }
        OutputMode::Pretty => {
            if ops.is_empty() {
                println!("No operations found.");
            } else {
                for op in ops.iter().rev() {
                    let kind_name = format!("{:?}", op.kind);
                    let kind_short = kind_name.split('{').next().unwrap_or("Unknown").trim();
                    println!(
                        "[{}] {} — {}",
                        op.timestamp.format("%Y-%m-%d %H:%M:%S"),
                        op.agent,
                        kind_short
                    );
                }
            }
        }
    }

    Ok(())
}
