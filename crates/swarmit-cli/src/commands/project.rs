use anyhow::Result;
use clap::{Args, Subcommand};

use swarmit_core::events::log::append_operation;
use swarmit_core::events::locking::try_append_with_timeout;
use swarmit_core::events::operations::{Operation, OperationKind};
use swarmit_core::models::AgentId;
use swarmit_core::state::ProjectState;

use crate::output::{print_json_ok, OutputMode};
use crate::Cli;

use super::init::{require_project_root, resolve_agent, toml_serialize};

#[derive(Args, Debug)]
pub struct ProjectArgs {
    #[command(subcommand)]
    pub command: ProjectCommands,
}

#[derive(Subcommand, Debug)]
pub enum ProjectCommands {
    /// Show project configuration
    Show(ProjectShowArgs),
    /// Update project settings
    Update(ProjectUpdateArgs),
}

#[derive(Args, Debug)]
pub struct ProjectShowArgs {}

#[derive(Args, Debug)]
pub struct ProjectUpdateArgs {
    /// New project name
    #[arg(long)]
    pub name: Option<String>,

    /// New project description
    #[arg(long, conflicts_with = "clear_description")]
    pub description: Option<String>,

    /// Clear the project description
    #[arg(long)]
    pub clear_description: bool,

    #[arg(long)]
    pub agent: Option<String>,
}

pub fn run(args: &ProjectArgs, cli: &Cli) -> Result<()> {
    match &args.command {
        ProjectCommands::Show(a) => show(a, cli),
        ProjectCommands::Update(a) => update(a, cli),
    }
}

fn show(_args: &ProjectShowArgs, cli: &Cli) -> Result<()> {
    let root = require_project_root(cli)?;
    let log_path = root.join(".swarmit").join("operations.log");
    let state = ProjectState::from_log(&log_path).map_err(|e| anyhow::anyhow!("{}", e))?;

    let config = state
        .config
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Project not initialized"))?;

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => {
            print_json_ok(serde_json::json!({
                "name": config.name,
                "description": config.description,
                "epic_prefix": config.epic_prefix,
                "task_prefix": config.task_prefix,
                "created_at": config.created_at.to_rfc3339(),
                "created_by": config.created_by.to_string(),
            }));
        }
        OutputMode::Pretty => {
            println!("Project: {}", config.name);
            if let Some(d) = &config.description {
                println!("  Description: {}", d);
            }
            println!("  Epic prefix:  {}", config.epic_prefix);
            println!("  Task prefix:  {}", config.task_prefix);
            println!("  Created at:   {}", config.created_at.format("%Y-%m-%d %H:%M:%S UTC"));
            println!("  Created by:   {}", config.created_by);
        }
    }

    Ok(())
}

fn update(args: &ProjectUpdateArgs, cli: &Cli) -> Result<()> {
    if args.name.is_none() && args.description.is_none() && !args.clear_description {
        anyhow::bail!("Nothing to update. Provide --name, --description, or --clear-description.");
    }

    let agent_str = resolve_agent(cli, &args.agent)?;
    let agent = AgentId::new(&agent_str).map_err(|e| anyhow::anyhow!("{}", e))?;
    let root = require_project_root(cli)?;
    let swarmit = root.join(".swarmit");
    let log_path = swarmit.join("operations.log");
    let lock_path = swarmit.join("operations.lock");

    // Verify project is initialized
    let state = ProjectState::from_log(&log_path).map_err(|e| anyhow::anyhow!("{}", e))?;
    let config = state
        .config
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Project not initialized"))?;

    let op = Operation::new(
        agent,
        OperationKind::UpdateProject {
            name: args.name.clone(),
            description: if args.clear_description { None } else { args.description.clone() },
            clear_description: args.clear_description,
        },
    );

    try_append_with_timeout(&lock_path, || append_operation(&log_path, &op))
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Regenerate project.toml to keep it in sync
    let post_state = ProjectState::from_log(&log_path).map_err(|e| anyhow::anyhow!("{}", e))?;
    if let Some(updated_config) = &post_state.config {
        let toml = toml_serialize(updated_config)?;
        std::fs::write(swarmit.join("project.toml"), toml)?;
    }

    let new_name = args.name.as_deref().unwrap_or(&config.name);
    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => print_json_ok(serde_json::json!({ "name": new_name })),
        OutputMode::Pretty => println!("Updated project '{}'", new_name),
    }

    Ok(())
}
