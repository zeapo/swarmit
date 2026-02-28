use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

use crate::state::markdown;

use crate::cli::output::{print_json_ok, OutputMode};
use crate::cli::Cli;

use super::init::require_project_root;

#[derive(Args, Debug)]
pub struct SyncArgs {
    /// Output directory for materialized markdown (overrides project config)
    #[arg(long)]
    pub path: Option<String>,
}

pub fn run(args: &SyncArgs, cli: &Cli) -> Result<()> {
    let root = require_project_root(cli)?;
    let swarmit = root.join(".swarmit");

    let (state, _log_offset) = crate::load_state(&root).map_err(|e| anyhow::anyhow!("{}", e))?;

    let state_dir: PathBuf = if let Some(p) = &args.path {
        PathBuf::from(p)
    } else {
        let rel = state
            .config
            .as_ref()
            .map(|c| c.materialize_path.as_str())
            .unwrap_or("state");
        swarmit.join(rel)
    };

    let mut epic_count = 0usize;
    let mut task_count = 0usize;

    for (epic_id, epic) in &state.epics {
        let tasks = state.tasks_for_epic(epic_id);
        task_count += tasks.len();
        markdown::materialize_epic(&state_dir, epic, &tasks)
            .map_err(|e| anyhow::anyhow!("Failed to materialize {}: {}", epic_id, e))?;
        epic_count += 1;
    }

    for task in state.tasks.values().filter(|t| t.epic_id.is_none()) {
        markdown::materialize_backlog_task(&state_dir, task)
            .map_err(|e| anyhow::anyhow!("Failed to materialize {}: {}", task.id, e))?;
        task_count += 1;
    }

    if let Some(config) = &state.config {
        let toml = super::init::toml_serialize(config)?;
        std::fs::write(swarmit.join("project.toml"), toml)?;
    }

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => print_json_ok(serde_json::json!({
            "epics": epic_count,
            "tasks": task_count,
            "path": state_dir.display().to_string(),
        })),
        OutputMode::Pretty => println!(
            "Materialized {} epic(s), {} task(s) → {}",
            epic_count,
            task_count,
            state_dir.display()
        ),
    }

    Ok(())
}
