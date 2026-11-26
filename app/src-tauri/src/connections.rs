//! The connections footer: every service's rows, T3's availability and the app's own, merged.

use airboss_services::{Availability, Connection};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde::Deserialize;
use tauri_plugin_log::log;

use crate::dto::{AgentTools, CliStatus, ConnectionsChanged, LauncherKind};
use crate::services::Services;
use crate::settings::{Settings, StartAgentsIn};
use crate::state::Problems;
use crate::viewmodel::ViewModel;

/// The Agents row's detail when Codex runs in Herdr with a setup that lacks `HC_TERMINAL_ID`.
pub const CODEX_NEEDS_TERMINAL_ID: &str = "Codex setup needs HC_TERMINAL_ID";

/// The footer's order: T3 Code, Herdr, GitHub, Agents, Claude Code, then anything else.
const ORDER: [&str; 5] = ["T3 Code", "Herdr", "GitHub", "Agents", "Claude Code"];

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

/// Claude Code's user config: its top-level `mcpServers` is where `claude mcp add --scope user`
/// writes. Everything else in the file (often megabytes of history) is skipped unparsed.
#[derive(Deserialize)]
struct ClaudeJson {
    #[serde(rename = "mcpServers", default)]
    mcp_servers: Option<HashMap<String, serde::de::IgnoredAny>>,
}

/// Whether `~/.claude.json`'s text registers `airboss`; `Err` when it isn't valid JSON or
/// `mcpServers` isn't an object.
pub fn claude_has_airboss(text: &str) -> Result<bool, String> {
    let c: ClaudeJson = serde_json::from_str(text).map_err(|e| e.to_string())?;
    Ok(c.mcp_servers.is_some_and(|m| m.contains_key("airboss")))
}

/// Whether `~/.codex/config.toml`'s text has an `airboss` table under `mcp_servers` (written
/// as `[mcp_servers.airboss]`, an inline table, or dotted keys); `Err` when it isn't TOML.
pub fn codex_has_airboss(text: &str) -> Result<bool, String> {
    let t: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    Ok(t.get("mcp_servers")
        .and_then(|m| m.get("airboss"))
        .is_some_and(toml::Value::is_table))
}

/// Whether `~/.codex/config.toml`'s `airboss` table passes `HC_TERMINAL_ID` in `env_vars`;
/// `Err` when it isn't TOML.
pub fn codex_passes_terminal_id(text: &str) -> Result<bool, String> {
    let t: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    Ok(t.get("mcp_servers")
        .and_then(|m| m.get("airboss"))
        .and_then(|a| a.get("env_vars"))
        .and_then(toml::Value::as_array)
        .is_some_and(|vars| vars.iter().any(|v| v.as_str() == Some("HC_TERMINAL_ID"))))
}

/// A file as last read: its modification time and size, and what it said.
struct Seen {
    stamp: Option<(SystemTime, u64)>,
    found: bool,
}

/// What each agent config file said, by path and question. Connections are recomputed on every catch-up,
/// so a file is read again only when its time or size changes; a problem is logged once each
/// time the file changes, not on every pass.
static SEEN: Mutex<Option<HashMap<(PathBuf, &'static str), Seen>>> = Mutex::new(None);

/// Whether the agent config at `path` says yes to `what` (e.g. "airboss"), by `parse`.
/// Missing, unreadable or malformed is "no": never an error, never a crash.
fn registered(path: &Path, what: &'static str, parse: fn(&str) -> Result<bool, String>) -> bool {
    let meta = std::fs::metadata(path);
    let stamp = meta
        .as_ref()
        .ok()
        .map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()));
    let mut seen = crate::state::lock(&SEEN);
    let seen = seen.get_or_insert_with(HashMap::new);
    let key = (path.to_path_buf(), what);
    if let Some(s) = seen.get(&key)
        && s.stamp == stamp
    {
        return s.found;
    }
    let found = match meta {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            log::info!("{} isn't there: not connected", path.display());
            false
        }
        Err(e) => {
            log::warn!("can't read {}: {e}; not connected", path.display());
            false
        }
        Ok(_) => match std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|t| parse(&t))
        {
            Ok(found) => {
                log::info!(
                    "{} {} {what}",
                    path.display(),
                    if found { "has" } else { "doesn't have" }
                );
                found
            }
            Err(e) => {
                log::warn!("can't read {}: {e}; not connected", path.display());
                false
            }
        },
    };
    seen.insert(key, Seen { stamp, found });
    found
}

/// Which agent tools under `home` have the `airboss` MCP server.
pub fn agent_tools(home: &Path) -> AgentTools {
    let codex = home.join(".codex/config.toml");
    AgentTools {
        claude: registered(&home.join(".claude.json"), "airboss", claude_has_airboss),
        codex: registered(&codex, "airboss", codex_has_airboss),
        codex_terminal_id: registered(&codex, "HC_TERMINAL_ID", codex_passes_terminal_id),
    }
}

