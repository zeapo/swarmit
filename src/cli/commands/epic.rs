use anyhow::Result;
use clap::{Args, Subcommand};

use crate::events::operations::{Operation, OperationKind};
use crate::models::{AgentId, ItemId, Priority, Status};
use crate::state::markdown;

use crate::cli::output::{print_json_ok, OutputMode};
use crate::cli::Cli;

use super::init::{materialize_path, require_project_root, resolve_agent, should_materialize};

#[derive(Args, Debug)]
pub struct EpicArgs {
    #[command(subcommand)]
    pub command: EpicCommands,
}

#[derive(Subcommand, Debug)]
pub enum EpicCommands {
    /// Create a new epic
    Create(EpicCreateArgs),
    /// List epics
    List(EpicListArgs),
    /// Show epic details
    Show(EpicShowArgs),
    /// Update an epic
    Update(EpicUpdateArgs),
    /// Delete an epic
    Delete(EpicDeleteArgs),
    /// Cancel an epic and all its non-terminal tasks
    Cancel(EpicCancelArgs),
}

#[derive(Args, Debug)]
pub struct EpicCreateArgs {
    #[arg(long)]
    pub title: String,
    #[arg(long)]
    pub description: Option<String>,
    #[arg(long, default_value = "medium")]
    pub priority: String,
    #[arg(long)]
    pub agent: Option<String>,
}

#[derive(Args, Debug)]
pub struct EpicListArgs {
    #[arg(long)]
    pub status: Option<String>,
    /// Include cancelled epics in the listing
    #[arg(long)]
    pub all: bool,
    #[arg(long)]
    pub agent: Option<String>,
}

#[derive(Args, Debug)]
pub struct EpicShowArgs {
    pub id: String,
}

#[derive(Args, Debug)]
pub struct EpicUpdateArgs {
    pub id: String,
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub description: Option<String>,
    #[arg(long)]
    pub priority: Option<String>,
    #[arg(long)]
    pub assignee: Option<String>,
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long)]
    pub agent: Option<String>,
}

#[derive(Args, Debug)]
pub struct EpicDeleteArgs {
    pub id: String,
    #[arg(long)]
    pub agent: Option<String>,
}

#[derive(Args, Debug)]
pub struct EpicCancelArgs {
    pub id: String,
    #[arg(long)]
    pub reason: String,
    #[arg(long)]
    pub agent: Option<String>,
}

pub fn run(args: &EpicArgs, cli: &Cli) -> Result<()> {
    match &args.command {
        EpicCommands::Create(a) => create(a, cli),
        EpicCommands::List(a) => list(a, cli),
        EpicCommands::Show(a) => show(a, cli),
        EpicCommands::Update(a) => update(a, cli),
        EpicCommands::Delete(a) => delete(a, cli),
        EpicCommands::Cancel(a) => cancel(a, cli),
    }
}

fn create(args: &EpicCreateArgs, cli: &Cli) -> Result<()> {
    let agent_str = resolve_agent(cli, &args.agent)?;
    let agent = AgentId::new(&agent_str).map_err(|e| anyhow::anyhow!("{}", e))?;
    let root = require_project_root(cli)?;
    let swarmit = root.join(".swarmit");

    let conn = crate::open_db(&root).map_err(|e| anyhow::anyhow!("{}", e))?;

    let priority = parse_priority(&args.priority)?;

    // Atomically allocate ID + write operation (prevents TOCTOU race on epic_seq)
    let (next_id, _op) = crate::create_epic_op(
        &conn,
        agent,
        args.title.clone(),
        args.description.clone(),
        priority,
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    let post_state = crate::load_state(&conn).map_err(|e| anyhow::anyhow!("{}", e))?;
    if should_materialize(&post_state) {
        let state_dir = materialize_path(&swarmit, &post_state);
        if let Some(epic) = post_state.epics.get(&next_id) {
            let tasks = post_state.tasks_for_epic(&next_id);
            markdown::materialize_epic(&state_dir, epic, &tasks)
                .map_err(|e| anyhow::anyhow!("Failed to materialize markdown: {}", e))?;
        }
    }

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => print_json_ok(serde_json::json!({
            "id": next_id.to_string(),
            "title": args.title,
        })),
        OutputMode::Pretty => println!("Created epic {} — {}", next_id, args.title),
    }

    Ok(())
}

