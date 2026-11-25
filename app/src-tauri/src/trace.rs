//! A debug trace of what the app sends its web views and menu-bar item, for checking the app
//! without a screen (integration QA, support).
//!
//! Off by default. `AIRBOSS_TRACE=1` at start-up turns it on: each event sent to a web view, each
//! menu-bar redraw and each catch-up batch is logged as one `trace …` line ending in
//! `epoch_ms=…`, so a script can time a write from another process to the patch that showed it.
//! It logs ids, statuses and counts, never task titles or notes.

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::sync::OnceLock;
use std::time::Duration;

use airboss_core::TaskId;
use tauri_plugin_log::log;

use crate::dto::Patch;
use crate::shell::UiEvent;
use crate::tray::TrayModel;

/// The variable that turns the trace on.
pub const VAR: &str = "AIRBOSS_TRACE";

/// The most ids one line lists before it says how many more there are.
const MAX_IDS: usize = 20;

/// Whether a value of [`VAR`] turns the trace on: set, not empty and not `0`.
pub fn on(value: Option<&OsStr>) -> bool {
    value.is_some_and(|v| !v.is_empty() && v != "0")
}

/// Whether the trace is on for this process (read once).
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| on(std::env::var_os(VAR).as_deref()))
}

fn emit_line(line: &str) {
    log::info!(
        "trace {line}; epoch_ms={}",
        crate::commands::app::epoch_ms()
    );
}

/// Logs an event on its way to a web view.
pub fn event(e: &UiEvent) {
    if enabled() {
        emit_line(&describe(e));
    }
}

/// Logs a menu-bar redraw.
pub fn tray(model: &TrayModel) {
    if enabled() {
        emit_line(&describe_tray(model));
    }
}

/// Logs one catch-up batch: what triggered it, how long it took and the `seq` after it.
pub fn batch(causes: &str, took: Duration, seq: u64) {
    if enabled() {
        emit_line(&format!(
            "catch_up causes={causes} took_us={} seq={seq}",
            took.as_micros()
        ));
    }
}

fn ids(list: impl IntoIterator<Item = String>) -> String {
    let all: Vec<String> = list.into_iter().collect();
    let mut out = all
        .iter()
        .take(MAX_IDS)
        .cloned()
        .collect::<Vec<_>>()
        .join(",");
    if all.len() > MAX_IDS {
        let _ = write!(out, ",…+{}", all.len() - MAX_IDS);
    }
    format!("[{out}]")
}

fn task_ids(list: &[TaskId]) -> String {
    ids(list.iter().map(|t| t.0.to_string()))
}

fn describe_patch(p: &Patch) -> String {
    format!(
        "tree:patch seq={} upserts={} removed={} created={} order={} repos={}",
        p.seq,
        ids(p.upserts.iter().map(|r| {
            let status = serde_json::to_value(r.status)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
            let mut s = format!("{}:{status}", r.id.0);
            if r.waiting {
                s.push_str(":waiting");
            }
            if r.needs {
                s.push_str(":needs");
            }
            s
        })),
        task_ids(&p.removed),
        task_ids(&p.created),
        p.order
            .as_ref()
            .map_or("same".into(), |o| o.len().to_string()),
        p.repos
            .as_ref()
            .map_or("same".into(), |r| r.len().to_string()),
    )
}

/// One line for an event: ids, statuses and counts only.
pub fn describe(e: &UiEvent) -> String {
    match e {
        UiEvent::TreePatch(p) => describe_patch(p),
        UiEvent::NeedsChanged(n) => format!(
            "needs:changed count={} version={} items={}",
            n.count,
            n.version,
            ids(n.items.iter().map(|i| format!("{}:{}", i.key.0, i.task.0)))
        ),
        UiEvent::ConnectionsChanged(c) => format!(
            "connections:changed availability={:?} rows={}",
            c.availability,
            ids(c.connections.iter().map(|r| format!(
                "{}={}({})",
                r.name,
                if r.ok { "ok" } else { "off" },
                r.state
            )))
        ),
        UiEvent::ErrorBackground(b) => format!("error:background source={}", b.source),
        other => other.name().to_string(),
    }
}

/// One line for a menu-bar redraw: the count it shows and its menu's item keys.
pub fn describe_tray(m: &TrayModel) -> String {
    format!(
        "tray title={} items={}",
        m.title.as_deref().unwrap_or("none"),
        ids(m.items.iter().map(|i| i.key.clone()))
    )
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use airboss_services::{
        Availability, Connection, NeedsGroup, NeedsItem, NeedsKey, NeedsKind, NeedsList, Target,
    };

    use super::*;
    use crate::dto::ConnectionsChanged;

    #[test]
    fn the_trace_is_off_unless_the_variable_says_otherwise() {
        assert!(!on(None));
        assert!(!on(Some(&OsString::from(""))));
        assert!(!on(Some(&OsString::from("0"))));
        assert!(on(Some(&OsString::from("1"))));
        assert!(on(Some(&OsString::from("yes"))));
    }

    #[test]
    fn a_patch_line_lists_ids_and_statuses_but_no_titles() {
        let p = Patch {
            seq: 7,
            removed: vec![TaskId(3)],
            created: vec![TaskId(9)],
            order: Some(vec![TaskId(1), TaskId(9)]),
            ..Patch::default()
        };
        assert_eq!(
            describe(&UiEvent::TreePatch(p)),
            "tree:patch seq=7 upserts=[] removed=[3] created=[9] order=2 repos=same"
        );
    }

    #[test]
    fn long_id_lists_are_cut_with_a_count() {
        let many: Vec<TaskId> = (1..=25).map(TaskId).collect();
        let line = task_ids(&many);
        assert!(line.ends_with(",20,…+5]"), "{line}");
    }

    #[test]
    fn needs_and_connections_lines_carry_counts_and_states() {
        let needs = NeedsList {
            items: vec![NeedsItem {
                key: NeedsKey("proposed:4".into()),
                kind: NeedsKind::Proposed,
                group: NeedsGroup::Proposed,
                task: TaskId(4),
                task_title: "secret title".into(),
                since: "2026-11-26T10:00:00Z".into(),
                headline: "h".into(),
                context: "c".into(),
                target: Target::Task(TaskId(4)),
                proposed: Vec::new(),
                stale: false,
            }],
            count: 1,
            checked_at: None,
            version: 3,
        };
        let line = describe(&UiEvent::NeedsChanged(needs));
        assert_eq!(line, "needs:changed count=1 version=3 items=[proposed:4:4]");
        assert!(!line.contains("secret"));
        let c = ConnectionsChanged {
            connections: vec![Connection {
                name: "GitHub".into(),
                ok: true,
                state: "Signed in".into(),
                detail: String::new(),
                fix: None,
            }],
            availability: Availability::Unknown,
        };
        assert_eq!(
            describe(&UiEvent::ConnectionsChanged(c)),
            "connections:changed availability=Unknown rows=[GitHub=ok(Signed in)]"
        );
    }

    #[test]
    fn a_tray_line_shows_the_count_it_draws() {
        let m = TrayModel {
            title: Some("2".into()),
            tooltip: String::new(),
            header: String::new(),
            items: Vec::new(),
        };
        assert_eq!(describe_tray(&m), "tray title=2 items=[]");
    }
}
