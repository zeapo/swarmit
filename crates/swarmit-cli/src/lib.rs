pub mod commands;
pub mod output;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use commands::{
    comment::CommentArgs, compact::CompactArgs, epic::EpicArgs, init::InitArgs,
    link::LinkArgs, log::LogArgs, project::ProjectArgs, sync::SyncArgs, task::TaskArgs,
};

#[derive(Parser, Debug)]
#[command(
    name = "swarmit",
    about = "Local-first project management for multi-agent workflows",
    version
)]
pub struct Cli {
    /// Agent identifier (or set SWARMIT_AGENT env var)
    #[arg(long, global = true, env = "SWARMIT_AGENT")]
    pub agent: Option<String>,

    /// Force JSON output
    #[arg(long, global = true)]
    pub json: bool,

    /// Force plain text output
    #[arg(long, global = true)]
    pub plain: bool,

    /// Project root directory (default: walk up to find .swarmit/)
    #[arg(long, global = true)]
    pub dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize a new swarmit project
    Init(InitArgs),
    /// Manage epics
    Epic(EpicArgs),
    /// Manage tasks
    Task(TaskArgs),
    /// Manage links between items
    Link(LinkArgs),
    /// Manage comments
    Comment(CommentArgs),
    /// View operation log
    Log(LogArgs),
    /// Compact the operation log
    Compact(CompactArgs),
    /// Manage project settings
    Project(ProjectArgs),
    /// Materialize all markdown files from the current state
    Sync(SyncArgs),
}

pub fn run(cli: Cli) -> Result<()> {
    match &cli.command {
        Some(Commands::Init(args)) => commands::init::run(args, &cli),
        Some(Commands::Epic(args)) => commands::epic::run(args, &cli),
        Some(Commands::Task(args)) => commands::task::run(args, &cli),
        Some(Commands::Link(args)) => commands::link::run(args, &cli),
        Some(Commands::Comment(args)) => commands::comment::run(args, &cli),
        Some(Commands::Log(args)) => commands::log::run(args, &cli),
        Some(Commands::Compact(args)) => commands::compact::run(args, &cli),
        Some(Commands::Project(args)) => commands::project::run(args, &cli),
        Some(Commands::Sync(args)) => commands::sync::run(args, &cli),
        None => {
            // No subcommand — caller should have launched TUI or printed help
            Err(anyhow::anyhow!(
                "No command specified. Run `swarmit --help` for usage."
            ))
        }
    }
}