fn list(args: &EpicListArgs, cli: &Cli) -> Result<()> {
    let root = require_project_root(cli)?;
    let conn = crate::open_db(&root).map_err(|e| anyhow::anyhow!("{}", e))?;
    let state = crate::load_state(&conn).map_err(|e| anyhow::anyhow!("{}", e))?;

    let status_filter = args.status.as_deref().map(parse_status).transpose()?;

    let epics: Vec<_> = state
        .epics
        .values()
        .filter(|e| {
            // Default-hide cancelled unless --all or --status cancelled
            if !args.all && status_filter.is_none() && e.status == Status::Cancelled {
                return false;
            }
            status_filter.is_none_or(|s| e.status == s)
        })
        .collect();

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => {
            let data: Vec<_> = epics
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "id": e.id.to_string(),
                        "title": e.title,
                        "status": e.status.to_string(),
                        "priority": e.priority.to_string(),
                        "task_count": e.task_ids.len(),
                    })
                })
                .collect();
            print_json_ok(data);
        }
        OutputMode::Pretty => {
            if epics.is_empty() {
                println!("No epics found.");
                return Ok(());
            }
            println!("{:<12} {:<8} {:<8} TITLE", "ID", "STATUS", "PRIORITY");
            println!("{}", "-".repeat(60));
            for e in &epics {
                println!(
                    "{:<12} {:<8} {:<8} {}",
                    e.id,
                    format!("{}", e.status),
                    format!("{}", e.priority),
                    e.title
                );
            }
        }
    }

    Ok(())
}

fn show(args: &EpicShowArgs, cli: &Cli) -> Result<()> {
    let root = require_project_root(cli)?;
    let conn = crate::open_db(&root).map_err(|e| anyhow::anyhow!("{}", e))?;
    let state = crate::load_state(&conn).map_err(|e| anyhow::anyhow!("{}", e))?;

    let id: ItemId = args
        .id
        .parse()
        .map_err(|e: crate::SwarmitError| anyhow::anyhow!("{}", e))?;
    let epic = state
        .epics
        .get(&id)
        .ok_or_else(|| anyhow::anyhow!("Epic not found: {}", id))?;

    let tasks = state.tasks_for_epic(&id);

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => {
            let tasks_data: Vec<_> = tasks
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "id": t.id.to_string(),
                        "title": t.title,
                        "status": t.status.to_string(),
                    })
                })
                .collect();
            print_json_ok(serde_json::json!({
                "id": epic.id.to_string(),
                "title": epic.title,
                "description": epic.description,
                "status": epic.status.to_string(),
                "priority": epic.priority.to_string(),
                "assignee": epic.assignee.as_ref().map(|a| a.to_string()),
                "tasks": tasks_data,
            }));
        }
        OutputMode::Pretty => {
            println!("Epic: {} — {}", epic.id, epic.title);
            println!("  Status:   {}", epic.status);
            println!("  Priority: {}", epic.priority);
            if let Some(a) = &epic.assignee {
                println!("  Assignee: {}", a);
            }
            if let Some(d) = &epic.description {
                println!("\n{}", d);
            }
            if !tasks.is_empty() {
                println!("\nTasks ({}):", tasks.len());
                for t in &tasks {
                    println!("  {} [{}] {}", t.id, t.status, t.title);
                }
            }
        }
    }

    Ok(())
}

fn update(args: &EpicUpdateArgs, cli: &Cli) -> Result<()> {
    let agent_str = resolve_agent(cli, &args.agent)?;
    let agent = AgentId::new(&agent_str).map_err(|e| anyhow::anyhow!("{}", e))?;
    let root = require_project_root(cli)?;
    let swarmit = root.join(".swarmit");

    let id: ItemId = args
        .id
        .parse()
        .map_err(|e: crate::SwarmitError| anyhow::anyhow!("{}", e))?;

    let conn = crate::open_db(&root).map_err(|e| anyhow::anyhow!("{}", e))?;
    let state = crate::load_state(&conn).map_err(|e| anyhow::anyhow!("{}", e))?;
    if !state.epics.contains_key(&id) {
        anyhow::bail!("Epic not found: {}", id);
    }

    let priority = args.priority.as_deref().map(parse_priority).transpose()?;
    let assignee = args
        .assignee
        .as_deref()
        .map(|a| AgentId::new(a).map_err(|e| anyhow::anyhow!("{}", e)))
        .transpose()?;

    let update_op = Operation::new(
        agent.clone(),
        OperationKind::UpdateEpic {
            id: id.clone(),
            title: args.title.clone(),
            description: args.description.clone(),
            priority,
            assignee,
        },
    );

    let mut ops = vec![update_op];

    // Handle status update separately
    if let Some(status_str) = &args.status {
        let status = parse_status(status_str)?;
        ops.push(Operation::new(
            agent,
            OperationKind::UpdateEpicStatus {
                id: id.clone(),
                status,
            },
        ));
    }

    crate::write_operations(&conn, &ops).map_err(|e| anyhow::anyhow!("{}", e))?;

    let post_state = crate::load_state(&conn).map_err(|e| anyhow::anyhow!("{}", e))?;
    if should_materialize(&post_state) {
        let state_dir = materialize_path(&swarmit, &post_state);
        if let Some(epic) = post_state.epics.get(&id) {
            let tasks = post_state.tasks_for_epic(&id);
            markdown::materialize_epic(&state_dir, epic, &tasks)
                .map_err(|e| anyhow::anyhow!("Failed to materialize markdown: {}", e))?;
        }
    }

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => print_json_ok(serde_json::json!({ "id": id.to_string() })),
        OutputMode::Pretty => println!("Updated epic {}", id),
    }

    Ok(())
}

