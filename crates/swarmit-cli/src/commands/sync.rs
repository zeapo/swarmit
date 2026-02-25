use anyhow::Result;
use clap::Args;

use swarmit_core::state::markdown;
use swarmit_core::state::ProjectState;

use crate::output::{print_json_ok, OutputMode};
use crate::Cli;

use super::init::require_project_root;

#[derive(Args, Debug)]
pub struct SyncArgs {}

pub fn run(_args: &SyncArgs, cli: &Cli) -> Result<()> {
    let root = require_project_root(cli)?;
    let swarmit = root.join(".swarmit");
    let log_path = swarmit.join("operations.log");
    let state_dir = swarmit.join("state");

    let state = ProjectState::from_log(&log_path).map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut epic_count = 0usize;
    let mut task_count = 0usize;

    for (epic_id, epic) in &state.epics {
        let tasks = state.tasks_for_epic(epic_id);
        task_count += tasks.len();
        markdown::materialize_epic(&state_dir, epic, &tasks)
            .map_err(|e| anyhow::anyhow!("Failed to materialize {}: {}", epic_id, e))?;
        epic_count += 1;
    }

    let mut archived_count = 0usize;

    for task in state.backlog_tasks() {
        markdown::materialize_backlog_task(&state_dir, task)
            .map_err(|e| anyhow::anyhow!("Failed to materialize {}: {}", task.id, e))?;
        task_count += 1;
    }

    for task in state.archived_tasks() {
        markdown::materialize_archived_task(&state_dir, task)
            .map_err(|e| anyhow::anyhow!("Failed to materialize {}: {}", task.id, e))?;
        archived_count += 1;
    }

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => print_json_ok(serde_json::json!({
            "epics": epic_count,
            "tasks": task_count,
            "archived": archived_count,
        })),
        OutputMode::Pretty => println!(
            "Materialized {} epic(s), {} task(s), {} archived → {}",
            epic_count,
            task_count,
            archived_count,
            state_dir.display()
        ),
    }

    Ok(())
}
