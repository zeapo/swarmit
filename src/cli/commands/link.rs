use anyhow::Result;
use clap::{Args, Subcommand};

use crate::events::operations::{Operation, OperationKind};
use crate::models::{AgentId, ItemId, RelationType, SwarmitError};
use crate::state::ProjectState;

use crate::cli::output::{print_json_ok, OutputMode};
use crate::cli::Cli;

use super::init::{require_project_root, resolve_agent};

#[derive(Args, Debug)]
pub struct LinkArgs {
    #[command(subcommand)]
    pub command: LinkCommands,
}

#[derive(Subcommand, Debug)]
pub enum LinkCommands {
    /// Add a relationship between two items
    Add(LinkAddArgs),
    /// Remove a relationship
    Remove(LinkRemoveArgs),
    /// List relationships for an item
    List(LinkListArgs),
}

#[derive(Args, Debug)]
pub struct LinkAddArgs {
    #[arg(long)]
    pub from: String,
    #[arg(long)]
    pub to: String,
    /// Relationship type: blocks, blocked_by, relates_to, duplicates, duplicated_by
    #[arg(long, default_value = "relates_to")]
    pub r#type: String,
    #[arg(long)]
    pub agent: Option<String>,
}

#[derive(Args, Debug)]
pub struct LinkRemoveArgs {
    #[arg(long)]
    pub from: String,
    #[arg(long)]
    pub to: String,
    #[arg(long, default_value = "relates_to")]
    pub r#type: String,
    #[arg(long)]
    pub agent: Option<String>,
}

#[derive(Args, Debug)]
pub struct LinkListArgs {
    /// Item ID to show relationships for
    pub id: String,
}

pub fn run(args: &LinkArgs, cli: &Cli) -> Result<()> {
    match &args.command {
        LinkCommands::Add(a) => add(a, cli),
        LinkCommands::Remove(a) => remove(a, cli),
        LinkCommands::List(a) => list(a, cli),
    }
}

fn add(args: &LinkAddArgs, cli: &Cli) -> Result<()> {
    let agent_str = resolve_agent(cli, &args.agent)?;
    let agent = AgentId::new(&agent_str).map_err(|e| anyhow::anyhow!("{}", e))?;
    let root = require_project_root(cli)?;

    let from: ItemId = args
        .from
        .parse()
        .map_err(|e: SwarmitError| anyhow::anyhow!("{}", e))?;
    let to: ItemId = args
        .to
        .parse()
        .map_err(|e: SwarmitError| anyhow::anyhow!("{}", e))?;

    // Validate no self-link
    if from == to {
        anyhow::bail!("Cannot create a self-relationship: {} -> {}", from, to);
    }

    let rel_type = parse_rel_type(&args.r#type)?;

    let conn = crate::open_db(&root).map_err(|e| anyhow::anyhow!("{}", e))?;
    let state = crate::load_state(&conn).map_err(|e| anyhow::anyhow!("{}", e))?;
    validate_item_exists(&state, &from)?;
    validate_item_exists(&state, &to)?;

    // Check for duplicate
    let existing = state
        .relationships
        .iter()
        .any(|r| r.from == from && r.to == to && r.rel_type == rel_type);
    if existing {
        anyhow::bail!("Relationship already exists: {} {} {}", from, rel_type, to);
    }

    let op = Operation::new(
        agent.clone(),
        OperationKind::AddRelationship {
            from: from.clone(),
            to: to.clone(),
            rel_type,
        },
    );

    // Also add inverse for non-symmetric relationships
    let mut ops = vec![op];
    if rel_type != RelationType::RelatesTo {
        ops.push(Operation::new(
            agent,
            OperationKind::AddRelationship {
                from: to.clone(),
                to: from.clone(),
                rel_type: rel_type.inverse(),
            },
        ));
    }

    crate::write_operations(&conn, &ops).map_err(|e| anyhow::anyhow!("{}", e))?;

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => print_json_ok(serde_json::json!({
            "from": from.to_string(),
            "to": to.to_string(),
            "type": rel_type.to_string(),
        })),
        OutputMode::Pretty => println!("Added relationship: {} {} {}", from, rel_type, to),
    }

    Ok(())
}

