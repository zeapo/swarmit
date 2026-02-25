use anyhow::Result;
use clap::{Args, Subcommand};

use swarmit_core::events::log::append_operation;
use swarmit_core::events::locking::try_append_with_timeout;
use swarmit_core::events::operations::{Operation, OperationKind};
use swarmit_core::models::{AgentId, ItemId};
use swarmit_core::state::ProjectState;

use crate::output::{print_json_ok, OutputMode};
use crate::Cli;

use super::epic::{parse_priority, parse_status};
use super::init::{require_project_root, resolve_agent};

#[derive(Args, Debug)]
pub struct TaskArgs {
    #[command(subcommand)]
    pub command: TaskCommands,
}

#[derive(Subcommand, Debug)]
pub enum TaskCommands {
    Create(TaskCreateArgs),
    List(TaskListArgs),
    Show(TaskShowArgs),
    Update(TaskUpdateArgs),
    Delete(TaskDeleteArgs),
    Claim(TaskClaimArgs),
    Done(TaskDoneArgs),
}

#[derive(Args, Debug)]
pub struct TaskCreateArgs {
    #[arg(long)]
    pub title: String,
    #[arg(long)]
    pub description: Option<String>,
    #[arg(long, default_value = "medium")]
    pub priority: String,
    #[arg(long)]
    pub epic: Option<String>,
    #[arg(long)]
    pub agent: Option<String>,
}

#[derive(Args, Debug)]
pub struct TaskListArgs {
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long)]
    pub epic: Option<String>,
    #[arg(long)]
    pub assignee: Option<String>,
    #[arg(long)]
    pub agent: Option<String>,
}

#[derive(Args, Debug)]
pub struct TaskShowArgs {
    pub id: String,
}

#[derive(Args, Debug)]
pub struct TaskUpdateArgs {
    pub id: String,
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub description: Option<String>,
    #[arg(long)]
    pub priority: Option<String>,
    #[arg(long)]
    pub epic: Option<String>,
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long)]
    pub assignee: Option<String>,
    #[arg(long)]
    pub agent: Option<String>,
}

#[derive(Args, Debug)]
pub struct TaskDeleteArgs {
    pub id: String,
    #[arg(long)]
    pub agent: Option<String>,
}

#[derive(Args, Debug)]
pub struct TaskClaimArgs {
    pub id: String,
    #[arg(long)]
    pub agent: Option<String>,
}

#[derive(Args, Debug)]
pub struct TaskDoneArgs {
    pub id: String,
    #[arg(long)]
    pub agent: Option<String>,
}

pub fn run(args: &TaskArgs, cli: &Cli) -> Result<()> {
    match &args.command {
        TaskCommands::Create(a) => create(a, cli),
        TaskCommands::List(a) => list(a, cli),
        TaskCommands::Show(a) => show(a, cli),
        TaskCommands::Update(a) => update(a, cli),
        TaskCommands::Delete(a) => delete(a, cli),
        TaskCommands::Claim(a) => claim(a, cli),
        TaskCommands::Done(a) => done(a, cli),
    }
}

fn create(args: &TaskCreateArgs, cli: &Cli) -> Result<()> {
    let agent_str = resolve_agent(cli, &args.agent)?;
    let agent = AgentId::new(&agent_str).map_err(|e| anyhow::anyhow!("{}", e))?;
    let root = require_project_root(cli)?;
    let swarmit = root.join(".swarmit");
    let log_path = swarmit.join("operations.log");
    let lock_path = swarmit.join("operations.lock");

    let state = ProjectState::from_log(&log_path).map_err(|e| anyhow::anyhow!("{}", e))?;
    let next_id = ItemId::new(
        &state.config.as_ref().map(|c| c.task_prefix.clone()).unwrap_or_else(|| "TASK".to_string()),
        state.task_seq + 1,
    );

    let priority = parse_priority(&args.priority)?;
    let epic_id = args
        .epic
        .as_deref()
        .map(|s| s.parse::<ItemId>().map_err(|e: swarmit_core::SwarmitError| anyhow::anyhow!("{}", e)))
        .transpose()?;

    // Validate epic exists if provided
    if let Some(eid) = &epic_id {
        if !state.epics.contains_key(eid) {
            anyhow::bail!("Epic not found: {}", eid);
        }
    }

    let op = Operation::new(
        agent,
        OperationKind::CreateTask {
            id: next_id.clone(),
            title: args.title.clone(),
            description: args.description.clone(),
            priority,
            epic_id,
        },
    );

    try_append_with_timeout(&lock_path, || append_operation(&log_path, &op))
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => print_json_ok(serde_json::json!({
            "id": next_id.to_string(),
            "title": args.title,
        })),
        OutputMode::Pretty => println!("Created task {} — {}", next_id, args.title),
    }

    Ok(())
}

