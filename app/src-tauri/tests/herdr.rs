//! Herdr from the app's side (SPEC-herdr "app"): the launcher "Start agents in" picks, the
//! Herdr connection row, Open thread on a Herdr terminal, and first run's folders from Herdr.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::PathBuf;

use airboss_app::commands::app as appcmd;
use airboss_app::commands::{detail, launch};
use airboss_app::dto::{LauncherKind, OpenTarget, ToastTone};
use airboss_app::errors::ErrorCode;
use airboss_app::herdr::Folder;
use airboss_app::settings::{Settings, StartAgentsIn};
use airboss_app::{Caller, UiEvent};
use airboss_core::{Actor, LinkKind, NewLink};
use airboss_services::{
    Availability, Connection, ErrorCode as SvcCode, HerdrAgent, LaunchReport, Launcher, Mode,
    ServiceError, Source, StartedThread, ThreadKey,
};
use common::{TestApp, temp_dir};

const M: Caller = Caller::Main;

fn ready(version: &str) -> Availability {
    Availability::Ready {
        version: version.into(),
    }
}

fn missing(reason: &str) -> Availability {
    Availability::Missing {
        reason: reason.into(),
    }
}

fn herdr_key(r: &str) -> ThreadKey {
    ThreadKey {
        source: Source::Herdr,
        r#ref: r.into(),
    }
}

/// Saves "Start agents in" and "Agent in Herdr", as Settings does.
fn choose(app: &TestApp, start_agents_in: StartAgentsIn, herdr_agent: HerdrAgent) -> Settings {
    let s = Settings {
        start_agents_in,
        herdr_agent,
        ..app.state.settings()
    };
    appcmd::set_settings(&app.state, M, s).unwrap()
}

/// The launcher the last Do it or Plan it asked for.
fn last_launcher(app: &TestApp) -> Launcher {
    app.launch.starts().last().unwrap().launcher
}

#[test]
fn automatic_starts_in_t3_when_ready_else_in_herdr_when_ready() {
    let app = TestApp::new();
    let t = app.add("T", None);
    app.sync_vm();
    // T3 Ready (the fakes' default), Herdr missing: T3, as before Herdr.
    launch::do_it(&app.state, M, t, None, None).unwrap();
    assert_eq!(last_launcher(&app), Launcher::T3);
    // Both Ready: still T3.
    app.launch.set_herdr_availability(ready("0.1.18"));
    launch::plan_it(&app.state, M, t, None).unwrap();
    assert_eq!(last_launcher(&app), Launcher::T3);
    // T3 missing, Herdr Ready: Herdr with Claude Code.
    app.launch
        .set_availability(missing("T3 Code isn't installed."));
    launch::do_it(&app.state, M, t, None, None).unwrap();
    assert_eq!(
        last_launcher(&app),
        Launcher::Herdr {
            agent: HerdrAgent::Claude
        }
    );
    // Neither: T3, whose error says what's missing.
    app.launch
        .set_herdr_availability(missing("Herdr isn't installed."));
    launch::do_it(&app.state, M, t, None, None).unwrap();
    assert_eq!(last_launcher(&app), Launcher::T3);
    app.assert_lock_free();
}

#[test]
fn a_chosen_launcher_and_agent_are_kept_whatever_is_ready() {
    let app = TestApp::new();
    let t = app.add("T", None);
    app.sync_vm();
    choose(&app, StartAgentsIn::Herdr, HerdrAgent::Codex);
    launch::do_it(&app.state, M, t, None, None).unwrap();
    assert_eq!(
        last_launcher(&app),
        Launcher::Herdr {
            agent: HerdrAgent::Codex
        }
    );
    app.launch.set_herdr_availability(ready("0.1.18"));
    app.launch.set_availability(missing("gone"));
    choose(&app, StartAgentsIn::T3, HerdrAgent::Codex);
    launch::plan_it(&app.state, M, t, None).unwrap();
    assert_eq!(last_launcher(&app), Launcher::T3);
}

