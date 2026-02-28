use anyhow::Result;
use clap::{Args, Subcommand};

use crate::events::locking::try_append_with_timeout;
use crate::events::log::append_operation;
use crate::events::operations::{Operation, OperationKind};
use crate::models::{AgentId, ItemId, SwarmitError};
use uuid::Uuid;

use crate::cli::output::{print_json_ok, OutputMode};
use crate::cli::Cli;

use super::init::{require_project_root, resolve_agent};

#[derive(Args, Debug)]
pub struct InsightArgs {
    #[command(subcommand)]
    pub command: InsightCommands,
}

#[derive(Subcommand, Debug)]
pub enum InsightCommands {
    /// Add an insight to a task
    Add(InsightAddArgs),
    /// List insights on a task
    List(InsightListArgs),
}

#[derive(Args, Debug)]
pub struct InsightAddArgs {
    /// Task ID
    pub task_id: String,
    #[arg(long)]
    pub file: String,
    #[arg(long)]
    pub body: String,
    #[arg(long)]
    pub before: Option<String>,
    #[arg(long)]
    pub after: Option<String>,
    #[arg(long)]
    pub agent: Option<String>,
}

#[derive(Args, Debug)]
pub struct InsightListArgs {
    /// Task ID
    pub task_id: String,
}

pub fn run(args: &InsightArgs, cli: &Cli) -> Result<()> {
    match &args.command {
        InsightCommands::Add(a) => add(a, cli),
        InsightCommands::List(a) => list(a, cli),
    }
}

fn add(args: &InsightAddArgs, cli: &Cli) -> Result<()> {
    let agent_str = resolve_agent(cli, &args.agent)?;
    let agent = AgentId::new(&agent_str).map_err(|e| anyhow::anyhow!("{}", e))?;
    let root = require_project_root(cli)?;
    let swarmit = root.join(".swarmit");
    let log_path = swarmit.join("operations.log");
    let lock_path = swarmit.join("operations.lock");
    let snapshot_path = swarmit.join("state.snap");

    let task_id: ItemId = args
        .task_id
        .parse()
        .map_err(|e: SwarmitError| anyhow::anyhow!("{}", e))?;

    let (state, log_offset) = crate::load_state(&root).map_err(|e| anyhow::anyhow!("{}", e))?;
    if !state.tasks.contains_key(&task_id) {
        anyhow::bail!("Task not found: {}", task_id);
    }

    let insight_id = Uuid::now_v7();
    let op = Operation::new(
        agent,
        OperationKind::AddInsight {
            id: insight_id,
            task_id: task_id.clone(),
            file_path: args.file.clone(),
            before_snippet: args.before.clone(),
            after_snippet: args.after.clone(),
            body: args.body.clone(),
        },
    );

    try_append_with_timeout(&lock_path, || append_operation(&log_path, &op))
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let log_len = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
    let (post_state, _) = crate::load_state(&root).map_err(|e| anyhow::anyhow!("{}", e))?;
    let _ = crate::check_and_write_snapshot(
        &log_path,
        &snapshot_path,
        log_len,
        log_offset,
        &post_state,
    );

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => print_json_ok(serde_json::json!({
            "id": insight_id.to_string(),
            "task_id": task_id.to_string(),
        })),
        OutputMode::Pretty => println!("Added insight to {}", task_id),
    }

    Ok(())
}

fn list(args: &InsightListArgs, cli: &Cli) -> Result<()> {
    let root = require_project_root(cli)?;
    let (state, _log_offset) = crate::load_state(&root).map_err(|e| anyhow::anyhow!("{}", e))?;

    let task_id: ItemId = args
        .task_id
        .parse()
        .map_err(|e: SwarmitError| anyhow::anyhow!("{}", e))?;
    let insights = state.insights_for(&task_id);

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => {
            let data: Vec<_> = insights
                .iter()
                .map(|i| {
                    serde_json::json!({
                        "id": i.id.to_string(),
                        "author": i.author.to_string(),
                        "file_path": i.file_path,
                        "before_snippet": i.before_snippet,
                        "after_snippet": i.after_snippet,
                        "body": i.body,
                        "created_at": i.created_at.to_rfc3339(),
                    })
                })
                .collect();
            print_json_ok(data);
        }
        OutputMode::Pretty => {
            if insights.is_empty() {
                println!("No insights on {}", task_id);
            } else {
                for i in &insights {
                    println!(
                        "[{}] {} — {}",
                        i.created_at.format("%Y-%m-%d %H:%M"),
                        i.author,
                        i.file_path,
                    );
                    if let Some(before) = &i.before_snippet {
                        println!("  Before: {}", before);
                    }
                    if let Some(after) = &i.after_snippet {
                        println!("  After:  {}", after);
                    }
                    println!("  {}", i.body);
                    println!();
                }
            }
        }
    }

    Ok(())
}