fn delete(args: &EpicDeleteArgs, cli: &Cli) -> Result<()> {
    let agent_str = resolve_agent(cli, &args.agent)?;
    let agent = AgentId::new(&agent_str).map_err(|e| anyhow::anyhow!("{}", e))?;
    let root = require_project_root(cli)?;
    let swarmit = root.join(".swarmit");

    let id: ItemId = args
        .id
        .parse()
        .map_err(|e: crate::SwarmitError| anyhow::anyhow!("{}", e))?;

    let conn = crate::open_db(&root).map_err(|e| anyhow::anyhow!("{}", e))?;
    let pre_state = crate::load_state(&conn).map_err(|e| anyhow::anyhow!("{}", e))?;
    let pre_epic = pre_state.epics.get(&id).cloned();

    let op = Operation::new(agent, OperationKind::DeleteEpic { id: id.clone() });

    crate::write_operation(&conn, &op).map_err(|e| anyhow::anyhow!("{}", e))?;

    if should_materialize(&pre_state) {
        let state_dir = materialize_path(&swarmit, &pre_state);
        if let Some(epic) = &pre_epic {
            markdown::remove_epic(&state_dir, epic)
                .map_err(|e| anyhow::anyhow!("Failed to remove markdown: {}", e))?;
        }
    }

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => print_json_ok(serde_json::json!({ "id": id.to_string() })),
        OutputMode::Pretty => println!("Deleted epic {}", id),
    }

    Ok(())
}

fn cancel(args: &EpicCancelArgs, cli: &Cli) -> Result<()> {
    let agent_str = resolve_agent(cli, &args.agent)?;
    let agent = AgentId::new(&agent_str).map_err(|e| anyhow::anyhow!("{}", e))?;
    let root = require_project_root(cli)?;
    let swarmit = root.join(".swarmit");

    let id: ItemId = args
        .id
        .parse()
        .map_err(|e: crate::SwarmitError| anyhow::anyhow!("{}", e))?;

    let conn = crate::open_db(&root).map_err(|e| anyhow::anyhow!("{}", e))?;
    let state = crate::load_state(&conn).map_err(|e| anyhow::anyhow!("{}", e))?;
    let epic = state
        .epics
        .get(&id)
        .ok_or_else(|| anyhow::anyhow!("Epic not found: {}", id))?;

    // Collect non-terminal task IDs for cascade info
    let cancelled_tasks: Vec<ItemId> = epic
        .task_ids
        .iter()
        .filter(|tid| {
            state
                .tasks
                .get(*tid)
                .is_some_and(|t| !t.status.is_terminal())
        })
        .cloned()
        .collect();

    let op = Operation::new(
        agent,
        OperationKind::CancelEpic {
            id: id.clone(),
            reason: args.reason.clone(),
        },
    );

    crate::write_operation(&conn, &op).map_err(|e| anyhow::anyhow!("{}", e))?;

    let post_state = crate::load_state(&conn).map_err(|e| anyhow::anyhow!("{}", e))?;
    if should_materialize(&post_state) {
        let state_dir = materialize_path(&swarmit, &post_state);
        if let Some(epic) = post_state.epics.get(&id) {
            let tasks = post_state.tasks_for_epic(&id);
            markdown::materialize_epic(&state_dir, epic, &tasks)
                .map_err(|e| anyhow::anyhow!("Failed to materialize markdown: {}", e))?;
        }
    }

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => print_json_ok(serde_json::json!({
            "id": id.to_string(),
            "cancelled": true,
            "tasks_cancelled": cancelled_tasks.iter().map(|t| t.to_string()).collect::<Vec<_>>(),
        })),
        OutputMode::Pretty => {
            println!("Cancelled epic {}", id);
            if !cancelled_tasks.is_empty() {
                println!(
                    "  Also cancelled {} task(s): {}",
                    cancelled_tasks.len(),
                    cancelled_tasks
                        .iter()
                        .map(|t| t.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
    }

    Ok(())
}

pub fn parse_priority(s: &str) -> Result<Priority> {
    match s.to_lowercase().as_str() {
        "low" => Ok(Priority::Low),
        "medium" | "med" => Ok(Priority::Medium),
        "high" => Ok(Priority::High),
        "urgent" => Ok(Priority::Urgent),
        _ => Err(anyhow::anyhow!(
            "Invalid priority '{}'. Use: low, medium, high, urgent",
            s
        )),
    }
}

pub fn parse_status(s: &str) -> Result<Status> {
    match s.to_lowercase().replace(['-', ' '], "_").as_str() {
        "todo" => Ok(Status::Todo),
        "in_progress" | "inprogress" | "wip" => Ok(Status::InProgress),
        "done" | "complete" | "completed" => Ok(Status::Done),
        "blocked" => Ok(Status::Blocked),
        "cancelled" | "canceled" => Ok(Status::Cancelled),
        _ => Err(anyhow::anyhow!(
            "Invalid status '{}'. Use: todo, in_progress, done, blocked, cancelled",
            s
        )),
    }
}
