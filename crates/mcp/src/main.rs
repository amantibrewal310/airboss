//! The `airboss` binary: `airboss mcp` serves agents, everything else is the CLI.

use std::process::ExitCode;

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use airboss_core::{LinkKind, NewLink, Status};
use airboss_mcp::cli::{self, SetStatus, Target};
use airboss_mcp::identity::Identity;
use airboss_mcp::render::Style;
use airboss_mcp::tools::LinkKindName;

/// A to-do list where agents do the work.
///
/// These commands act as you. Agents reach the same tasks through `airboss mcp`.
#[derive(Parser)]
#[command(name = "airboss", version, arg_required_else_help = true, after_help = AFTER_HELP)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

const AFTER_HELP: &str = "\
Examples:
  airboss add \"Fix booking typo\" --repo hub-ui
  airboss ls
  airboss set 12 doing
  airboss accept 14

Tasks live in ~/Library/Application Support/Airboss/airboss.db.
Set AIRBOSS_DB to use another file.";

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
    /// Add a task, as todo, at the end of its siblings.
    Add {
        /// What to do, e.g. "Fix booking typo".
        title: String,
        /// Put it under this task.
        #[arg(long, value_name = "TASK")]
        under: Option<i64>,
        /// Link it to a repo, by name. Give it again for more.
        #[arg(long = "repo", value_name = "NAME")]
        repos: Vec<String>,
        /// Notes.
        #[arg(long)]
        body: Option<String>,
        #[command(flatten)]
        out: Out,
    },
    /// Set a task's status. For done and dropped, use `done` and `drop`.
    Set {
        /// The task's id.
        id: i64,
        status: SetStatus,
        /// Why it's blocked, or a note on the change.
        #[arg(long)]
        note: Option<String>,
        #[command(flatten)]
        out: Out,
    },
    /// Mark a task done.
    Done {
        /// The task's id.
        id: i64,
        /// What was done.
        #[arg(long)]
        note: Option<String>,
        #[command(flatten)]
        out: Out,
    },
    /// Drop a task, and every open task under it.
    Drop {
        /// The task's id.
        id: i64,
        /// Why.
        #[arg(long)]
        note: Option<String>,
        #[command(flatten)]
        out: Out,
    },
    /// Accept an agent's proposal; it becomes todo.
    Accept {
        /// The proposed task's id.
        id: i64,
        #[command(flatten)]
        out: Out,
    },
    /// Reject an agent's proposal; it's dropped, hidden from the tree, kept in history.
    Reject {
        /// The proposed task's id.
        id: i64,
        /// Why, for the agent and for history.
        #[arg(long)]
        note: Option<String>,
        #[command(flatten)]
        out: Out,
    },
    /// Change a task's title or notes.
    Edit {
        /// The task's id.
        id: i64,
        /// The new title.
        #[arg(long)]
        title: Option<String>,
        /// The new notes; an empty string clears them.
        #[arg(long)]
        body: Option<String>,
        #[command(flatten)]
        out: Out,
    },
    /// Move a task, with everything under it.
    Mv {
        /// The task's id.
        id: i64,
        /// Move it under this task.
        #[arg(long, value_name = "TASK", conflicts_with = "top")]
        under: Option<i64>,
        /// Move it to the top level.
        #[arg(long)]
        top: bool,
        /// Its position among its new siblings, from 1. Left out: the end.
        #[arg(long, value_name = "N", required_unless_present_any = ["under", "top"])]
        pos: Option<u32>,
        #[command(flatten)]
        out: Out,
    },
    /// Make a task wait on another.
    Wait {
        /// The task that waits.
        id: i64,
        /// The task it waits on.
        #[arg(long, value_name = "TASK")]
        on: i64,
        #[command(flatten)]
        out: Out,
    },
    /// Stop a task waiting on another.
    Unwait {
        /// The task that waits.
        id: i64,
        /// The task it waits on.
        #[arg(long, value_name = "TASK")]
        on: i64,
        #[command(flatten)]
        out: Out,
    },
    /// Link something to a task: a PR, a doc, a thread, a Slack message or an issue.
    Link {
        /// The task's id.
        id: i64,
        kind: LinkKindName,
        /// owner/repo#42, a file path, a thread id or a permalink.
        #[arg(value_name = "REF")]
        reference: String,
        /// The web page it lives at.
        #[arg(long)]
        url: Option<String>,
        /// A short name to show instead of the ref.
        #[arg(long)]
        title: Option<String>,
        #[command(flatten)]
        out: Out,
    },
    /// Set the branch every thread for a task shares.
    Branch {
        /// The task's id.
        id: i64,
        /// The branch name.
        #[arg(required_unless_present = "clear", conflicts_with = "clear")]
        name: Option<String>,
        /// Clear the branch instead.
        #[arg(long)]
        clear: bool,
        #[command(flatten)]
        out: Out,
    },
    /// Register, edit, remove and list repos, and link them to tasks.
    #[command(subcommand)]
    Repo(RepoCommand),
    /// Show recent history, newest last.
    Log {
        /// Show every event after this event id instead of the last 50.
        #[arg(long, value_name = "EVENT")]
        after: Option<i64>,
        #[command(flatten)]
        out: Out,
    },
    /// Serve the MCP tools over stdin and stdout, for one agent thread.
    ///
    /// Agent tools start this themselves, one process per thread; see the README.
    Mcp,
}