fn list(args: &TaskListArgs, cli: &Cli) -> Result<()> {
    let root = require_project_root(cli)?;
    let log_path = root.join(".swarmit").join("operations.log");
    let state = ProjectState::from_log(&log_path).map_err(|e| anyhow::anyhow!("{}", e))?;

    let status_filter = args.status.as_deref().map(parse_status).transpose()?;
    let epic_filter = args
        .epic
        .as_deref()
        .map(|s| s.parse::<ItemId>().map_err(|e: swarmit_core::SwarmitError| anyhow::anyhow!("{}", e)))
        .transpose()?;
    let assignee_filter = args
        .assignee
        .as_deref()
        .map(|a| AgentId::new(a).map_err(|e| anyhow::anyhow!("{}", e)))
        .transpose()?;

    let tasks: Vec<_> = state
        .tasks
        .values()
        .filter(|t| {
            status_filter.map_or(true, |s| t.status == s)
                && epic_filter.as_ref().map_or(true, |eid| t.epic_id.as_ref() == Some(eid))
                && assignee_filter
                    .as_ref()
                    .map_or(true, |a| t.assignee.as_ref() == Some(a))
        })
        .collect();

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => {
            let data: Vec<_> = tasks
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "id": t.id.to_string(),
                        "title": t.title,
                        "status": t.status.to_string(),
                        "priority": t.priority.to_string(),
                        "epic_id": t.epic_id.as_ref().map(|e| e.to_string()),
                        "assignee": t.assignee.as_ref().map(|a| a.to_string()),
                    })
                })
                .collect();
            print_json_ok(data);
        }
        OutputMode::Pretty => {
            if tasks.is_empty() {
                println!("No tasks found.");
                return Ok(());
            }
            println!(
                "{:<12} {:<12} {:<8} {:<8} {}",
                "ID", "ASSIGNEE", "STATUS", "PRIORITY", "TITLE"
            );
            println!("{}", "-".repeat(70));
            for t in &tasks {
                println!(
                    "{:<12} {:<12} {:<8} {:<8} {}",
                    t.id,
                    t.assignee.as_ref().map(|a| a.to_string()).unwrap_or_else(|| "-".to_string()),
                    format!("{}", t.status),
                    format!("{}", t.priority),
                    t.title,
                );
            }
        }
    }

    Ok(())
}

fn show(args: &TaskShowArgs, cli: &Cli) -> Result<()> {
    let root = require_project_root(cli)?;
    let log_path = root.join(".swarmit").join("operations.log");
    let state = ProjectState::from_log(&log_path).map_err(|e| anyhow::anyhow!("{}", e))?;

    let id: ItemId = args.id.parse().map_err(|e: swarmit_core::SwarmitError| anyhow::anyhow!("{}", e))?;
    let task = state
        .tasks
        .get(&id)
        .ok_or_else(|| anyhow::anyhow!("Task not found: {}", id))?;

    let rels = state.relationships_for(&id);
    let comments = state.comments_for(&id);

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => {
            let rels_data: Vec<_> = rels
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "from": r.from.to_string(),
                        "to": r.to.to_string(),
                        "type": r.rel_type.to_string(),
                    })
                })
                .collect();
            let comments_data: Vec<_> = comments
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
            print_json_ok(serde_json::json!({
                "id": task.id.to_string(),
                "title": task.title,
                "description": task.description,
                "status": task.status.to_string(),
                "priority": task.priority.to_string(),
                "epic_id": task.epic_id.as_ref().map(|e| e.to_string()),
                "assignee": task.assignee.as_ref().map(|a| a.to_string()),
                "created_at": task.created_at.to_rfc3339(),
                "relationships": rels_data,
                "comments": comments_data,
            }));
        }
        OutputMode::Pretty => {
            println!("Task: {} — {}", task.id, task.title);
            println!("  Status:   {}", task.status);
            println!("  Priority: {}", task.priority);
            if let Some(eid) = &task.epic_id {
                println!("  Epic:     {}", eid);
            }
            if let Some(a) = &task.assignee {
                println!("  Assignee: {}", a);
            }
            if let Some(d) = &task.description {
                println!("\n{}", d);
            }
            if !rels.is_empty() {
                println!("\nRelationships:");
                for r in &rels {
                    println!("  {}", r);
                }
            }
            if !comments.is_empty() {
                println!("\nComments ({}):", comments.len());
                for c in &comments {
                    println!("  [{}] {}: {}", c.created_at.format("%Y-%m-%d %H:%M"), c.author, c.body);
                }
            }
        }
    }

    Ok(())
}

