//! `airboss-launch`: Do it and Plan it.
//!
//! The only module that runs the `t3` CLI. It commits a `pending` row to its own table before
//! every `t3` call, so a retry, a timeout or a restart never starts a second thread for the
//! same task, repo and mode. See `SPEC-launch.md` at the repo root, and the decisions in
//! `tasks/launch-plan.md`.

mod db;
mod process;

pub use db::MIGRATIONS;
pub use process::{CmdOutput, ProcessRunner, REMOVED_VARS, child_env};
