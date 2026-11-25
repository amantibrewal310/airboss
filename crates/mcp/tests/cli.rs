//! The real binary as a CLI, against a temporary `AIRBOSS_DB`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;

use airboss_core::{Actor, LinkKind, NewLink, NewTask, Status, Store, TaskId};
use common::{NEWER_DATABASE, TempDir, as_if_a_newer_airboss_migrated, run, stderr, stdout};
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

// ---- writes ----

#[test]
fn add_makes_a_todo_task_with_parent_repos_and_body() {
    let dir = TempDir::new();
    let db = dir.db();
    ok(&db, &["repo", "add", "web", "/src/web"]);
    ok(&db, &["repo", "add", "api", "/src/api"]);
    assert_eq!(ok(&db, &["add", "Booking"]), "Added ○ #1 Booking\n");
    assert_eq!(
        ok(
            &db,
            &[
                "add", "Schema", "--under", "1", "--repo", "web", "--repo", "api", "--body", "why"
            ]
        ),
        "Added ○ #2 Schema under #1 in api, web\n"
    );
    let v = json(&db, &["add", "Third"]);
    assert_eq!(v["id"], 3);
    assert_eq!(v["status"], "todo");
    assert_eq!(v["origin"], "me");
    let detail = json(&db, &["show", "2"]);
    assert_eq!(detail["task"]["body"], "why");
    assert_eq!(detail["task"]["parent"], 1);
}

#[test]
fn add_with_an_unknown_repo_lists_the_registered_ones() {
    let dir = TempDir::new();
    ok(&dir.db(), &["repo", "add", "web", "/src/web"]);
    assert_eq!(
        fails(&dir.db(), &["add", "X", "--repo", "wbe"]),
        "There's no repo named \"wbe\". Registered repos: web."
    );
    assert_eq!(
        fails(&dir.db(), &["add", " "]),
        "Give every task a title that isn't blank."
    );
}

#[test]
fn set_done_and_drop_change_status() {
    let dir = TempDir::new();
    let db = dir.db();
    for t in ["A", "B", "C", "D"] {
        ok(&db, &["add", t]);
    }
    ok(&db, &["add", "C1", "--under", "3"]);
    assert_eq!(
        ok(&db, &["set", "1", "doing"]),
        "◑ #1 A is now in progress\n"
    );
    assert_eq!(
        ok(&db, &["set", "1", "in_review"]),
        "◕ #1 A is now in review\n"
    );
    assert_eq!(
        ok(&db, &["set", "2", "blocked", "--note", "waiting on copy"]),
        "⊖ #2 B is now blocked\n"
    );
    assert_eq!(
        fails(&db, &["set", "4", "blocked"]),
        "Say why it's blocked in `note`."
    );
    assert_eq!(
        ok(&db, &["done", "1", "--note", "shipped"]),
        "● #1 A is now done\n"
    );
    assert_eq!(
        ok(&db, &["drop", "3"]),
        "⊗ #3 C is now dropped, with 1 open subtask\n"
    );
    assert_eq!(json(&db, &["show", "5"])["task"]["status"], "dropped");
    assert_eq!(json(&db, &["set", "1", "todo"])["status"], "todo");
    assert_eq!(json(&db, &["done", "4"])["status"], "done");
}

#[test]
fn accept_and_reject_only_take_proposals() {
    let dir = TempDir::new();
    let db = dir.db();
    {
        let mut s = Store::open(&db).unwrap();
        let agent = Actor::Agent("t3/x".into());
        s.add_task(NewTask::new("Idea one"), &agent).unwrap();
        s.add_task(NewTask::new("Idea two"), &agent).unwrap();
    }
    ok(&db, &["add", "Mine"]);
    assert_eq!(
        ok(&db, &["accept", "1"]),
        "Accepted ○ #1 Idea one; it's todo now\n"
    );
    assert_eq!(
        ok(&db, &["reject", "2", "--note", "not now"]),
        "Rejected ⊗ #2 Idea two; it's dropped and hidden from the tree\n"
    );
    assert!(!ok(&db, &["ls", "--all"]).contains("Idea two"));
    assert_eq!(
        fails(&db, &["accept", "3"]),
        "Task #3 isn't a proposal; it's todo. Use airboss set, done or drop instead."
    );
    let s = Store::open(&db).unwrap();
    let last = s.history(TaskId(2)).unwrap().pop().unwrap();
    assert_eq!(last.body.as_deref(), Some("not now"));
}