/// Herdr's row from `launch.herdr_availability()` (SPEC-herdr "app"): "Ready" with its
/// version, or "Not installed". It shows when Herdr is installed or chosen in Settings.
fn herdr_row(herdr: &Availability, chosen: bool, launcher: LauncherKind) -> Option<Connection> {
    match herdr {
        Availability::Ready { version } => Some(row(
            "Herdr",
            true,
            "Ready",
            &match launcher {
                LauncherKind::Herdr => {
                    format!("Herdr {version}. Do it and Plan it start agents here.")
                }
                LauncherKind::T3 => format!(
                    "Herdr {version}. To start agents here, choose it in Settings › Agent setup."
                ),
            },
            None,
        )),
        Availability::Missing { reason } if chosen => Some(row(
            "Herdr",
            false,
            "Not installed",
            reason,
            Some(
                "Install Herdr, then choose Check again. Or start agents in T3 Code in Settings › Agent setup.",
            ),
        )),
        _ => None,
    }
}

/// Every row the footer shows, T3's and Herdr's availability, and the launcher Do it uses.
/// `agent_report` is the time of the newest `agent:*` event, which the UI shows as "Last
/// report 3m ago".
pub(crate) fn merged(
    services: &Services,
    _vm: &ViewModel,
    problems: &Problems,
    settings: &Settings,
    cli_status: CliStatus,
    agents: AgentTools,
    agent_report: Option<&str>,
) -> ConnectionsChanged {
    let cli = cli_at(Path::new(&cli_status.path));
    let availability = services.launch.availability();
    let herdr = services.launch.herdr_availability();
    let launcher = LauncherKind::from(settings.launcher(&availability, &herdr));
    // Herdr's own row goes first, so its "Ready" and version win over another ok "Herdr" row
    // (sync's "Live state"); a broken one still wins, as merging keeps the worst.
    let mut rows: Vec<Connection> = herdr_row(
        &herdr,
        settings.start_agents_in == StartAgentsIn::Herdr,
        launcher,
    )
    .into_iter()
    .collect();
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
    let connected = agents.claude || agents.codex;
    let codex_outdated = agents.codex_needs_terminal_id(launcher, settings.herdr_agent);
    rows.push(match (cli, agent_report) {
        (Cli::Ready, _) if !connected => row(
            "Agents",
            false,
            "Not connected",
            "Agents can't report to Airboss yet.",
            Some("Agents can't report to Airboss yet. Connect Claude Code in Settings › Agent setup."),
        ),
        // A Codex setup from before Herdr: Codex in Herdr can't say which terminal it is.
        (Cli::Ready, _) if codex_outdated => row(
            "Agents",
            false,
            "Out of date",
            CODEX_NEEDS_TERMINAL_ID,
            Some("Codex setup needs HC_TERMINAL_ID. Copy the Codex setup and put it in place of [mcp_servers.airboss] in ~/.codex/config.toml."),
        ),
        (Cli::Ready, Some(_)) => row("Agents", true, "Ready", detail, None),
        (Cli::Ready, None) => row("Agents", true, "Ready", "No agent has reported yet.", None),
        (Cli::Broken, _) => row(
            "Agents",
            false,
            "Link broken",
            "~/.local/bin/airboss points at a file that's gone or can't run.",
            Some("Install the command-line tool again in Settings, then connect Claude Code."),
        ),
        (Cli::Missing, _) => row(
            "Agents",
            false,
            "Not set up",
            detail,
            Some("Install the command-line tool, then connect Claude Code."),
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
    // Someone who uses Herdr and not T3 Code isn't told T3 Code is missing, unless they chose
    // T3 Code, whose row then says why Do it is off.
    let t3_missing = matches!(availability, Availability::Missing { .. });
    if t3_missing
        && matches!(herdr, Availability::Ready { .. })
        && settings.start_agents_in != StartAgentsIn::T3
    {
        merged.retain(|r| r.name != "T3 Code");
    }
    ConnectionsChanged {
        connections: merged,
        availability,
        herdr,
        launcher,
        agent_report: agent_report.map(str::to_string),
        cli: cli_status,
        agents,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    /// A throwaway home folder with `files` written under it.
    fn home(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("airboss-agents-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (path, text) in files {
            let p = dir.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        dir
    }

    #[test]
    fn claude_counts_only_a_top_level_airboss_server() {
        let yes = r#"{"numStartups":3,"mcpServers":{"airboss":{"type":"stdio","command":"/Users/x/.local/bin/airboss","args":["mcp"]}},"projects":{}}"#;
        assert_eq!(claude_has_airboss(yes), Ok(true));
        assert_eq!(
            claude_has_airboss(r#"{"mcpServers":{"other":{}}}"#),
            Ok(false)
        );
        assert_eq!(claude_has_airboss(r#"{"mcpServers":{}}"#), Ok(false));
        assert_eq!(claude_has_airboss("{}"), Ok(false));
        // A project-scoped server isn't the user-scope one Airboss needs.
        let project = r#"{"projects":{"/x":{"mcpServers":{"airboss":{}}}}}"#;
        assert_eq!(claude_has_airboss(project), Ok(false));
        assert!(claude_has_airboss("{not json").is_err());
        assert!(claude_has_airboss(r#"{"mcpServers":["airboss"]}"#).is_err());
    }

    #[test]
    fn codex_passes_hc_terminal_id_only_when_its_env_vars_name_it() {
        let old =
            "[mcp_servers.airboss]\ncommand = \"airboss\"\nenv_vars = [\"T3CODE_THREAD_ID\"]\n";
        assert_eq!(codex_passes_terminal_id(old), Ok(false));
        let new = "[mcp_servers.airboss]\ncommand = \"airboss\"\nenv_vars = [\"T3CODE_THREAD_ID\", \"HC_TERMINAL_ID\"]\n";
        assert_eq!(codex_passes_terminal_id(new), Ok(true));
        assert_eq!(
            codex_passes_terminal_id("[mcp_servers.airboss]\ncommand = \"airboss\"\n"),
            Ok(false)
        );
        // Another server's env_vars don't count.
        let other = "[mcp_servers.other]\nenv_vars = [\"HC_TERMINAL_ID\"]\n[mcp_servers.airboss]\ncommand = \"x\"\n";
        assert_eq!(codex_passes_terminal_id(other), Ok(false));
        assert!(codex_passes_terminal_id("[[[").is_err());
        // The text Copy Codex setup gives passes it.
        let copied = crate::commands::app::codex_setup(Path::new("/Users/x"));
        assert_eq!(codex_passes_terminal_id(&copied), Ok(true));
        assert_eq!(codex_has_airboss(&copied), Ok(true));
    }

    #[test]
    fn codex_counts_an_airboss_table_however_it_is_written() {
        let table = "model = \"o4\"\n[mcp_servers.airboss]\ncommand = \"/Users/x/.local/bin/airboss\"\nargs = [\"mcp\"]\nenv_vars = [\"T3CODE_THREAD_ID\"]\n";
        assert_eq!(codex_has_airboss(table), Ok(true));
        let inline = "[mcp_servers]\nairboss = { command = \"airboss\", args = [\"mcp\"] }\n";
        assert_eq!(codex_has_airboss(inline), Ok(true));
        assert_eq!(
            codex_has_airboss("mcp_servers.airboss.command = \"airboss\"\n"),
            Ok(true)
        );
        assert_eq!(
            codex_has_airboss("[mcp_servers.other]\ncommand = \"x\"\n"),
            Ok(false)
        );
        assert_eq!(
            codex_has_airboss("[mcp_servers]\nairboss = \"x\"\n"),
            Ok(false)
        );
        assert_eq!(codex_has_airboss(""), Ok(false));
        assert!(codex_has_airboss("[mcp_servers.airboss\ncommand =").is_err());
    }

    #[test]
    fn agent_tools_read_both_files_under_home_and_treat_problems_as_not_connected() {
        let claude = r#"{"mcpServers":{"airboss":{}}}"#;
        let codex = "[mcp_servers.airboss]\ncommand = \"airboss\"\n";
        let none = home("none", &[]);
        assert_eq!(agent_tools(&none), AgentTools::default());
        let both = home(
            "both",
            &[(".claude.json", claude), (".codex/config.toml", codex)],
        );
        assert_eq!(
            agent_tools(&both),
            AgentTools {
                claude: true,
                codex: true,
                codex_terminal_id: false,
            }
        );
        let broken = home(
            "broken",
            &[
                (".claude.json", "{\"mcpServers\": {\"airb"),
                (".codex/config.toml", "[[["),
            ],
        );
        assert_eq!(agent_tools(&broken), AgentTools::default());
        // A folder where the file should be can't be read: not connected, no panic.
        let odd = home("odd", &[(".claude.json/x", "")]);
        assert!(!agent_tools(&odd).claude);
        // Unreadable (no permission): not connected.
        let locked = home("locked", &[(".claude.json", claude)]);
        {
            use std::os::unix::fs::PermissionsExt;
            let p = locked.join(".claude.json");
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
            // Root can read anything; the check only means something for a normal user.
            if std::fs::read(&p).is_err() {
                assert!(!agent_tools(&locked).claude);
            }
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        for d in [none, both, broken, odd, locked] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn a_changed_file_is_read_again_and_an_unchanged_one_is_not() {
        let dir = home("cache", &[(".claude.json", "{}")]);
        assert!(!agent_tools(&dir).claude);
        // A different size changes the stamp, even within the same mtime tick.
        std::fs::write(dir.join(".claude.json"), r#"{"mcpServers":{"airboss":{}}}"#).unwrap();
        assert!(agent_tools(&dir).claude);
        // Removing it is noticed too.
        std::fs::remove_file(dir.join(".claude.json")).unwrap();
        assert!(!agent_tools(&dir).claude);
        let _ = std::fs::remove_dir_all(dir);
    }

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
