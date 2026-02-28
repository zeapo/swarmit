use anyhow::{bail, Result};
use chrono::Utc;
use clap::Args;
use std::fs;
use std::path::PathBuf;

use crate::events::operations::{Operation, OperationKind};
use crate::models::{AgentId, ProjectConfig};
use crate::state::ProjectState;

use crate::cli::output::{print_json_ok, OutputMode};
use crate::cli::Cli;

#[derive(Args, Debug)]
pub struct InitArgs {
    /// Project name
    #[arg(long)]
    pub name: String,

    /// Your agent identifier
    #[arg(long)]
    pub agent: Option<String>,

    /// Epic ID prefix (default: EPIC)
    #[arg(long)]
    pub epic_prefix: Option<String>,

    /// Task ID prefix (default: TASK)
    #[arg(long)]
    pub task_prefix: Option<String>,

    /// Project description
    #[arg(long)]
    pub description: Option<String>,

    /// Enable auto-materialization of markdown on every mutation
    #[arg(long)]
    pub auto_materialize: bool,

    /// Path for materialized markdown, relative to .swarmit/ (default: state)
    #[arg(long)]
    pub materialize_path: Option<String>,
}

pub fn run(args: &InitArgs, cli: &Cli) -> Result<()> {
    let agent_str = resolve_agent(cli, &args.agent)?;
    let agent = AgentId::new(&agent_str).map_err(|e| anyhow::anyhow!("{}", e))?;

    let project_root = resolve_project_root(cli)?;

    let swarmit_dir = project_root.join(".swarmit");

    if swarmit_dir.exists() {
        bail!("Project already initialized at {}", swarmit_dir.display());
    }

    fs::create_dir_all(&swarmit_dir)?;

    // Open the DB (creates schema)
    let conn = crate::open_db(&project_root).map_err(|e| anyhow::anyhow!("{}", e))?;

    // Write the init operation
    let auto_mat = if args.auto_materialize {
        Some(true)
    } else {
        None
    };
    let op = Operation::new(
        agent.clone(),
        OperationKind::InitProject {
            name: args.name.clone(),
            description: args.description.clone(),
            epic_prefix: args.epic_prefix.clone(),
            task_prefix: args.task_prefix.clone(),
            auto_materialize: auto_mat,
            materialize_path: args.materialize_path.clone(),
        },
    );

    crate::write_operation(&conn, &op).map_err(|e| anyhow::anyhow!("{}", e))?;

    // Write project.toml
    let config = ProjectConfig {
        name: args.name.clone(),
        description: args.description.clone(),
        epic_prefix: args
            .epic_prefix
            .clone()
            .unwrap_or_else(|| "EPIC".to_string()),
        task_prefix: args
            .task_prefix
            .clone()
            .unwrap_or_else(|| "TASK".to_string()),
        auto_materialize: args.auto_materialize,
        materialize_path: args
            .materialize_path
            .clone()
            .unwrap_or_else(|| "state".to_string()),
        created_at: Utc::now(),
        created_by: agent,
    };

    let toml = toml_serialize(&config)?;
    fs::write(swarmit_dir.join("project.toml"), toml)?;

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => print_json_ok(serde_json::json!({
            "name": args.name,
            "dir": swarmit_dir.display().to_string()
        })),
        OutputMode::Pretty => {
            println!(
                "Initialized swarmit project '{}' at {}",
                args.name,
                swarmit_dir.display()
            );
        }
    }

    Ok(())
}

pub fn toml_serialize(config: &ProjectConfig) -> Result<String> {
    // Use simple manual TOML since we don't want another dep for now
    let mut s = String::new();
    s.push_str(&format!("name = {:?}\n", config.name));
    if let Some(d) = &config.description {
        s.push_str(&format!("description = {:?}\n", d));
    }
    s.push_str(&format!("epic_prefix = {:?}\n", config.epic_prefix));
    s.push_str(&format!("task_prefix = {:?}\n", config.task_prefix));
    s.push_str(&format!("auto_materialize = {}\n", config.auto_materialize));
    s.push_str(&format!(
        "materialize_path = {:?}\n",
        config.materialize_path
    ));
    s.push_str(&format!(
        "created_at = {:?}\n",
        config.created_at.to_rfc3339()
    ));
    s.push_str(&format!(
        "created_by = {:?}\n",
        config.created_by.to_string()
    ));
    Ok(s)
}

/// Resolves the agent from CLI flag, command-level arg, or SWARMIT_AGENT env var.
pub fn resolve_agent(cli: &Cli, cmd_agent: &Option<String>) -> Result<String> {
    cmd_agent
        .clone()
        .or_else(|| cli.agent.clone())
        .ok_or_else(|| {
            anyhow::anyhow!("Agent ID required. Use --agent <ID> or set SWARMIT_AGENT env var.")
        })
}

/// Finds the project root by walking up from cwd to find a .swarmit/ dir.
/// Falls back to cwd if --dir is specified.
pub fn resolve_project_root(cli: &Cli) -> Result<PathBuf> {
    if let Some(dir) = &cli.dir {
        return Ok(dir.clone());
    }

    let cwd = std::env::current_dir()?;
    let mut current = cwd.as_path();

    loop {
        if current.join(".swarmit").exists() {
            return Ok(current.to_path_buf());
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => break,
        }
    }

    // Default to cwd for init
    Ok(cwd)
}

/// Returns true if the project config has auto_materialize enabled.
pub fn should_materialize(state: &ProjectState) -> bool {
    state.config.as_ref().is_some_and(|c| c.auto_materialize)
}

/// Returns the materialization directory, resolved relative to the .swarmit/ dir.
pub fn materialize_path(swarmit_dir: &std::path::Path, state: &ProjectState) -> PathBuf {
    let rel = state
        .config
        .as_ref()
        .map(|c| c.materialize_path.as_str())
        .unwrap_or("state");
    swarmit_dir.join(rel)
}

/// Like resolve_project_root but fails if .swarmit doesn't exist.
pub fn require_project_root(cli: &Cli) -> Result<PathBuf> {
    if let Some(dir) = &cli.dir {
        let swarmit = dir.join(".swarmit");
        if !swarmit.exists() {
            bail!("No .swarmit directory found at {}", dir.display());
        }
        return Ok(dir.clone());
    }

    let cwd = std::env::current_dir()?;
    let mut current = cwd.as_path();

    loop {
        if current.join(".swarmit").exists() {
            return Ok(current.to_path_buf());
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => break,
        }
    }

    bail!("Not in a swarmit project. Run `swarmit init` first.")
}