fn update(args: &TaskUpdateArgs, cli: &Cli) -> Result<()> {
    let agent_str = resolve_agent(cli, &args.agent)?;
    let agent = AgentId::new(&agent_str).map_err(|e| anyhow::anyhow!("{}", e))?;
    let root = require_project_root(cli)?;
    let swarmit = root.join(".swarmit");
    let log_path = swarmit.join("operations.log");
    let lock_path = swarmit.join("operations.lock");

    let id: ItemId = args.id.parse().map_err(|e: swarmit_core::SwarmitError| anyhow::anyhow!("{}", e))?;

    let state = ProjectState::from_log(&log_path).map_err(|e| anyhow::anyhow!("{}", e))?;
    if !state.tasks.contains_key(&id) {
        anyhow::bail!("Task not found: {}", id);
    }

    let priority = args.priority.as_deref().map(parse_priority).transpose()?;
    let epic_id = args
        .epic
        .as_deref()
        .map(|s| s.parse::<ItemId>().map_err(|e: swarmit_core::SwarmitError| anyhow::anyhow!("{}", e)))
        .transpose()?;
    let assignee = args
        .assignee
        .as_deref()
        .map(|a| AgentId::new(a).map_err(|e| anyhow::anyhow!("{}", e)))
        .transpose()?;

    let ops_to_write: Vec<Operation> = {
        let mut ops = Vec::new();
        ops.push(Operation::new(
            agent.clone(),
            OperationKind::UpdateTask {
                id: id.clone(),
                title: args.title.clone(),
                description: args.description.clone(),
                priority,
                epic_id: epic_id.map(Some),
                assignee: assignee.map(Some),
            },
        ));
        if let Some(status_str) = &args.status {
            let status = parse_status(status_str)?;
            ops.push(Operation::new(
                agent,
                OperationKind::UpdateTaskStatus { id: id.clone(), status },
            ));
        }
        ops
    };

    try_append_with_timeout(&lock_path, || {
        for op in &ops_to_write {
            append_operation(&log_path, op)?;
        }
        Ok(())
    })
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => print_json_ok(serde_json::json!({ "id": id.to_string() })),
        OutputMode::Pretty => println!("Updated task {}", id),
    }

    Ok(())
}

fn delete(args: &TaskDeleteArgs, cli: &Cli) -> Result<()> {
    let agent_str = resolve_agent(cli, &args.agent)?;
    let agent = AgentId::new(&agent_str).map_err(|e| anyhow::anyhow!("{}", e))?;
    let root = require_project_root(cli)?;
    let swarmit = root.join(".swarmit");
    let log_path = swarmit.join("operations.log");
    let lock_path = swarmit.join("operations.lock");

    let id: ItemId = args.id.parse().map_err(|e: swarmit_core::SwarmitError| anyhow::anyhow!("{}", e))?;
    let op = Operation::new(agent, OperationKind::DeleteTask { id: id.clone() });

    try_append_with_timeout(&lock_path, || append_operation(&log_path, &op))
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => print_json_ok(serde_json::json!({ "id": id.to_string() })),
        OutputMode::Pretty => println!("Deleted task {}", id),
    }

    Ok(())
}

fn claim(args: &TaskClaimArgs, cli: &Cli) -> Result<()> {
    let agent_str = resolve_agent(cli, &args.agent)?;
    let agent = AgentId::new(&agent_str).map_err(|e| anyhow::anyhow!("{}", e))?;
    let root = require_project_root(cli)?;
    let swarmit = root.join(".swarmit");
    let log_path = swarmit.join("operations.log");
    let lock_path = swarmit.join("operations.lock");

    let id: ItemId = args.id.parse().map_err(|e: swarmit_core::SwarmitError| anyhow::anyhow!("{}", e))?;
    let op = Operation::new(agent, OperationKind::ClaimTask { id: id.clone() });

    try_append_with_timeout(&lock_path, || append_operation(&log_path, &op))
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => print_json_ok(serde_json::json!({ "id": id.to_string(), "claimed": true })),
        OutputMode::Pretty => println!("Claimed task {}", id),
    }

    Ok(())
}

fn done(args: &TaskDoneArgs, cli: &Cli) -> Result<()> {
    let agent_str = resolve_agent(cli, &args.agent)?;
    let agent = AgentId::new(&agent_str).map_err(|e| anyhow::anyhow!("{}", e))?;
    let root = require_project_root(cli)?;
    let swarmit = root.join(".swarmit");
    let log_path = swarmit.join("operations.log");
    let lock_path = swarmit.join("operations.lock");

    let id: ItemId = args.id.parse().map_err(|e: swarmit_core::SwarmitError| anyhow::anyhow!("{}", e))?;
    let op = Operation::new(agent, OperationKind::CompleteTask { id: id.clone() });

    try_append_with_timeout(&lock_path, || append_operation(&log_path, &op))
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => print_json_ok(serde_json::json!({ "id": id.to_string(), "done": true })),
        OutputMode::Pretty => println!("Completed task {}", id),
    }

    Ok(())
}