#[derive(Subcommand)]
enum RepoCommand {
    /// Register a repo tasks can be linked to.
    Add {
        /// A short name, e.g. hub-ui.
        name: String,
        /// Where it's checked out.
        path: PathBuf,
        /// Its remote URL.
        #[arg(long)]
        remote: Option<String>,
        #[command(flatten)]
        out: Out,
    },
    /// Rename a repo, point it at another folder, or change its remote.
    Edit {
        /// The repo's name now.
        name: String,
        /// Its new name.
        #[arg(long = "name", value_name = "NEW")]
        new_name: Option<String>,
        /// Where it's checked out now.
        #[arg(long)]
        path: Option<PathBuf>,
        /// Its new remote URL.
        #[arg(long, conflicts_with = "no_remote")]
        remote: Option<String>,
        /// Clear its remote URL.
        #[arg(long)]
        no_remote: bool,
        #[command(flatten)]
        out: Out,
    },
    /// Remove a repo. It's unlinked from every task that has it first.
    Rm {
        /// The repo's name.
        name: String,
        #[command(flatten)]
        out: Out,
    },
    /// Link a task to a repo.
    Link {
        /// The task's id.
        task: i64,
        /// The repo's name.
        name: String,
        #[command(flatten)]
        out: Out,
    },
    /// Unlink a task from a repo.
    Unlink {
        /// The task's id.
        task: i64,
        /// The repo's name.
        name: String,
        #[command(flatten)]
        out: Out,
    },
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
    let mut store = airboss_mcp::open_store()?;
    let store = &mut store;
    let style = Style::detect();
    let out = match command {
        Command::Add {
            title,
            under,
            repos,
            body,
            out,
        } => {
            let args = cli::Add {
                title: &title,
                under,
                repos: &repos,
                body: body.as_deref(),
            };
            cli::add(store, args, out.json, style)
        }
        Command::Ls { all, ready, out } => cli::ls(store, all, ready, out.json, style),
        Command::Show { id, out } => cli::show(store, id, out.json, style),
        Command::Set {
            id,
            status,
            note,
            out,
        } => cli::set(store, id, status.into(), note.as_deref(), out.json, style),
        Command::Done { id, note, out } => {
            cli::set(store, id, Status::Done, note.as_deref(), out.json, style)
        }
        Command::Drop { id, note, out } => {
            cli::set(store, id, Status::Dropped, note.as_deref(), out.json, style)
        }
        Command::Accept { id, out } => cli::decide(store, id, true, None, out.json, style),
        Command::Reject { id, note, out } => {
            cli::decide(store, id, false, note.as_deref(), out.json, style)
        }
        Command::Edit {
            id,
            title,
            body,
            out,
        } => cli::edit(
            store,
            id,
            title.as_deref(),
            body.as_deref(),
            out.json,
            style,
        ),
        Command::Mv {
            id,
            under,
            top,
            pos,
            out,
        } => {
            let target = match (under, top) {
                (Some(p), _) => Target::Under(p),
                (None, true) => Target::Top,
                (None, false) => Target::Same,
            };
            cli::mv(store, id, target, pos, out.json, style)
        }
        Command::Wait { id, on, out } => cli::wait(store, id, on, true, out.json, style),
        Command::Unwait { id, on, out } => cli::wait(store, id, on, false, out.json, style),
        Command::Link {
            id,
            kind,
            reference,
            url,
            title,
            out,
        } => {
            let mut new = NewLink::new(LinkKind::from(kind), reference.trim());
            new.url = url;
            new.title = title;
            cli::link(store, id, new, out.json, style)
        }
        Command::Branch {
            id,
            name,
            clear,
            out,
        } => {
            let name = if clear { None } else { name.as_deref() };
            cli::branch(store, id, name, out.json, style)
        }
        Command::Log { after, out } => cli::log(store, after, out.json, style),
        Command::Repo(repo) => match repo {
            RepoCommand::Add {
                name,
                path,
                remote,
                out,
            } => cli::repo_add(store, &name, &path, remote.as_deref(), out.json),
            RepoCommand::Edit {
                name,
                new_name,
                path,
                remote,
                no_remote,
                out,
            } => {
                let change = cli::RepoChange {
                    name: new_name.as_deref(),
                    path: path.as_deref(),
                    remote: match (remote.as_deref(), no_remote) {
                        (Some(url), _) => Some(Some(url)),
                        (None, true) => Some(None),
                        (None, false) => None,
                    },
                };
                cli::repo_edit(store, &name, change, out.json)
            }
            RepoCommand::Rm { name, out } => cli::repo_rm(store, &name, out.json),
            RepoCommand::Link { task, name, out } => {
                cli::repo_link(store, task, &name, true, out.json, style)
            }
            RepoCommand::Unlink { task, name, out } => {
                cli::repo_link(store, task, &name, false, out.json, style)
            }
            RepoCommand::Ls { out } => cli::repo_ls(store, out.json, style),
        },
        Command::Mcp => return Err("airboss mcp is handled before this".into()),
    };
    out.map_err(|e| e.to_string())
}