#[test]
fn settings_round_trip_and_tell_the_window_where_agents_start() {
    let dir = temp_dir("herdr-settings");
    let path = dir.join("app-settings.json");
    let app = TestApp::new();
    app.launch.set_herdr_availability(ready("0.1.18"));
    app.state.catch_up_services();
    let saved = choose(&app, StartAgentsIn::Herdr, HerdrAgent::Codex);
    assert_eq!(appcmd::get_settings(&app.state, M).unwrap(), saved);
    airboss_app::settings::save(&path, &saved).unwrap();
    assert_eq!(airboss_app::settings::load(&path), (saved, false));
    // The change reaches the window: Herdr is the launcher now.
    let last = app
        .shell
        .events()
        .into_iter()
        .rev()
        .find_map(|e| match e {
            UiEvent::ConnectionsChanged(c) => Some(c),
            _ => None,
        })
        .unwrap();
    assert_eq!(last.launcher, LauncherKind::Herdr);
    assert_eq!(last.herdr, ready("0.1.18"));
    // And bootstrap says the same.
    let b = detail::bootstrap(&app.state, M).unwrap();
    assert_eq!(
        (b.launcher, b.settings.herdr_agent),
        (LauncherKind::Herdr, HerdrAgent::Codex)
    );
}

#[test]
fn a_second_do_it_names_herdr_when_agents_start_there() {
    let app = TestApp::new();
    let t = app.add("T", None);
    app.sync_vm();
    choose(&app, StartAgentsIn::Herdr, HerdrAgent::Claude);
    app.gate.close();
    let state = app.state.clone();
    let first = std::thread::spawn(move || launch::do_it(&state, M, t, None, None));
    while app.gate.entered.load(std::sync::atomic::Ordering::SeqCst) == 0 {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let e = launch::do_it(&app.state, M, t, None, None).unwrap_err();
    assert_eq!(e.code, ErrorCode::AlreadyStarting);
    assert_eq!(e.message, "Already starting — check Herdr in a moment.");
    app.gate.open();
    first.join().unwrap().unwrap();
}

#[test]
fn herdr_errors_read_well_and_offer_retry_never_add_to_t3() {
    let app = TestApp::new();
    let t = app.add("T", None);
    app.sync_vm();
    choose(&app, StartAgentsIn::Herdr, HerdrAgent::Claude);
    app.launch.set_start(Err(ServiceError::new(
        SvcCode::HerdrNotRunning,
        "Herdr didn't open. Open it and try again.",
    )));
    let e = launch::do_it(&app.state, M, t, None, None).unwrap_err();
    assert_eq!(e.code, ErrorCode::HerdrNotRunning);
    // Quick add's launch shows its result as a toast.
    app.launch.set_start(Err(ServiceError::new(
        SvcCode::HerdrMissing,
        "Herdr isn't installed.",
    )));
    let _ = launch::do_it(&app.state, Caller::QuickAdd, t, None, None);
    let toast = app
        .shell
        .events()
        .into_iter()
        .rev()
        .find_map(|e| match e {
            UiEvent::Toast(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        toast.text,
        format!("Couldn't start #{}: Herdr isn't installed.", t.0)
    );
    let labels: Vec<&str> = toast.actions.iter().map(|a| a.label.as_str()).collect();
    assert_eq!(labels, vec!["Retry"]);
    // A place that failed in Herdr is retried, never added to T3.
    let web = app.core(|s| {
        s.add_repo("web", std::path::Path::new("/src/web"), None, &Actor::Me)
            .unwrap()
            .id
    });
    let report = LaunchReport {
        started: vec![StartedThread {
            key: herdr_key("t4p"),
            mode: Mode::DoIt,
            repo: Some(web),
            title: "1 T".into(),
            linked: true,
        }],
        skipped: vec![],
        failed: vec![(
            None,
            ServiceError::new(
                SvcCode::WorktreeFailed,
                "fatal: 'task-1-t' is already used.",
            ),
        )],
        branch: Some("task-1-t".into()),
        moved_to_doing: true,
    };
    let toast = launch::report_toast(t, Mode::DoIt, &report, &|_| "web".into());
    assert_eq!(toast.tone, Some(ToastTone::Warning));
    let labels: Vec<&str> = toast.actions.iter().map(|a| a.label.as_str()).collect();
    assert_eq!(labels, vec!["Retry"]);
}

#[test]
fn open_thread_shows_a_herdr_terminal_through_herdr_app_then_brings_herdr_forward() {
    let app = TestApp::new();
    appcmd::open_thread(&app.state, M, herdr_key("t4p")).unwrap();
    assert_eq!(
        app.shell.calls().last().unwrap(),
        r#"show_herdr_terminal(herdr-app do agent.focus {"terminal":"t4p"}; open -b dev.herdr.app)"#
    );
    // The link itself opens the same way: it has no URL, and never reaches the allowlist.
    let t = app.add("T", None);
    let link = app.core(|s| {
        s.link(
            t,
            NewLink::new(LinkKind::HerdrTerminal, "vps/t7"),
            &Actor::Me,
        )
        .unwrap()
    });
    appcmd::open_url(&app.state, M, OpenTarget::Link(link.id)).unwrap();
    assert_eq!(
        app.shell.calls().last().unwrap(),
        r#"show_herdr_terminal(herdr-app do agent.focus {"terminal":"vps/t7"}; open -b dev.herdr.app)"#
    );
    assert!(!app.shell.calls().iter().any(|c| c.starts_with("open_url")));
    // Herdr couldn't show it: the toast's sentence.
    app.shell.fail_herdr(true);
    let e = appcmd::open_thread(&app.state, M, herdr_key("t4p")).unwrap_err();
    assert_eq!(
        (e.code, e.message.as_str()),
        (
            ErrorCode::HerdrFailed,
            "Couldn't show that terminal in Herdr."
        )
    );
    // An address core would never store isn't passed on.
    let before = app.shell.calls().len();
    for bad in ["", "t 4", &"t".repeat(65), "t4\n"] {
        assert!(appcmd::open_thread(&app.state, M, herdr_key(bad)).is_err());
    }
    assert_eq!(app.shell.calls().len(), before);
}

fn row<'a>(rows: &'a [Connection], name: &str) -> Option<&'a Connection> {
    rows.iter().find(|r| r.name == name)
}

#[test]
fn the_herdr_row_shows_when_herdr_is_installed_or_chosen() {
    let app = TestApp::new();
    // Not installed, not chosen: no row (the fakes start with Herdr missing).
    app.state.catch_up_services();
    assert!(row(&app.state.connection_rows(), "Herdr").is_none());
    // Chosen: Not installed.
    choose(&app, StartAgentsIn::Herdr, HerdrAgent::Claude);
    let rows = app.state.connection_rows();
    let h = row(&rows, "Herdr").unwrap();
    assert!(!h.ok);
    assert_eq!(h.state, "Not installed");
    assert_eq!(h.detail, "Herdr isn't installed.");
    // Installed: Ready, with its version, after T3 Code in the footer's order.
    choose(&app, StartAgentsIn::Auto, HerdrAgent::Claude);
    app.launch.set_herdr_availability(ready("0.1.18"));
    app.sync.set_connections(vec![Connection {
        name: "T3 Code".into(),
        ok: true,
        state: "Live state".into(),
        detail: "threads".into(),
        fix: None,
    }]);
    app.state.catch_up_services();
    let rows = app.state.connection_rows();
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(&names[..2], &["T3 Code", "Herdr"]);
    let h = row(&rows, "Herdr").unwrap();
    assert!(h.ok);
    assert_eq!(h.state, "Ready");
    assert!(h.detail.contains("0.1.18"), "{}", h.detail);
}

#[test]
fn herdr_ready_and_no_t3_hides_the_t3_row_unless_t3_is_chosen() {
    let app = TestApp::new();
    app.launch
        .set_availability(missing("T3 Code isn't installed."));
    app.launch.set_herdr_availability(ready("0.1.18"));
    app.state.catch_up_services();
    let rows = app.state.connection_rows();
    assert!(row(&rows, "T3 Code").is_none(), "{rows:?}");
    assert!(row(&rows, "Herdr").unwrap().ok);
    // A broken Herdr row from another service still wins over Ready.
    app.sync.set_connections(vec![Connection {
        name: "Herdr".into(),
        ok: false,
        state: "Can't read".into(),
        detail: "Herdr's status can't be read".into(),
        fix: None,
    }]);
    app.state.catch_up_services();
    assert!(!row(&app.state.connection_rows(), "Herdr").unwrap().ok);
    // Someone who chose T3 Code is told why it's off.
    choose(&app, StartAgentsIn::T3, HerdrAgent::Claude);
    let t3 = row(&app.state.connection_rows(), "T3 Code")
        .cloned()
        .unwrap();
    assert!(!t3.ok);
}

#[test]
fn first_run_lists_herdr_folders_with_the_same_rules_as_t3_projects() {
    let app = TestApp::new();
    // No Herdr: nothing, and no error.
    assert!(detail::herdr_folders(&app.state, M).unwrap().is_empty());
    let dir = temp_dir("herdr-folders");
    let git = dir.join("booking-web");
    std::fs::create_dir_all(git.join(".git")).unwrap();
    let plain = dir.join("notes");
    std::fs::create_dir_all(&plain).unwrap();
    let other = dir.join("api");
    std::fs::create_dir_all(other.join(".git")).unwrap();
    app.core(|s| s.add_repo("booking-web", &git, None, &Actor::Me).unwrap());
    app.sync_vm();
    let folder = |name: &str, path: &PathBuf| Folder {
        name: name.into(),
        path: path.clone(),
    };
    app.shell.herdr_folders_fake().answer(Ok(vec![
        folder("booking", &git),
        folder("notes", &plain),
        folder("api", &other),
    ]));
    let list = detail::herdr_folders(&app.state, M).unwrap();
    let got: Vec<(&str, bool, bool)> = list
        .iter()
        .map(|r| (r.name.as_str(), r.is_git, r.added))
        .collect();
    assert_eq!(
        got,
        vec![
            ("booking", true, true),
            ("notes", false, false),
            ("api", true, false)
        ]
    );
    // It's a command the window may call.
    let v = app.call("herdr_folders", serde_json::json!({})).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 3);
}

