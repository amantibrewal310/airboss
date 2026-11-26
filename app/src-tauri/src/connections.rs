//! The connections footer: every service's rows, T3's availability and the app's own, merged.

use airboss_services::{Availability, Connection};

use std::path::Path;

use crate::dto::{CliStatus, ConnectionsChanged};
use crate::services::Services;
use crate::state::Problems;
use crate::viewmodel::ViewModel;

/// The footer's order: T3 Code, GitHub, Agents, Claude Code, then anything else.
const ORDER: [&str; 4] = ["T3 Code", "GitHub", "Agents", "Claude Code"];

fn row(name: &str, ok: bool, state: &str, detail: &str, fix: Option<&str>) -> Connection {
    Connection {
        name: name.into(),
        ok,
        state: state.into(),
        detail: detail.into(),
        fix: fix.map(Into::into),
    }
}

/// Rows with the same name become one (EM decision): ok only if all are, with `state`,
/// `detail` and `fix` from the first that isn't.
pub fn merge(rows: Vec<Connection>) -> Vec<Connection> {
    let mut out: Vec<Connection> = Vec::new();
    for r in rows {
        match out.iter_mut().find(|x| x.name == r.name) {
            Some(existing) if existing.ok && !r.ok => *existing = r,
            Some(_) => {}
            None => out.push(r),
        }
    }
    out.sort_by_key(|r| {
        ORDER
            .iter()
            .position(|n| *n == r.name)
            .unwrap_or(ORDER.len())
    });
    out
}

/// What `~/.local/bin/airboss` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cli {
    /// Nothing there.
    Missing,
    /// There, but it (or what it links to) is gone, empty or can't run.
    Broken,
    /// It runs: a non-empty, executable file, through the link.
    Ready,
}

/// Follows the link and checks the file it ends at, so a dangling link to an Airboss.app
/// that moved isn't "Ready" (final audit should-fix 9).
pub fn cli_at(link: &std::path::Path) -> Cli {
    use std::os::unix::fs::PermissionsExt;
    if std::fs::symlink_metadata(link).is_err() {
        return Cli::Missing;
    }
    match std::fs::metadata(link) {
        Ok(m) if m.is_file() && m.len() > 0 && m.permissions().mode() & 0o111 != 0 => Cli::Ready,
        _ => Cli::Broken,
    }
}

/// Every row the footer shows, and T3's availability. `agent_report` is the time of the
/// newest `agent:*` event, which the UI shows as "Last report 3m ago".
pub(crate) fn merged(
    services: &Services,
    _vm: &ViewModel,
    problems: &Problems,
    cli_status: CliStatus,
    agent_report: Option<&str>,
) -> ConnectionsChanged {
    let cli = cli_at(Path::new(&cli_status.path));
    let availability = services.launch.availability();
    let mut rows: Vec<Connection> = Vec::new();
    for s in services.all() {
        rows.extend(s.connections());
    }
    // T3 missing turns the T3 Code row orange whoever reported it (the worst state wins).
    // Ready adds a row only when no service reported one (the fakes, a stand-in).
    let reported = rows.iter().any(|r| r.name == "T3 Code");
    match &availability {
        Availability::Missing { reason } => rows.push(row(
            "T3 Code",
            false,
            "Not found",
            reason,
            Some("Install T3 Code, then choose Check again."),
        )),
        Availability::Ready { .. } if !reported => rows.push(row(
            "T3 Code",
            true,
            "Found",
            "Starts threads for Do it and Plan it.",
            None,
        )),
        _ => {}
    }
    let detail = "Agents report progress and ask questions through the airboss command.";
    rows.push(match (cli, agent_report) {
        (Cli::Ready, Some(_)) => row("Agents", true, "Ready", detail, None),
        (Cli::Ready, None) => row("Agents", true, "Ready", "No agent has reported yet.", None),
        (Cli::Broken, _) => row(
            "Agents",
            false,
            "Link broken",
            "~/.local/bin/airboss points at a file that's gone or can't run.",
            Some("Install the command-line tool again in Settings, then copy the agent note."),
        ),
        (Cli::Missing, _) => row(
            "Agents",
            false,
            "Not set up",
            detail,
            Some("Install the command-line tool, then copy the agent note."),
        ),
    });
    if problems.db_moved {
        rows.push(row(
            "Database",
            false,
            "Database moved",
            "The task database was moved or replaced while Airboss was open.",
            Some("Quit Airboss and open it again."),
        ));
    }
    if problems.live_stopped {
        rows.push(row(
            "Live updates",
            false,
            "Stopped",
            "Live updates stopped after an internal error.",
            Some("Restart Airboss."),
        ));
    }
    if let Some(why) = &problems.watcher {
        rows.push(row(
            "Live updates",
            false,
            "Not watching",
            why,
            Some("Changes from agents show when the window is focused. Restart Airboss."),
        ));
    }
    if let Some(key) = &problems.hotkey_taken {
        rows.push(row(
            "Quick add",
            false,
            "Shortcut taken",
            &format!("{key} opens quick add from any app."),
            Some(&format!(
                "Quick add shortcut {key} is taken by another app. Change it in Settings."
            )),
        ));
    }
    let mut merged = merge(rows);
    // Claude Code shows only when it's broken.
    merged.retain(|r| r.name != "Claude Code" || !r.ok);
    ConnectionsChanged {
        connections: merged,
        availability,
        agent_report: agent_report.map(str::to_string),
        cli: cli_status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_with_the_same_name_merge_and_the_worst_state_wins() {
        let rows = merge(vec![
            row("GitHub", true, "Signed in", "", None),
            row("T3 Code", true, "Found", "", None),
            row("T3 Code", false, "Not running", "", None),
            row("T3 Code", false, "Second problem", "", None),
        ]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "T3 Code");
        assert!(!rows[0].ok);
        assert_eq!(rows[0].state, "Not running");
        assert_eq!(rows[1].name, "GitHub");
    }
}
