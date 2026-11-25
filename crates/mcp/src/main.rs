//! The `airboss` binary: `airboss mcp` serves agents, everything else is the CLI.

use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use airboss_mcp::cli::{self, Style};
use airboss_mcp::identity::Identity;

/// A to-do list where agents do the work.
///
/// These commands act as you. Agents reach the same tasks through `airboss mcp`.
#[derive(Parser)]
#[command(name = "airboss", version, arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// `--json`, on every command except `mcp`.
#[derive(Args, Clone, Copy)]
struct Out {
    /// Print JSON, the same shapes agents get, instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Show the task tree. Done and dropped tasks are hidden unless --all.
    Ls {
        /// Include done and dropped tasks.
        #[arg(long)]
        all: bool,
        /// Only tasks that can start now: todo, with nothing open to wait on.
        #[arg(long, conflicts_with = "all")]
        ready: bool,
        #[command(flatten)]
        out: Out,
    },
    /// Show one task: repos, branch, links, dependencies, run order and history.
    Show {
        /// The task's id, e.g. 12.
        id: i64,
        #[command(flatten)]
        out: Out,
    },
    /// Show recent history, newest last.
    Log {
        /// Show every event after this event id instead of the last 50.
        #[arg(long, value_name = "EVENT")]
        after: Option<i64>,
        #[command(flatten)]
        out: Out,
    },
    /// Register and list repos, and link them to tasks.
    #[command(subcommand)]
    Repo(RepoCommand),
    /// Serve the MCP tools over stdin and stdout, for one agent thread.
    ///
    /// Agent tools start this themselves, one process per thread; see the README.
    Mcp,
}

#[derive(Subcommand)]
enum RepoCommand {
    /// List the registered repos.
    Ls {
        #[command(flatten)]
        out: Out,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Mcp => mcp(),
        other => run(other).and_then(|text| cli::print(&text)),
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

/// Runs one CLI command as `me` and returns what to print.
fn run(command: Command) -> Result<String, String> {
    let store = airboss_mcp::open_store()?;
    let style = Style::detect();
    let out = match command {
        Command::Ls { all, ready, out } => cli::ls(&store, all, ready, out.json, style),
        Command::Show { id, out } => cli::show(&store, id, out.json, style),
        Command::Log { after, out } => cli::log(&store, after, out.json, style),
        Command::Repo(RepoCommand::Ls { out }) => cli::repo_ls(&store, out.json, style),
        Command::Mcp => return Err("airboss mcp is handled before this".into()),
    };
    out.map_err(|e| e.to_string())
}
