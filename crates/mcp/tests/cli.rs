//! The real binary as a CLI, against a temporary `AIRBOSS_DB`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;

use airboss_core::{Actor, LinkKind, NewLink, NewTask, Status, Store, TaskId};
use common::{TempDir, run, stderr, stdout};
use serde_json::Value;

fn ok(db: &Path, args: &[&str]) -> String {
    let out = run(db, args);
    assert!(out.status.success(), "{args:?} failed: {}", stderr(&out));
    assert!(
        stderr(&out).is_empty(),
        "{args:?} wrote to stderr: {}",
        stderr(&out)
    );
    stdout(&out)
}

fn json(db: &Path, args: &[&str]) -> Value {
    let mut all = args.to_vec();
    all.push("--json");
    serde_json::from_str(&ok(db, &all)).unwrap()
}

/// One line on stderr, exit code 1; returns the message.
fn fails(db: &Path, args: &[&str]) -> String {
    let out = run(db, args);
    assert_eq!(out.status.code(), Some(1), "{args:?}: {}", stdout(&out));
    assert!(out.stdout.is_empty());
    let err = stderr(&out);
    assert_eq!(err.lines().count(), 1, "{err}");
    err.trim_end()
        .strip_prefix("airboss: ")
        .unwrap()
        .to_string()
}

/// A small tree: Booking (web, api) > Schema (doing), API (waits on Schema); Docs (blocked);
/// Old (done); and an agent's proposal.
fn seed(db: &Path) -> Vec<TaskId> {
    let mut s = Store::open(db).unwrap();
    let me = Actor::Me;
    let web = s
        .add_repo(
            "web",
            Path::new("/src/web"),
            Some("git@github.com:acme/web.git"),
            &me,
        )
        .unwrap()
        .id;
    let api = s
        .add_repo("api", Path::new("/src/api"), None, &me)
        .unwrap()
        .id;
    let booking = s
        .add_task(NewTask::new("Booking").in_repo(web).in_repo(api), &me)
        .unwrap()
        .id;
    let schema = s
        .add_task(NewTask::new("Schema").under(booking), &me)
        .unwrap()
        .id;
    let endpoint = s
        .add_task(NewTask::new("API").under(booking), &me)
        .unwrap()
        .id;
    let docs = s.add_task(NewTask::new("Docs"), &me).unwrap().id;
    let old = s.add_task(NewTask::new("Old"), &me).unwrap().id;
    s.add_dependency(endpoint, schema, &me).unwrap();
    s.set_status(schema, Status::Doing, None, &me).unwrap();
    s.set_status(docs, Status::Blocked, Some("needs copy"), &me)
        .unwrap();
    s.set_status(old, Status::Done, None, &me).unwrap();
    let idea = s
        .add_task(NewTask::new("Idea"), &Actor::Agent("t3/x".into()))
        .unwrap()
        .id;
    s.set_branch(booking, Some("booking-flow"), &me).unwrap();
    s.link(
        booking,
        NewLink::pr("acme/web#42").with_title("Booking PR"),
        &me,
    )
    .unwrap();
    vec![booking, schema, endpoint, docs, old, idea]
}

#[test]
fn help_lists_the_commands() {
    let dir = TempDir::new();
    let text = ok(&dir.db(), &["--help"]);
    for cmd in ["ls", "show", "log", "repo", "mcp"] {
        assert!(text.contains(cmd), "{cmd} missing from:\n{text}");
    }
}

