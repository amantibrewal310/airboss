//! The `airboss` binary: `airboss mcp` serves agents, everything else is the CLI.

use std::process::ExitCode;

use clap::{Parser, Subcommand};

use airboss_mcp::identity::Identity;

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

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Mcp => mcp(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("airboss: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Works out who this thread is, once, then serves it. Only MCP messages go to stdout.
fn mcp() -> Result<(), String> {
    let who = Identity::current();
    let store = airboss_mcp::open_store()?;
    airboss_mcp::server::serve(store, who)
}
