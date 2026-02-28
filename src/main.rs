use std::io::IsTerminal;

use clap::Parser;
use swarmit::cli::{run, Cli};

fn main() {
    let cli = Cli::parse();

    // If no subcommand and stdout is a TTY, launch TUI
    if cli.command.is_none() && std::io::stdout().is_terminal() {
        let root = swarmit::cli::commands::init::resolve_project_root(&cli)
            .unwrap_or_else(|_| std::env::current_dir().unwrap());

        if let Err(e) = swarmit::tui::run(&root) {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    if let Err(e) = run(cli) {
        let is_piped = !std::io::stdout().is_terminal();
        if is_piped {
            // JSON error envelope for piped consumers
            println!(
                "{{\"ok\":false,\"error\":\"{}\"}}",
                e.to_string().replace('"', "\\\"")
            );
        } else {
            eprintln!("Error: {}", e);
        }
        std::process::exit(1);
    }
}
