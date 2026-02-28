use anyhow::Result;
use clap::{Args, Subcommand};

use crate::events::locking::try_append_with_timeout;
use crate::events::log::append_operation;
use crate::events::operations::{Operation, OperationKind};
use crate::models::{AgentId, ItemId};
use crate::state::markdown;
use crate::state::ProjectState;

use crate::cli::output::{print_json_ok, OutputMode};
use crate::cli::Cli;

use super::epic::{parse_priority, parse_status};
use super::init::{materialize_path, require_project_root, resolve_agent, should_materialize};

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

/// Materializes the task's markdown file from the given state.
fn materialize_task_from_state(
    state_dir: &std::path::Path,
    state: &ProjectState,
    task_id: &ItemId,
) -> Result<()> {
    if let Some(task) = state.tasks.get(task_id) {
        match &task.epic_id {
            Some(eid) => {
                if let Some(epic) = state.epics.get(eid) {
                    let tasks = state.tasks_for_epic(eid);
                    markdown::materialize_epic(state_dir, epic, &tasks)
                        .map_err(|e| anyhow::anyhow!("Failed to materialize markdown: {}", e))?;
                }
            }
            None => {
                markdown::materialize_backlog_task(state_dir, task)
                    .map_err(|e| anyhow::anyhow!("Failed to materialize markdown: {}", e))?;
            }
        }
    }
    Ok(())
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
    let snapshot_path = swarmit.join("state.snap");

    let (state, log_offset) = crate::load_state(&root).map_err(|e| anyhow::anyhow!("{}", e))?;
    let next_id = ItemId::new(
        &state
            .config
            .as_ref()
            .map(|c| c.task_prefix.clone())
            .unwrap_or_else(|| "TASK".to_string()),
        state.task_seq + 1,
    );

    let priority = parse_priority(&args.priority)?;
    let epic_id = args
        .epic
        .as_deref()
        .map(|s| {
            s.parse::<ItemId>()
                .map_err(|e: crate::SwarmitError| anyhow::anyhow!("{}", e))
        })
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

    let (post_state, _) = crate::load_state(&root).map_err(|e| anyhow::anyhow!("{}", e))?;
    if should_materialize(&post_state) {
        let state_dir = materialize_path(&swarmit, &post_state);
        materialize_task_from_state(&state_dir, &post_state, &next_id)?;
    }

    let log_len = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
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
            "id": next_id.to_string(),
            "title": args.title,
        })),
        OutputMode::Pretty => println!("Created task {} — {}", next_id, args.title),
    }

    Ok(())
}