#[test]
fn a_herdr_terminal_link_shows_its_thread_in_the_task_detail() {
    let app = TestApp::new();
    let t = app.add("T", None);
    app.core(|s| {
        s.link(t, NewLink::new(LinkKind::HerdrTerminal, "t4p"), &Actor::Me)
            .unwrap()
    });
    app.sync_vm();
    let mut snap = airboss_services::SyncSnapshot::default();
    snap.threads.insert(
        herdr_key("t4p"),
        airboss_services::ThreadView {
            key: herdr_key("t4p"),
            linked: true,
            state: airboss_services::ThreadState::NeedsMe,
            title: Some("1 T".into()),
            request: None,
            error: None,
            run_id: None,
            project_path: Some("/w/web".into()),
            branch: None,
            worktree_path: None,
            parent_ref: None,
            child_count: 0,
            last_activity_at: None,
            stale: false,
        },
    );
    app.sync.set_snapshot(snap);
    let d = detail::task_detail(&app.state, M, t, 1, 30).unwrap();
    let l = d
        .links
        .iter()
        .find(|l| l.kind == LinkKind::HerdrTerminal)
        .unwrap();
    let th = l.thread.as_ref().unwrap();
    assert_eq!(th.key, herdr_key("t4p"));
    assert_eq!(th.chip, Some(airboss_services::Chip::WaitingOnYou));
    assert_eq!(th.url, None);
}