#[test]
fn edit_changes_title_and_notes() {
    let dir = TempDir::new();
    let db = dir.db();
    ok(&db, &["add", "Old", "--body", "notes"]);
    assert_eq!(
        ok(&db, &["edit", "1", "--title", "New"]),
        "Edited ○ #1 New\n"
    );
    let v = json(&db, &["edit", "1", "--body", ""]);
    assert!(v.get("body").is_none(), "{v}");
    assert_eq!(fails(&db, &["edit", "1"]), "Give --title, --body, or both.");
}

#[test]
fn mv_moves_and_reorders() {
    let dir = TempDir::new();
    let db = dir.db();
    for t in ["A", "B", "C"] {
        ok(&db, &["add", t]);
    }
    assert_eq!(
        ok(&db, &["mv", "3", "--under", "1"]),
        "Moved ○ #3 C under #1, position 1\n"
    );
    assert_eq!(
        ok(&db, &["mv", "2", "--pos", "1"]),
        "Moved ○ #2 B at the top level, position 1\n"
    );
    assert_eq!(
        ok(&db, &["mv", "3", "--top", "--pos", "1"]),
        "Moved ○ #3 C at the top level, position 1\n"
    );
    ok(&db, &["mv", "1", "--under", "3"]);
    assert_eq!(
        fails(&db, &["mv", "3", "--under", "1"]),
        "Task #1 is inside #3, so #3 can't move under it."
    );
    assert!(!run(&db, &["mv", "3"]).status.success());
}

#[test]
fn wait_and_unwait() {
    let dir = TempDir::new();
    let db = dir.db();
    ok(&db, &["add", "Design"]);
    ok(&db, &["add", "Build"]);
    assert_eq!(
        ok(&db, &["wait", "2", "--on", "1"]),
        "○ #2 Build now waits on ○ #1 Design\n"
    );
    assert!(ok(&db, &["ls"]).contains("waiting"));
    assert_eq!(
        fails(&db, &["wait", "1", "--on", "2"]),
        "That would make a loop of tasks waiting on each other. Remove one of the dependencies."
    );
    assert_eq!(
        ok(&db, &["unwait", "2", "--on", "1"]),
        "○ #2 Build no longer waits on ○ #1 Design\n"
    );
    assert_eq!(
        fails(&db, &["unwait", "2", "--on", "1"]),
        "Task #2 doesn't wait on #1."
    );
    assert_eq!(
        fails(&db, &["wait", "2", "--on", "9"]),
        "There's no task #9. List the tasks to find the right id."
    );
}

#[test]
fn link_attaches_a_pr_with_url_and_title() {
    let dir = TempDir::new();
    let db = dir.db();
    ok(&db, &["add", "Code"]);
    assert_eq!(
        ok(&db, &["link", "1", "pr", "acme/web#42"]),
        "Linked pr acme/web#42 to ○ #1 Code\n"
    );
    let v = json(
        &db,
        &[
            "link",
            "1",
            "doc",
            "/plans/a.md",
            "--title",
            "a.md",
            "--url",
            "https://x",
        ],
    );
    assert_eq!(v["kind"], "doc");
    assert_eq!(v["title"], "a.md");
    assert_eq!(v["linked_by"], "me");
    assert!(!run(&db, &["link", "1", "wiki", "x"]).status.success());
}

