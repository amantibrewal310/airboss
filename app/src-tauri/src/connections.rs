//! The connections footer: every service's rows, T3's availability and the app's own, merged.

use airboss_services::Availability;

use crate::dto::{AvailabilityDto, ConnectionDto, ConnectionsDto};
use crate::services::Services;
use crate::state::Problems;
use crate::viewmodel::{ViewModel, last_agent_report};

/// The footer's order: T3 Code, GitHub, Agents, Claude Code, then anything else.
const ORDER: [&str; 4] = ["T3 Code", "GitHub", "Agents", "Claude Code"];

/// Rows with the same name become one (EM decision): ok only if all are, with `state`,
/// `detail` and `fix` from the first that isn't. First-seen order is kept within a name.
pub fn merge(rows: Vec<ConnectionDto>) -> Vec<ConnectionDto> {
    let mut out: Vec<ConnectionDto> = Vec::new();
    for row in rows {
        match out.iter_mut().find(|r| r.name == row.name) {
            Some(existing) => {
                if existing.ok && !row.ok {
                    let last_at = existing.last_at.take().or(row.last_at.clone());
                    *existing = ConnectionDto { last_at, ..row };
                }
            }
            None => out.push(row),
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

/// Every row the footer shows, and T3's availability.
pub(crate) fn merged(
    services: &Services,
    vm: &ViewModel,
    problems: &Problems,
    cli_installed: bool,
) -> ConnectionsDto {
    let availability = services.launch.availability();
    let mut rows: Vec<ConnectionDto> = Vec::new();
    for s in services.all() {
        rows.extend(s.connections().iter().map(ConnectionDto::from));
    }
    if let Availability::Missing { reason } = &availability {
        rows.push(ConnectionDto {
            name: "T3 Code".into(),
            ok: false,
            state: "Not found".into(),
            detail: reason.clone(),
            fix: Some("Install T3 Code, then choose Check again.".into()),
            last_at: None,
        });
    } else if let Availability::Ready { .. } = &availability {
        rows.push(ConnectionDto {
            name: "T3 Code".into(),
            ok: true,
            state: "Found".into(),
            detail: "Starts threads for Do it and Plan it.".into(),
            fix: None,
            last_at: None,
        });
    }
    let last = last_agent_report(vm);
    rows.push(if cli_installed {
        ConnectionDto {
            name: "Agents".into(),
            ok: true,
            state: "Ready".into(),
            detail: "Agents report progress through the airboss command.".into(),
            fix: None,
            last_at: last,
        }
    } else {
        ConnectionDto {
            name: "Agents".into(),
            ok: false,
            state: "Not set up".into(),
            detail: "Agents report progress through the airboss command.".into(),
            fix: Some("Install the command-line tool, then copy the agent note.".into()),
            last_at: last,
        }
    });
    if problems.db_moved {
        rows.push(ConnectionDto {
            name: "Database".into(),
            ok: false,
            state: "Database moved".into(),
            detail: "The task database was moved or replaced while Airboss was open.".into(),
            fix: Some("Quit Airboss and open it again.".into()),
            last_at: None,
        });
    }
    if problems.live_stopped {
        rows.push(ConnectionDto {
            name: "Live updates".into(),
            ok: false,
            state: "Stopped".into(),
            detail: "Live updates stopped after an internal error.".into(),
            fix: Some("Restart Airboss.".into()),
            last_at: None,
        });
    }
    if let Some(why) = &problems.watcher {
        rows.push(ConnectionDto {
            name: "Live updates".into(),
            ok: false,
            state: "Not watching".into(),
            detail: why.clone(),
            fix: Some(
                "Changes from agents show when the window is focused. Restart Airboss.".into(),
            ),
            last_at: None,
        });
    }
    let mut merged = merge(rows);
    // Claude Code shows only when it's broken.
    merged.retain(|r| r.name != "Claude Code" || !r.ok);
    ConnectionsDto {
        connections: merged,
        availability: AvailabilityDto::from(&availability),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, ok: bool, state: &str) -> ConnectionDto {
        ConnectionDto {
            name: name.into(),
            ok,
            state: state.into(),
            detail: String::new(),
            fix: None,
            last_at: None,
        }
    }

    #[test]
    fn rows_with_the_same_name_merge_and_the_worst_state_wins() {
        let rows = merge(vec![
            row("GitHub", true, "Signed in"),
            row("T3 Code", true, "Found"),
            row("T3 Code", false, "Not running"),
            row("T3 Code", false, "Second problem"),
        ]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "T3 Code");
        assert!(!rows[0].ok);
        assert_eq!(rows[0].state, "Not running");
        assert_eq!(rows[1].name, "GitHub");
    }
}