/// A working `~/.local/bin/airboss` under the test's home.
fn install_fake_cli(app: &TestApp) {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let cli = temp_dir("cli").join("airboss");
    std::fs::write(&cli, b"#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
    let bin = app.home.join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    symlink(&cli, bin.join("airboss")).unwrap();
}

fn agents_row(app: &TestApp) -> Connection {
    row(&app.state.connection_rows(), "Agents")
        .cloned()
        .unwrap()
}

#[test]
fn codex_in_herdr_with_a_setup_from_before_herdr_turns_agents_orange_until_it_passes_hc_terminal_id()
 {
    let app = TestApp::new();
    install_fake_cli(&app);
    let config = app.home.join(".codex/config.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    let old = "[mcp_servers.airboss]\ncommand = \"/x/airboss\"\nargs = [\"mcp\"]\nenv_vars = [\"T3CODE_THREAD_ID\"]\n";
    std::fs::write(&config, old).unwrap();
    app.launch.set_herdr_availability(ready("0.1.18"));
    // On T3 Code, or Claude Code in Herdr, the old setup is fine.
    app.state.catch_up_services();
    assert!(agents_row(&app).ok, "{:?}", agents_row(&app));
    choose(&app, StartAgentsIn::Herdr, HerdrAgent::Claude);
    assert!(agents_row(&app).ok);
    // Codex in Herdr: it couldn't say which terminal it is.
    choose(&app, StartAgentsIn::Herdr, HerdrAgent::Codex);
    let a = agents_row(&app);
    assert!(!a.ok);
    assert_eq!(a.detail, "Codex setup needs HC_TERMINAL_ID");
    assert!(
        a.fix
            .unwrap()
            .starts_with("Codex setup needs HC_TERMINAL_ID.")
    );
    let b = detail::bootstrap(&app.state, M).unwrap();
    assert!(b.agents.codex && !b.agents.codex_terminal_id);
    // The fix is the current Codex setup.
    appcmd::copy_text(&app.state, M, airboss_app::dto::CopyWhat::CodexSetup).unwrap();
    let copied = app.shell.calls().last().unwrap().clone();
    assert!(
        copied.contains(r#"env_vars = ["T3CODE_THREAD_ID", "HC_TERMINAL_ID"]"#),
        "{copied}"
    );
    // Pasted in: Ready again.
    std::fs::write(&config, airboss_app::commands::app::codex_setup(&app.home)).unwrap();
    app.state.catch_up_services();
    assert!(agents_row(&app).ok, "{:?}", agents_row(&app));
}