#[test]
fn repo_add_link_and_unlink() {
    let dir = TempDir::new();
    let db = dir.db();
    let src = dir.0.join("checkout");
    std::fs::create_dir_all(&src).unwrap();
    let src = src.canonicalize().unwrap();
    let out = ok(
        &db,
        &[
            "repo",
            "add",
            "web",
            src.to_str().unwrap(),
            "--remote",
            "git@x:web.git",
        ],
    );
    assert_eq!(out, format!("Added repo web at {}\n", src.display()));
    let v = json(&db, &["repo", "add", "api", "/src/api"]);
    assert_eq!(v["name"], "api");
    assert_eq!(
        fails(&db, &["repo", "add", "web", "/elsewhere"]),
        "That name or path is already registered. List the repos to see it."
    );

    ok(&db, &["add", "Task"]);
    assert_eq!(
        ok(&db, &["repo", "link", "1", "web"]),
        "○ #1 Task is linked to web\n"
    );
    assert_eq!(
        ok(&db, &["repo", "link", "1", "api"]),
        "○ #1 Task is linked to api, web\n"
    );
    assert_eq!(
        json(&db, &["repo", "unlink", "1", "web"])["repos"],
        serde_json::json!(["api"])
    );
    assert_eq!(
        ok(&db, &["repo", "unlink", "1", "api"]),
        "○ #1 Task has no repos now\n"
    );
    assert_eq!(
        fails(&db, &["repo", "unlink", "1", "api"]),
        "Task #1 isn't linked to the repo \"api\"."
    );
}

#[test]
fn repo_edit_renames_moves_and_changes_the_remote() {
    let dir = TempDir::new();
    let db = dir.db();
    ok(
        &db,
        &[
            "repo",
            "add",
            "web",
            "/src/web",
            "--remote",
            "git@x:web.git",
        ],
    );
    ok(&db, &["add", "Task", "--repo", "web"]);
    let src = dir.0.join("moved");
    std::fs::create_dir_all(&src).unwrap();
    let src = src.canonicalize().unwrap();
    assert_eq!(
        ok(
            &db,
            &[
                "repo",
                "edit",
                "web",
                "--name",
                "site",
                "--path",
                src.to_str().unwrap()
            ]
        ),
        format!("Edited repo site at {}\n", src.display())
    );
    assert_eq!(
        json(&db, &["show", "1"])["task"]["repos"],
        serde_json::json!(["site"])
    );
    let v = json(&db, &["repo", "edit", "site", "--no-remote"]);
    assert_eq!(v["remote_url"], Value::Null);
    let v = json(&db, &["repo", "edit", "site", "--remote", "git@x:site.git"]);
    assert_eq!(v["remote_url"], "git@x:site.git");

    ok(&db, &["repo", "add", "api", "/src/api"]);
    assert_eq!(
        fails(&db, &["repo", "edit", "site", "--name", "api"]),
        "That name or path is already registered. List the repos to see it."
    );
    assert_eq!(
        fails(&db, &["repo", "edit", "site", "--name", " "]),
        "A repo needs a name and a path that aren't blank."
    );
    assert_eq!(
        fails(&db, &["repo", "edit", "site"]),
        "Give --name, --path, --remote or --no-remote."
    );
    assert_eq!(
        fails(&db, &["repo", "edit", "nope", "--name", "x"]),
        "There's no repo named \"nope\". Registered repos: api, site."
    );
    assert!(
        !run(
            &db,
            &["repo", "edit", "site", "--remote", "a", "--no-remote"]
        )
        .status
        .success()
    );
}

