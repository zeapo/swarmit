use anyhow::Result;
use clap::{Args, Subcommand};

use uuid::Uuid;
use swarmit_core::events::log::append_operation;
use swarmit_core::events::locking::try_append_with_timeout;
use swarmit_core::events::operations::{Operation, OperationKind};
use swarmit_core::models::{AgentId, ItemId, SwarmitError};
use swarmit_core::state::ProjectState;

use crate::output::{print_json_ok, OutputMode};
use crate::Cli;

use super::init::{require_project_root, resolve_agent};

#[derive(Args, Debug)]
pub struct CommentArgs {
    #[command(subcommand)]
    pub command: CommentCommands,
}

#[derive(Subcommand, Debug)]
pub enum CommentCommands {
    /// Add a comment to a task
    Add(CommentAddArgs),
    /// List comments on a task
    List(CommentListArgs),
}

#[derive(Args, Debug)]
pub struct CommentAddArgs {
    /// Task ID
    pub task_id: String,
    #[arg(long)]
    pub body: String,
    #[arg(long)]
    pub agent: Option<String>,
}

#[derive(Args, Debug)]
pub struct CommentListArgs {
    /// Task ID
    pub task_id: String,
}

pub fn run(args: &CommentArgs, cli: &Cli) -> Result<()> {
    match &args.command {
        CommentCommands::Add(a) => add(a, cli),
        CommentCommands::List(a) => list(a, cli),
    }
}

fn add(args: &CommentAddArgs, cli: &Cli) -> Result<()> {
    let agent_str = resolve_agent(cli, &args.agent)?;
    let agent = AgentId::new(&agent_str).map_err(|e| anyhow::anyhow!("{}", e))?;
    let root = require_project_root(cli)?;
    let swarmit = root.join(".swarmit");
    let log_path = swarmit.join("operations.log");
    let lock_path = swarmit.join("operations.lock");

    let task_id: ItemId = args
        .task_id
        .parse()
        .map_err(|e: SwarmitError| anyhow::anyhow!("{}", e))?;

    let state = ProjectState::from_log(&log_path).map_err(|e| anyhow::anyhow!("{}", e))?;
    if !state.tasks.contains_key(&task_id) {
        anyhow::bail!("Task not found: {}", task_id);
    }

    let comment_id = Uuid::now_v7();
    let op = Operation::new(
        agent,
        OperationKind::AddComment {
            id: comment_id,
            task_id: task_id.clone(),
            body: args.body.clone(),
        },
    );

    try_append_with_timeout(&lock_path, || append_operation(&log_path, &op))
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => print_json_ok(serde_json::json!({
            "id": comment_id.to_string(),
            "task_id": task_id.to_string(),
        })),
        OutputMode::Pretty => println!("Added comment to {}", task_id),
    }

    Ok(())
}

fn list(args: &CommentListArgs, cli: &Cli) -> Result<()> {
    let root = require_project_root(cli)?;
    let log_path = root.join(".swarmit").join("operations.log");
    let state = ProjectState::from_log(&log_path).map_err(|e| anyhow::anyhow!("{}", e))?;

    let task_id: ItemId = args
        .task_id
        .parse()
        .map_err(|e: SwarmitError| anyhow::anyhow!("{}", e))?;
    let comments = state.comments_for(&task_id);

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => {
            let data: Vec<_> = comments
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "id": c.id.to_string(),
                        "author": c.author.to_string(),
                        "body": c.body,
                        "created_at": c.created_at.to_rfc3339(),
                    })
                })
                .collect();
            print_json_ok(data);
        }
        OutputMode::Pretty => {
            if comments.is_empty() {
                println!("No comments on {}", task_id);
            } else {
                for c in &comments {
                    println!(
                        "[{}] {}: {}",
                        c.created_at.format("%Y-%m-%d %H:%M"),
                        c.author,
                        c.body
                    );
                }
            }
        }
    }

    Ok(())
}