fn remove(args: &LinkRemoveArgs, cli: &Cli) -> Result<()> {
    let agent_str = resolve_agent(cli, &args.agent)?;
    let agent = AgentId::new(&agent_str).map_err(|e| anyhow::anyhow!("{}", e))?;
    let root = require_project_root(cli)?;

    let from: ItemId = args
        .from
        .parse()
        .map_err(|e: SwarmitError| anyhow::anyhow!("{}", e))?;
    let to: ItemId = args
        .to
        .parse()
        .map_err(|e: SwarmitError| anyhow::anyhow!("{}", e))?;
    let rel_type = parse_rel_type(&args.r#type)?;

    let conn = crate::open_db(&root).map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut ops = vec![Operation::new(
        agent.clone(),
        OperationKind::RemoveRelationship {
            from: from.clone(),
            to: to.clone(),
            rel_type,
        },
    )];

    // Remove inverse too
    if rel_type != RelationType::RelatesTo {
        ops.push(Operation::new(
            agent,
            OperationKind::RemoveRelationship {
                from: to.clone(),
                to: from.clone(),
                rel_type: rel_type.inverse(),
            },
        ));
    }

    crate::write_operations(&conn, &ops).map_err(|e| anyhow::anyhow!("{}", e))?;

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => print_json_ok(serde_json::json!({ "removed": true })),
        OutputMode::Pretty => println!("Removed relationship: {} {} {}", from, rel_type, to),
    }

    Ok(())
}

fn list(args: &LinkListArgs, cli: &Cli) -> Result<()> {
    let root = require_project_root(cli)?;
    let conn = crate::open_db(&root).map_err(|e| anyhow::anyhow!("{}", e))?;
    let state = crate::load_state(&conn).map_err(|e| anyhow::anyhow!("{}", e))?;

    let id: ItemId = args
        .id
        .parse()
        .map_err(|e: SwarmitError| anyhow::anyhow!("{}", e))?;
    let rels = state.relationships_for(&id);

    let mode = OutputMode::detect(cli.json, cli.plain);
    match mode {
        OutputMode::Json => {
            let data: Vec<_> = rels
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "from": r.from.to_string(),
                        "to": r.to.to_string(),
                        "type": r.rel_type.to_string(),
                    })
                })
                .collect();
            print_json_ok(data);
        }
        OutputMode::Pretty => {
            if rels.is_empty() {
                println!("No relationships for {}", id);
            } else {
                for r in &rels {
                    println!("{}", r);
                }
            }
        }
    }

    Ok(())
}

pub fn parse_rel_type(s: &str) -> Result<RelationType> {
    match s.to_lowercase().replace(['-', ' '], "_").as_str() {
        "blocks" => Ok(RelationType::Blocks),
        "blocked_by" | "blockedby" => Ok(RelationType::BlockedBy),
        "parent" => Ok(RelationType::Parent),
        "child" => Ok(RelationType::Child),
        "relates_to" | "relatesto" | "relates" => Ok(RelationType::RelatesTo),
        "duplicates" => Ok(RelationType::Duplicates),
        "duplicated_by" | "duplicatedby" => Ok(RelationType::DuplicatedBy),
        _ => Err(anyhow::anyhow!(
            "Invalid relationship type '{}'. Use: blocks, blocked_by, parent, child, relates_to, duplicates, duplicated_by",
            s
        )),
    }
}

fn validate_item_exists(state: &ProjectState, id: &ItemId) -> Result<()> {
    if state.tasks.contains_key(id) || state.epics.contains_key(id) {
        Ok(())
    } else {
        Err(anyhow::anyhow!("Item not found: {}", id))
    }
}