#[test]
fn repo_rm_unlinks_its_tasks_then_removes_it() {
    let dir = TempDir::new();
    let db = dir.db();
    ok(&db, &["repo", "add", "web", "/src/web"]);
    ok(&db, &["repo", "add", "api", "/src/api"]);
    ok(&db, &["add", "One", "--repo", "web", "--repo", "api"]);
    ok(&db, &["add", "Two", "--repo", "web"]);
    assert_eq!(
        ok(&db, &["repo", "rm", "web"]),
        "Removed repo web, and unlinked it from #1, #2\n"
    );
    assert_eq!(ok(&db, &["repo", "ls"]), "api  /src/api\n");
    assert_eq!(
        json(&db, &["show", "1"])["task"]["repos"],
        serde_json::json!(["api"])
    );
    let log = json(&db, &["log"]);
    let last = &log.as_array().unwrap()[log.as_array().unwrap().len() - 1];
    assert_eq!(
        (last["kind"].as_str(), last["body"].as_str()),
        (Some("repo_unlinked"), Some("web"))
    );

    let v = json(&db, &["repo", "rm", "api"]);
    assert_eq!(v["repo"]["name"], "api");
    assert_eq!(v["unlinked"], serde_json::json!([1]));
    ok(&db, &["repo", "add", "ios", "/src/ios"]);
    assert_eq!(
        ok(&db, &["repo", "rm", "ios"]),
        "Removed repo ios; no task had it\n"
    );
    assert_eq!(
        fails(&db, &["repo", "rm", "ios"]),
        "There's no repo named \"ios\". No repos are registered yet."
    );
}

#[test]
fn branch_sets_and_clears() {
    let dir = TempDir::new();
    let db = dir.db();
    ok(&db, &["add", "Task"]);
    assert_eq!(
        ok(&db, &["branch", "1", "booking-flow"]),
        "○ #1 Task is on branch booking-flow\n"
    );
    assert_eq!(json(&db, &["show", "1"])["task"]["branch"], "booking-flow");
    assert_eq!(
        ok(&db, &["branch", "1", "--clear"]),
        "○ #1 Task has no branch now\n"
    );
    assert!(!run(&db, &["branch", "1"]).status.success());
}

#[test]
fn a_branch_name_git_would_misread_is_refused_plainly() {
    let dir = TempDir::new();
    let db = dir.db();
    ok(&db, &["add", "Task"]);
    for bad in ["a..b", "feature/", "x.lock", "has space", "/abs"] {
        let err = fails(&db, &["branch", "1", bad]);
        assert!(err.contains("A branch name uses only"), "{bad}: {err}");
    }
    assert!(json(&db, &["show", "1"])["task"]["branch"].is_null());
}

#[test]
fn link_refuses_a_malformed_pr_ref_plainly() {
    let dir = TempDir::new();
    let db = dir.db();
    ok(&db, &["add", "Code"]);
    let err = fails(&db, &["link", "1", "pr", "web#42"]);
    assert!(err.contains("owner/repo#42"), "{err}");
    assert_eq!(
        ok(
            &db,
            &["link", "1", "pr", "https://github.com/Acme/Web/pull/42"]
        ),
        "Linked pr acme/web#42 to ○ #1 Code\n"
    );
}

#[test]
fn every_write_is_recorded_as_me() {
    let dir = TempDir::new();
    let db = dir.db();
    ok(&db, &["add", "Task"]);
    ok(&db, &["set", "1", "doing"]);
    let log = json(&db, &["log"]);
    for e in log.as_array().unwrap() {
        assert_eq!(e["actor"], "me");
    }
}

#[test]
fn a_database_from_a_newer_airboss_is_refused_with_what_to_do() {
    let dir = TempDir::new();
    let db = dir.db();
    ok(&db, &["add", "Task"]);
    as_if_a_newer_airboss_migrated(&db);
    assert_eq!(fails(&db, &["ls"]), NEWER_DATABASE);
    assert_eq!(fails(&db, &["add", "Another"]), NEWER_DATABASE);
}

#[test]
fn with_no_home_and_no_airboss_db_it_says_what_to_set() {
    let dir = TempDir::new();
    let out = common::airboss(&dir.db())
        .env_remove("AIRBOSS_DB")
        .env_remove("HOME")
        .current_dir(&dir.0)
        .arg("ls")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stderr(&out),
        "airboss: HOME isn't set, so there's no default database; set AIRBOSS_DB\n"
    );
    assert_eq!(
        std::fs::read_dir(&dir.0).unwrap().count(),
        0,
        "no file anywhere"
    );
}
