use anyhow::Result;
use clap::{Args, Subcommand};

use crate::events::operations::{Operation, OperationKind};
use crate::models::{AgentId, ItemId, SwarmitError};
use uuid::Uuid;

use crate::cli::output::{print_json_ok, OutputMode};
use crate::cli::Cli;

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

    let task_id: ItemId = args
        .task_id
        .parse()
        .map_err(|e: SwarmitError| anyhow::anyhow!("{}", e))?;

    let conn = crate::open_db(&root).map_err(|e| anyhow::anyhow!("{}", e))?;
    let state = crate::load_state(&conn).map_err(|e| anyhow::anyhow!("{}", e))?;
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

    crate::write_operation(&conn, &op).map_err(|e| anyhow::anyhow!("{}", e))?;

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
    let conn = crate::open_db(&root).map_err(|e| anyhow::anyhow!("{}", e))?;
    let state = crate::load_state(&conn).map_err(|e| anyhow::anyhow!("{}", e))?;

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