#[test]
fn version_prints_the_crate_version() {
    let dir = TempDir::new();
    assert_eq!(
        ok(&dir.db(), &["--version"]).trim(),
        format!("airboss {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn ls_on_an_empty_database_says_how_to_start() {
    let dir = TempDir::new();
    assert_eq!(
        ok(&dir.db(), &["ls"]),
        "No tasks yet. Add one with: airboss add \"What to do\"\n"
    );
}

#[test]
fn ls_prints_the_tree_with_marks_in_aligned_columns() {
    let dir = TempDir::new();
    seed(&dir.db());
    let text = ok(&dir.db(), &["ls"]);
    assert_eq!(
        text,
        "\
#1  ○ Booking   api, web
#2    ◑ Schema
#3    ○ API               waiting
#4  ⊖ Docs                blocked: needs copy
#6  ◌ Idea                proposed, waiting for you
"
    );
}

#[test]
fn ls_all_includes_done_and_dropped() {
    let dir = TempDir::new();
    seed(&dir.db());
    let text = ok(&dir.db(), &["ls", "--all"]);
    assert!(text.contains("#5  ● Old"), "{text}");
}

#[test]
fn ls_keeps_a_finished_parent_when_something_under_it_is_open() {
    let dir = TempDir::new();
    let mut s = Store::open(&dir.db()).unwrap();
    let p = s.add_task(NewTask::new("Parent"), &Actor::Me).unwrap().id;
    s.add_task(NewTask::new("Child"), &Actor::Me).unwrap();
    let c = s
        .add_task(NewTask::new("Open child").under(p), &Actor::Me)
        .unwrap()
        .id;
    s.set_status(p, Status::Done, None, &Actor::Me).unwrap();
    s.set_status(c, Status::Todo, None, &Actor::Me).unwrap();
    drop(s);
    let text = ok(&dir.db(), &["ls"]);
    assert!(text.contains("● Parent"), "{text}");
    assert!(text.contains("  ○ Open child"), "{text}");
}

#[test]
fn ls_ready_lists_what_can_start_now() {
    let dir = TempDir::new();
    seed(&dir.db());
    let text = ok(&dir.db(), &["ls", "--ready"]);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1, "{text}");
    assert!(
        lines[0].starts_with("#1  ○ Booking  api, web  ready since 20"),
        "{text}"
    );
}

#[test]
fn ls_json_round_trips_the_tool_shape() {
    let dir = TempDir::new();
    seed(&dir.db());
    let v = json(&dir.db(), &["ls"]);
    let tasks = v["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 5);
    assert_eq!(tasks[0]["repos"], serde_json::json!(["api", "web"]));
    assert_eq!(tasks[2]["waiting"], true);
    assert_eq!(v["truncated"], false);
}

#[test]
fn output_has_no_colour_when_piped_even_without_no_color() {
    let dir = TempDir::new();
    seed(&dir.db());
    for args in [&["ls"][..], &["show", "1"], &["log"], &["repo", "ls"]] {
        assert!(!ok(&dir.db(), args).contains('\x1b'), "{args:?}");
    }
}

#[test]
fn show_prints_every_section() {
    let dir = TempDir::new();
    let ids = seed(&dir.db());
    let text = ok(&dir.db(), &["show", "1"]);
    for want in [
        "#1 Booking",
        "Status    ○ Todo",
        "Repos     api, web",
        "Branch    booking-flow",
        "Links     pr  Booking PR  acme/web#42  open",
        "Subtasks  ◑ #2 Schema",
        "          ○ #3 API",
        "Run order Wave 1  #2",
        "          Wave 2  #3",
        "History",
    ] {
        assert!(text.contains(want), "missing {want:?} in:\n{text}");
    }
    let text = ok(&dir.db(), &["show", &ids[2].0.to_string()]);
    assert!(text.contains("Status    ○ Waiting"), "{text}");
    assert!(text.contains("Parent    #1 Booking"), "{text}");
    assert!(text.contains("Waits on  ◑ #2 Schema"), "{text}");
    let text = ok(&dir.db(), &["show", "4"]);
    assert!(text.contains("Why       needs copy"), "{text}");
}

#[test]
fn show_json_is_the_get_task_shape() {
    let dir = TempDir::new();
    seed(&dir.db());
    let v = json(&dir.db(), &["show", "1"]);
    assert_eq!(v["task"]["branch"], "booking-flow");
    assert_eq!(v["links"][0]["title"], "Booking PR");
    assert_eq!(v["run_order"]["waves"][0][0]["id"], 2);
}

#[test]
fn show_a_missing_task_is_one_line_with_the_agents_message() {
    let dir = TempDir::new();
    assert_eq!(
        fails(&dir.db(), &["show", "99"]),
        "There's no task #99. List the tasks to find the right id."
    );
}

#[test]
fn log_shows_recent_history_newest_last() {
    let dir = TempDir::new();
    seed(&dir.db());
    let text = ok(&dir.db(), &["log"]);
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[0].contains("#1  me") && lines[0].contains("created"),
        "{text}"
    );
    assert!(lines.last().unwrap().contains("linked"), "{text}");
    assert!(text.contains("→ in progress"), "{text}");
    assert!(text.contains("agent:t3/x"), "{text}");
}

#[test]
fn log_after_shows_only_newer_events() {
    let dir = TempDir::new();
    seed(&dir.db());
    let all = json(&dir.db(), &["log"]);
    let n = all.as_array().unwrap().len();
    let after = all[n - 2]["id"].as_i64().unwrap().to_string();
    let newer = json(&dir.db(), &["log", "--after", &after]);
    assert_eq!(newer.as_array().unwrap().len(), 1);
    assert_eq!(newer[0]["kind"], "linked");
}

#[test]
fn repo_ls_lists_names_paths_and_remotes() {
    let dir = TempDir::new();
    seed(&dir.db());
    assert_eq!(
        ok(&dir.db(), &["repo", "ls"]),
        "api  /src/api\nweb  /src/web  git@github.com:acme/web.git\n"
    );
    let v = json(&dir.db(), &["repo", "ls"]);
    assert_eq!(v[1]["remote_url"], "git@github.com:acme/web.git");
}

#[test]
fn a_link_to_a_thread_shows_in_show() {
    let dir = TempDir::new();
    let mut s = Store::open(&dir.db()).unwrap();
    let id = s.add_task(NewTask::new("T"), &Actor::Me).unwrap().id;
    s.link(
        id,
        NewLink::new(LinkKind::T3Thread, "abc"),
        &Actor::Agent("t3/abc".into()),
    )
    .unwrap();
    drop(s);
    let text = ok(&dir.db(), &["show", "1"]);
    assert!(text.contains("Links     t3_thread  abc"), "{text}");
}
