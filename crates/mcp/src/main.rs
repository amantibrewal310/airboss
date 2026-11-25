//! The `airboss` binary: `airboss mcp` serves agents, everything else is the CLI.

use clap::{Parser, Subcommand};

/// A to-do list where agents do the work.
#[derive(Parser)]
#[command(name = "airboss", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the MCP tools over stdin and stdout, for one agent thread.
    Mcp,
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Mcp => {
            eprintln!("airboss mcp: not built yet");
            std::process::ExitCode::FAILURE
        }
    }
}