fn list(args: &TaskListArgs, cli: &Cli) -> Result<()> {
    let root = require_project_root(cli)?;
    let (state, _log_offset) = crate::load_state(&root).map_err(|e| anyhow::anyhow!("{}", e))?;

    let status_filter = args.status.as_deref().map(parse_status).transpose()?;
    let epic_filter = args
        .epic
        .as_deref()
        .map(|s| {
            s.parse::<ItemId>()
                .map_err(|e: crate::SwarmitError| anyhow::anyhow!("{}", e))
        })
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
            status_filter.is_none_or(|s| t.status == s)
                && epic_filter
                    .as_ref()
                    .is_none_or(|eid| t.epic_id.as_ref() == Some(eid))
                && assignee_filter
                    .as_ref()
                    .is_none_or(|a| t.assignee.as_ref() == Some(a))
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
                "{:<12} {:<12} {:<8} {:<8} TITLE",
                "ID", "ASSIGNEE", "STATUS", "PRIORITY"
            );
            println!("{}", "-".repeat(70));
            for t in &tasks {
                println!(
                    "{:<12} {:<12} {:<8} {:<8} {}",
                    t.id,
                    t.assignee
                        .as_ref()
                        .map(|a| a.to_string())
                        .unwrap_or_else(|| "-".to_string()),
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
    let (state, _log_offset) = crate::load_state(&root).map_err(|e| anyhow::anyhow!("{}", e))?;

    let id: ItemId = args
        .id
        .parse()
        .map_err(|e: crate::SwarmitError| anyhow::anyhow!("{}", e))?;
    let task = state
        .tasks
        .get(&id)
        .ok_or_else(|| anyhow::anyhow!("Task not found: {}", id))?;

    let rels = state.relationships_for(&id);
    let comments = state.comments_for(&id);
    let insights = state.insights_for(&id);

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
            let insights_data: Vec<_> = insights
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
                "insights": insights_data,
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
                    println!(
                        "  [{}] {}: {}",
                        c.created_at.format("%Y-%m-%d %H:%M"),
                        c.author,
                        c.body
                    );
                }
            }
            if !insights.is_empty() {
                println!("\nInsights ({}):", insights.len());
                for i in &insights {
                    println!(
                        "  [{}] {} — {}",
                        i.created_at.format("%Y-%m-%d %H:%M"),
                        i.author,
                        i.file_path,
                    );
                    if let Some(before) = &i.before_snippet {
                        println!("    Before: {}", before);
                    }
                    if let Some(after) = &i.after_snippet {
                        println!("    After:  {}", after);
                    }
                    println!("    {}", i.body);
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
    let snapshot_path = swarmit.join("state.snap");

    let id: ItemId = args
        .id
        .parse()
        .map_err(|e: crate::SwarmitError| anyhow::anyhow!("{}", e))?;

    let (state, log_offset) = crate::load_state(&root).map_err(|e| anyhow::anyhow!("{}", e))?;
    if !state.tasks.contains_key(&id) {
        anyhow::bail!("Task not found: {}", id);
    }
    let pre_epic_id = state.tasks.get(&id).and_then(|t| t.epic_id.clone());

    let priority = args.priority.as_deref().map(parse_priority).transpose()?;
    let epic_id = args
        .epic
        .as_deref()
        .map(|s| {
            s.parse::<ItemId>()
                .map_err(|e: crate::SwarmitError| anyhow::anyhow!("{}", e))
        })
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
                OperationKind::UpdateTaskStatus {
                    id: id.clone(),
                    status,
                },
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

    let (post_state, _) = crate::load_state(&root).map_err(|e| anyhow::anyhow!("{}", e))?;
    if should_materialize(&post_state) {
        let state_dir = materialize_path(&swarmit, &post_state);
        let post_epic_id = post_state.tasks.get(&id).and_then(|t| t.epic_id.clone());
        // If task moved epics, clean up the stale file in the old epic directory
        if pre_epic_id != post_epic_id {
            if let Some(old_eid) = &pre_epic_id {
                if let Some(old_epic) = state.epics.get(old_eid) {
                    markdown::remove_task_file(&state_dir, &id, Some(old_epic))
                        .map_err(|e| anyhow::anyhow!("Failed to remove stale markdown: {}", e))?;
                    // Re-materialize old epic so its directory reflects the removed task
                    let old_tasks = post_state.tasks_for_epic(old_eid);
                    markdown::materialize_epic(&state_dir, old_epic, &old_tasks)
                        .map_err(|e| anyhow::anyhow!("Failed to materialize markdown: {}", e))?;
                }
            }
        }
        materialize_task_from_state(&state_dir, &post_state, &id)?;
    }

    let log_len = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
    let _ = crate::check_and_write_snapshot(
        &log_path,
        &snapshot_path,
        log_len,
        log_offset,
        &post_state,
    );

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
    let snapshot_path = swarmit.join("state.snap");

    let id: ItemId = args
        .id
        .parse()
        .map_err(|e: crate::SwarmitError| anyhow::anyhow!("{}", e))?;

    let (pre_state, log_offset) = crate::load_state(&root).map_err(|e| anyhow::anyhow!("{}", e))?;
    let pre_epic = pre_state
        .tasks
        .get(&id)
        .and_then(|t| t.epic_id.as_ref())
        .and_then(|eid| pre_state.epics.get(eid))
        .cloned();

    let op = Operation::new(agent, OperationKind::DeleteTask { id: id.clone() });

    try_append_with_timeout(&lock_path, || append_operation(&log_path, &op))
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    if should_materialize(&pre_state) {
        let state_dir = materialize_path(&swarmit, &pre_state);
        markdown::remove_task_file(&state_dir, &id, pre_epic.as_ref())
            .map_err(|e| anyhow::anyhow!("Failed to remove markdown: {}", e))?;
    }

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
    let snapshot_path = swarmit.join("state.snap");

    let (_, log_offset) = crate::load_state(&root).map_err(|e| anyhow::anyhow!("{}", e))?;

    let id: ItemId = args
        .id
        .parse()
        .map_err(|e: crate::SwarmitError| anyhow::anyhow!("{}", e))?;
    let op = Operation::new(agent, OperationKind::ClaimTask { id: id.clone() });

    try_append_with_timeout(&lock_path, || append_operation(&log_path, &op))
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let (post_state, _) = crate::load_state(&root).map_err(|e| anyhow::anyhow!("{}", e))?;
    if should_materialize(&post_state) {
        let state_dir = materialize_path(&swarmit, &post_state);
        materialize_task_from_state(&state_dir, &post_state, &id)?;
    }

    let log_len = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
    let _ = crate::check_and_write_snapshot(
        &log_path,
        &snapshot_path,
        log_len,
        log_offset,
        &post_state,
    );

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => {
            print_json_ok(serde_json::json!({ "id": id.to_string(), "claimed": true }))
        }
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
    let snapshot_path = swarmit.join("state.snap");

    let (_, log_offset) = crate::load_state(&root).map_err(|e| anyhow::anyhow!("{}", e))?;

    let id: ItemId = args
        .id
        .parse()
        .map_err(|e: crate::SwarmitError| anyhow::anyhow!("{}", e))?;
    let op = Operation::new(agent, OperationKind::CompleteTask { id: id.clone() });

    try_append_with_timeout(&lock_path, || append_operation(&log_path, &op))
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let (post_state, _) = crate::load_state(&root).map_err(|e| anyhow::anyhow!("{}", e))?;
    if should_materialize(&post_state) {
        let state_dir = materialize_path(&swarmit, &post_state);
        materialize_task_from_state(&state_dir, &post_state, &id)?;
    }

    let log_len = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
    let _ = crate::check_and_write_snapshot(
        &log_path,
        &snapshot_path,
        log_len,
        log_offset,
        &post_state,
    );

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => {
            print_json_ok(serde_json::json!({ "id": id.to_string(), "done": true }))
        }
        OutputMode::Pretty => println!("Completed task {}", id),
    }

    Ok(())
}
