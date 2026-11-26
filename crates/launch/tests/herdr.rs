//! Do it and Plan it in Herdr, on the fake `herdr-app`, `hc` and `git`: worktrees, spaces,
//! tabs, labels, opening the window, errors and settling.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use airboss_core::{LinkKind, Repo, Status, Task, TaskId};
use airboss_launch::fake::{HerdrReply, Reply};
use airboss_launch::herdr::Tool;
use airboss_launch::{HERDR_MISSING, LaunchMsg};
use airboss_services::{
    Availability, ErrorCode, HerdrAgent, LaunchService, Launcher, Mode, Place, Source,
    StartRequest, ThreadKey,
};
use common::{H, t0};

const CLAUDE: Launcher = Launcher::Herdr {
    agent: HerdrAgent::Claude,
};

fn herdr(mut req: StartRequest) -> StartRequest {
    req.launcher = CLAUDE;
    req
}

fn do_it(task: TaskId) -> StartRequest {
    herdr(common::do_it(task))
}

fn plan_it(task: TaskId) -> StartRequest {
    herdr(common::plan_it(task))
}

fn key(address: &str) -> ThreadKey {
    ThreadKey {
        source: Source::Herdr,
        r#ref: address.to_string(),
    }
}

/// The worktree Herdr's Do it uses for `repo` on `branch`, under the default directory.
fn worktree(h: &H, repo: &Repo, branch: &str) -> PathBuf {
    h.cfg
        .home
        .join(".herdr/worktrees")
        .join(Path::new(&repo.path).file_name().unwrap())
        .join(branch.replace('/', "-"))
}

/// `(id, launcher, worktree_path)` of every row of a task, oldest first.
fn herdr_rows(h: &H, task: TaskId) -> Vec<(i64, String, Option<String>)> {
    let c = h.db();
    let mut s = c
        .prepare(
            "SELECT id, launcher, worktree_path FROM launch_threads WHERE task_id = ?1 ORDER BY id",
        )
        .unwrap();
    s.query_map([task.0], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// A repo whose `main` exists, as most do.
fn repo(h: &H, name: &str) -> Repo {
    let r = h.repo(name);
    h.git.add_branch(&r.path, "main");
    r
}

fn task(h: &H, title: &str, repos: &[&Repo]) -> Task {
    h.task(title, repos)
}

fn app_commands(h: &H) -> Vec<String> {
    h.herdr
        .calls()
        .iter()
        .filter(|c| c.tool == Tool::App)
        .map(|c| c.command().map_or_else(|| c.argv.join(" "), str::to_string))
        .collect()
}

#[test]
fn do_it_in_one_repo_makes_the_worktree_and_opens_a_tab_in_a_new_space() {
    let h = H::new();
    let web = repo(&h, "web");
    let t = task(&h, "Booking flow", &[&web]);
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    let branch = format!("task-{}-booking-flow", t.id.0);
    assert_eq!(report.branch.as_deref(), Some(branch.as_str()));
    let wt = worktree(&h, &web, &branch);

    // The worktree, from the local main.
    assert!(h.git.calls().contains(&format!(
        "add_worktree({}, {}, {branch}, main)",
        web.path,
        wt.display()
    )));
    assert!(wt.is_dir());

    // No space for the repo yet: space.new makes one, named after it, without focus.
    let tabs = h.herdr.tabs();
    assert_eq!(tabs.len(), 1);
    let tab = &tabs[0];
    assert!(tab.new_space);
    assert_eq!(tab.name, format!("{} Booking flow", t.id.0));
    assert_eq!(tab.folder, wt.display().to_string());
    assert_eq!(tab.argv, ["claude"]);
    assert_eq!(tab.focus, Some(false));
    assert_eq!(h.herdr.spaces()[0].1.as_deref(), Some("web"));

    // The brief is the task: it names the worktree and Herdr's own link kind.
    assert!(
        tab.task.starts_with(&format!(
            "You're working on Airboss task {}, in web ({})",
            t.id.0,
            wt.display()
        )),
        "{}",
        tab.task
    );
    assert!(tab.task.contains("`kind: \"herdr_terminal\"`"));
    assert!(!tab.task.contains("t3_thread"));

    // Labelled, recorded, linked, and the task is doing.
    let rows = herdr_rows(&h, t.id);
    let (row, launcher, path) = &rows[0];
    assert_eq!(launcher, "herdr");
    assert_eq!(path.as_deref(), Some(wt.display().to_string().as_str()));
    assert_eq!(
        h.herdr.labels(&tab.terminal),
        [
            "airboss.launch=".to_string() + &row.to_string(),
            format!("airboss.task={}", t.id.0),
            "airboss=1".to_string()
        ]
    );
    assert_eq!(report.started.len(), 1);
    let started = &report.started[0];
    assert_eq!(started.key, key(&tab.terminal));
    assert!(started.linked);
    assert_eq!(started.repo, Some(web.id));
    let links = h.store.lock().unwrap().links(t.id).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].kind, LinkKind::HerdrTerminal);
    assert_eq!(links[0].r#ref, tab.terminal);
    assert_eq!(links[0].title.as_deref(), Some(tab.name.as_str()));
    assert!(report.moved_to_doing);
    assert_eq!(h.get(t.id).status, Status::Doing);
    assert_eq!(h.rows(t.id)[0].2, "started");
    // No T3 call at all.
    assert!(h.t3.starts().is_empty());
    // prepare reports it as a Herdr thread too.
    let prepared = h.launch.prepare(t.id, Mode::DoIt).unwrap();
    assert_eq!(prepared.started[0].key, key(&tab.terminal));
}

#[test]
fn the_repos_space_is_found_by_name_then_by_folder_on_this_computer() {
    let h = H::new();
    let web = repo(&h, "web");
    let api = repo(&h, "api");
    let db = repo(&h, "db");
    h.herdr
        .add_space("sp-remote", Some("web"), Path::new("/srv/web"), Some("vps"));
    h.herdr
        .add_space("sp-web", Some("web"), Path::new("/elsewhere"), None);
    h.herdr
        .add_space("sp-api", Some("Backend"), Path::new(&api.path), None);
    let t = task(&h, "X", &[&web, &api, &db]);
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(report.started.len(), 3, "{:?}", report.failed);
    let tabs = h.herdr.tabs();
    let in_space = |name: &str| {
        tabs.iter()
            .find(|t| t.name.ends_with(&format!(" · {name}")))
            .unwrap()
            .clone()
    };
    // Repos go by name: api, db, web.
    assert_eq!(in_space("web").space, "sp-web");
    assert!(!in_space("web").new_space);
    assert_eq!(in_space("api").space, "sp-api");
    assert!(in_space("db").new_space);
}

#[test]
fn do_it_in_two_repos_uses_one_branch_and_a_worktree_each() {
    let h = H::new();
    let web = repo(&h, "booking-web");
    let api = repo(&h, "booking-api");
    let t = task(&h, "Booking flow", &[&web, &api]);
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(report.started.len(), 2);
    let branch = report.branch.clone().unwrap();
    let tabs = h.herdr.tabs();
    // By name: booking-api first.
    assert_eq!(
        tabs[0].folder,
        worktree(&h, &api, &branch).display().to_string()
    );
    assert_eq!(
        tabs[1].folder,
        worktree(&h, &web, &branch).display().to_string()
    );
    assert_eq!(
        tabs[0].name,
        format!("{} Booking flow · booking-api", t.id.0)
    );
    assert!(
        tabs[1]
            .task
            .contains("spans 2 repos: booking-api, booking-web")
    );
    assert!(tabs[1].task.contains(&format!(
        "You're the booking-web thread ({})",
        worktree(&h, &web, &branch).display()
    )));
    // One space each, named after its repo.
    let names: Vec<_> = h.herdr.spaces().into_iter().map(|s| s.1.unwrap()).collect();
    assert_eq!(names, ["booking-api", "booking-web"]);
}

#[test]
fn an_existing_worktree_on_the_branch_is_reused() {
    let h = H::new();
    let web = repo(&h, "web");
    let t = task(&h, "X", &[&web]);
    let branch = format!("task-{}-x", t.id.0);
    let wt = worktree(&h, &web, &branch);
    std::fs::create_dir_all(&wt).unwrap();
    h.git.add_worktree_at(&web.path, &wt, Some(&branch));
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(report.started.len(), 1);
    assert!(!h.git.calls().iter().any(|c| c.starts_with("add_worktree")));
    assert_eq!(h.herdr.tabs()[0].folder, wt.display().to_string());
}

#[test]
fn a_worktree_there_on_another_branch_fails_only_that_repo() {
    let h = H::new();
    let web = repo(&h, "web");
    let api = repo(&h, "api");
    let t = task(&h, "X", &[&web, &api]);
    let branch = format!("task-{}-x", t.id.0);
    let wt = worktree(&h, &api, &branch);
    h.git
        .add_worktree_at(&api.path, &wt, Some("something-else"));
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(report.started.len(), 1);
    assert_eq!(report.started[0].repo, Some(web.id));
    let (repo, e) = &report.failed[0];
    assert_eq!(*repo, Some(api.id));
    assert_eq!(e.code, ErrorCode::WorktreeFailed);
    assert!(e.message.contains("on something-else"), "{}", e.message);
    assert_eq!(h.rows(t.id)[0].2, "failed");
}

#[test]
fn gits_refusal_is_worktree_failed_and_doesnt_stop_the_others() {
    let h = H::new();
    let web = repo(&h, "web");
    let api = repo(&h, "api");
    let t = task(&h, "X", &[&web, &api]);
    h.git
        .refuse_worktree("fatal: 'task-1-x' is already checked out at '/src/api'");
    let report = h.launch.start(&do_it(t.id)).unwrap();
    let (repo, e) = &report.failed[0];
    assert_eq!(*repo, Some(api.id));
    assert_eq!(e.code, ErrorCode::WorktreeFailed);
    assert_eq!(
        e.message,
        "'task-1-x' is already checked out at '/src/api'."
    );
    assert_eq!(report.started.len(), 1);
    assert_eq!(h.herdr.tabs().len(), 1, "no tab for the failed repo");
}

#[test]
fn a_base_only_on_origin_is_used_from_origin() {
    let h = H::new();
    let web = h.repo("web");
    let t = task(&h, "X", &[&web]);
    h.launch.start(&do_it(t.id)).unwrap();
    let add = h
        .git
        .calls()
        .into_iter()
        .find(|c| c.starts_with("add_worktree"))
        .unwrap();
    assert!(add.ends_with(", origin/main)"), "{add}");
}

#[test]
fn a_retry_after_a_herdr_error_reuses_the_worktree() {
    let h = H::new();
    let web = repo(&h, "web");
    let t = task(&h, "X", &[&web]);
    h.herdr.push(HerdrReply::Error("no space \"sp9\"".into()));
    let report = h.launch.start(&do_it(t.id)).unwrap();
    let e = &report.failed[0].1;
    assert_eq!(e.code, ErrorCode::HerdrFailed);
    assert_eq!(e.message, "no space \"sp9\"");
    assert_eq!(h.rows(t.id)[0].2, "failed");
    assert_eq!(h.get(t.id).status, Status::Todo);

    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(report.started.len(), 1);
    let adds = h
        .git
        .calls()
        .iter()
        .filter(|c| c.starts_with("add_worktree"))
        .count();
    assert_eq!(adds, 1, "the second start reused the first's worktree");
}

#[test]
fn start_again_uses_the_again_branch_and_its_own_worktree() {
    let h = H::new();
    let web = repo(&h, "web");
    let t = task(&h, "X", &[&web]);
    h.launch.start(&do_it(t.id)).unwrap();
    // A second press without again skips.
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(report.skipped, [web.id]);
    assert_eq!(h.herdr.tabs().len(), 1);

    let report = h.launch.start(&common::again(do_it(t.id))).unwrap();
    assert_eq!(report.started.len(), 1);
    let branch = format!("task-{}-x", t.id.0);
    let again = format!("{branch}-2");
    let wt = worktree(&h, &web, &again);
    assert!(h.git.calls().contains(&format!(
        "add_worktree({}, {}, {again}, {branch})",
        web.path,
        wt.display()
    )));
    let tabs = h.herdr.tabs();
    assert_eq!(tabs[1].folder, wt.display().to_string());
    // The second tab goes in the space the first made.
    assert!(!tabs[1].new_space);
    assert_eq!(tabs[1].space, tabs[0].space);
    assert_eq!(h.get(t.id).branch.as_deref(), Some(branch.as_str()));
}

#[test]
fn a_t3_thread_already_started_here_is_skipped() {
    let h = H::new();
    let web = repo(&h, "web");
    let t = task(&h, "X", &[&web]);
    h.launch.start(&common::do_it(t.id)).unwrap();
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(report.skipped, [web.id]);
    assert!(h.herdr.tabs().is_empty());
}

#[test]
fn plan_it_opens_one_tab_in_the_first_repo_with_no_worktree() {
    let h = H::new();
    let web = repo(&h, "web");
    let api = repo(&h, "api");
    let t = task(&h, "Big thing", &[&web, &api]);
    let report = h.launch.start(&plan_it(t.id)).unwrap();
    assert_eq!(report.started.len(), 1);
    assert_eq!(report.branch, None);
    let tabs = h.herdr.tabs();
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].name, format!("Plan {} Big thing", t.id.0));
    assert_eq!(tabs[0].folder, api.path);
    assert!(tabs[0].task.starts_with("Plan Airboss task"));
    assert!(tabs[0].task.contains("`kind: \"herdr_terminal\"`"));
    assert!(!h.git.calls().iter().any(|c| c.contains("worktree")));
    assert_eq!(herdr_rows(&h, t.id)[0].2, None);
    assert_eq!(h.get(t.id).status, Status::Doing);
}

#[test]
fn research_only_opens_one_tab_in_the_folder() {
    let h = H::new();
    let t = task(&h, "Read up", &[]);
    let dir = h.dir.dir("notes");
    let req = StartRequest {
        place: Place::NoRepo(dir.clone()),
        ..do_it(t.id)
    };
    let report = h.launch.start(&req).unwrap();
    assert_eq!(report.started.len(), 1);
    assert_eq!(report.started[0].repo, None);
    let tab = &h.herdr.tabs()[0];
    assert_eq!(tab.folder, dir.display().to_string());
    assert!(tab.task.contains("No repo: this is research or writing."));
    assert!(tab.task.contains("`kind: \"herdr_terminal\"`"));
    // Its space is named after the folder.
    assert_eq!(h.herdr.spaces()[0].1.as_deref(), Some("notes"));
}

#[test]
fn codex_runs_without_its_daemon() {
    let h = H::new();
    let web = repo(&h, "web");
    let t = task(&h, "X", &[&web]);
    let req = StartRequest {
        launcher: Launcher::Herdr {
            agent: HerdrAgent::Codex,
        },
        ..common::do_it(t.id)
    };
    h.launch.start(&req).unwrap();
    assert_eq!(h.herdr.tabs()[0].argv, ["codex", "--no-daemon"]);
}

#[test]
fn herdrs_worktree_directory_setting_is_used() {
    let h = H::new();
    let web = repo(&h, "web");
    let t = task(&h, "X", &[&web]);
    let settings = &h.cfg.herdr_settings;
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(settings, r#"{"worktrees":{"directory":"~/Developer/wt"}}"#).unwrap();
    h.launch.start(&do_it(t.id)).unwrap();
    let expected = h
        .cfg
        .home
        .join("Developer/wt/web")
        .join(format!("task-{}-x", t.id.0));
    assert_eq!(h.herdr.tabs()[0].folder, expected.display().to_string());
}

#[test]
fn with_the_window_closed_herdr_is_opened_and_the_tab_still_starts() {
    let h = H::new();
    let web = repo(&h, "web");
    let t = task(&h, "X", &[&web]);
    h.herdr.close_window();
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(report.started.len(), 1, "{:?}", report.failed);
    let opens: Vec<_> = h
        .herdr
        .calls()
        .into_iter()
        .filter(|c| c.tool == Tool::Open)
        .collect();
    assert_eq!(opens.len(), 1);
    assert_eq!(opens[0].argv, ["-g", "-b", "dev.herdr.app"]);
    // ls failed, status failed, open, status until it answered, then ls again.
    assert_eq!(
        app_commands(&h)[..6],
        ["ls", "status", "status", "status", "ls", "space.new"]
    );
    assert_eq!(h.herdr.pauses(), [Duration::from_millis(500)]);
}

#[test]
fn herdr_not_opening_stops_the_loop() {
    let h = H::new();
    let web = repo(&h, "web");
    let api = repo(&h, "api");
    let t = task(&h, "X", &[&web, &api]);
    h.herdr.never_opens();
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert!(report.started.is_empty());
    assert_eq!(report.failed.len(), 2);
    for (_, e) in &report.failed {
        assert_eq!(e.code, ErrorCode::HerdrNotRunning);
        assert_eq!(e.message, "Herdr didn't open. Open it and try again.");
    }
    // One try at opening, about 10 s of waiting, then the rest fail without asking again.
    let opens = h
        .herdr
        .calls()
        .iter()
        .filter(|c| c.tool == Tool::Open)
        .count();
    assert_eq!(opens, 1);
    assert_eq!(h.herdr.pauses().len(), 20);
    assert_eq!(h.rows(t.id).len(), 1);
    assert_eq!(h.rows(t.id)[0].2, "failed");
    assert_eq!(h.get(t.id).status, Status::Todo);
}

#[test]
fn herdr_missing_stops_the_loop_and_rechecks() {
    let mut h = H::new();
    let web = repo(&h, "web");
    let api = repo(&h, "api");
    let t = task(&h, "X", &[&web, &api]);
    h.herdr.uninstall();
    while h.worker.try_next().is_some() {}
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(report.failed.len(), 2);
    for (_, e) in &report.failed {
        assert_eq!(e.code, ErrorCode::HerdrMissing);
        assert_eq!(e.message, HERDR_MISSING);
    }
    assert_eq!(h.worker.try_next(), Some(LaunchMsg::CheckAvailability));
}

#[test]
fn a_tab_that_may_exist_leaves_the_row_pending_and_stops_the_loop() {
    let h = H::new();
    let web = repo(&h, "web");
    let api = repo(&h, "api");
    let t = task(&h, "X", &[&web, &api]);
    h.herdr.push(HerdrReply::Timeout);
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(report.failed.len(), 2);
    let e = &report.failed[0].1;
    assert_eq!(e.code, ErrorCode::Timeout);
    assert!(
        e.message.starts_with("Herdr didn't answer in 30 s."),
        "{}",
        e.message
    );
    assert_eq!(h.rows(t.id)[0].2, "pending");
    // A second press says it's still starting, naming Herdr.
    let report = h.launch.start(&do_it(t.id)).unwrap();
    let still = report
        .failed
        .iter()
        .find(|(r, _)| *r == Some(api.id))
        .unwrap();
    assert_eq!(still.1.code, ErrorCode::AlreadyStarting);
    assert_eq!(
        still.1.message,
        "api is still starting. Check Herdr, or choose Start again."
    );
    // An answer that isn't the ids is unsure too.
    let h = H::new();
    let web = repo(&h, "web");
    let t = task(&h, "X", &[&web]);
    h.herdr.push(HerdrReply::Stdout("{}".into()));
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(report.failed[0].1.code, ErrorCode::BadAnswer);
    assert_eq!(h.rows(t.id)[0].2, "pending");
}

#[test]
fn the_pending_row_is_committed_before_the_tab_opens() {
    let h = H::new();
    let web = repo(&h, "web");
    let t = task(&h, "X", &[&web]);
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let (db, out) = (h.cfg.db_path.clone().unwrap(), seen.clone());
    let task_id = t.id;
    h.herdr.on_call(move |call| {
        if call.command().is_some() {
            let rows = common::rows_at(&db, task_id);
            out.lock().unwrap().push(rows[0].2.clone());
        }
    });
    h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(*seen.lock().unwrap(), ["pending"]);
}

#[test]
fn a_label_that_fails_still_starts_and_links() {
    let h = H::new();
    let web = repo(&h, "web");
    let t = task(&h, "X", &[&web]);
    h.herdr.fail_labels();
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(report.started.len(), 1);
    assert!(report.started[0].linked);
}

#[test]
fn a_pending_row_settles_by_its_labels() {
    let mut h = H::new();
    let web = repo(&h, "web");
    let t = task(&h, "X", &[&web]);
    h.herdr.push(HerdrReply::Timeout);
    h.launch.start(&do_it(t.id)).unwrap();
    let row = herdr_rows(&h, t.id)[0].0;
    // A terminal for another row, and one for this row whose program ended, don't count.
    h.herdr.add_terminal(
        "t8",
        &[
            ("airboss", "1"),
            ("airboss.task", &t.id.0.to_string()),
            ("airboss.launch", "999"),
        ],
    );
    let labels = [
        ("airboss", "1"),
        ("airboss.task", &t.id.0.to_string()),
        ("airboss.launch", &row.to_string()),
    ];
    h.herdr.add_terminal("t7", &labels);
    h.herdr.finish("t7");
    let step = h.worker.handle(LaunchMsg::Settle, t0());
    assert!(!step.changed);
    assert_eq!(step.next_wake, t0() + Duration::from_secs(10));
    // Airboss quit mid-start; on the next run the tab is found.
    h.herdr.add_terminal("t9", &labels);
    h.restart();
    while let Some(msg) = h.worker.try_next() {
        h.worker.handle(msg, t0() + Duration::from_secs(5));
    }
    assert_eq!(h.rows(t.id)[0].2, "started");
    assert_eq!(h.rows(t.id)[0].3.as_deref(), Some("t9"));
    let links = h.store.lock().unwrap().links(t.id).unwrap();
    assert_eq!(links[0].kind, LinkKind::HerdrTerminal);
    assert_eq!(links[0].r#ref, "t9");
    assert_eq!(h.get(t.id).status, Status::Doing);
    let prepared = h.launch.prepare(t.id, Mode::DoIt).unwrap();
    assert_eq!(prepared.started[0].key, key("t9"));
}

#[test]
fn a_pending_row_with_no_terminal_is_lost_after_2_minutes() {
    let mut h = H::new();
    let t = task(&h, "X", &[]);
    let req = StartRequest {
        place: Place::NoRepo(h.dir.dir("notes")),
        ..plan_it(t.id)
    };
    h.herdr.push(HerdrReply::Timeout);
    h.launch.start(&req).unwrap();
    // Herdr rows are looked up at 10 s, 30 s, 60 s and 2 minutes, Plan it ones too.
    let mut now = t0();
    let mut wakes = Vec::new();
    for _ in 0..4 {
        let step = h.worker.handle(LaunchMsg::Tick, now);
        wakes.push(step.next_wake.duration_since(t0()).unwrap().as_secs());
        now = step.next_wake;
    }
    assert_eq!(wakes, [10, 30, 60, 120]);
    let step = h.worker.handle(LaunchMsg::Tick, now);
    assert!(step.changed);
    assert_eq!(h.rows(t.id)[0].2, "lost");
    // Then a new press starts again.
    let report = h.launch.start(&req).unwrap();
    assert_eq!(report.started.len(), 1);
}

#[test]
fn while_hc_cant_answer_a_pending_row_stays_pending() {
    let mut h = H::new();
    let web = repo(&h, "web");
    let t = task(&h, "X", &[&web]);
    h.herdr.push(HerdrReply::Timeout);
    h.launch.start(&do_it(t.id)).unwrap();
    h.herdr.break_hc();
    h.worker
        .handle(LaunchMsg::Tick, t0() + Duration::from_secs(600));
    assert_eq!(h.rows(t.id)[0].2, "pending");
}

#[test]
fn t3_rows_still_settle_through_sync() {
    let mut h = H::new();
    let web = repo(&h, "web");
    let t = task(&h, "X", &[&web]);
    h.t3.push(Reply::Timeout);
    h.launch.start(&common::do_it(t.id)).unwrap();
    h.herdr.add_terminal(
        "t1",
        &[
            ("airboss", "1"),
            ("airboss.task", &t.id.0.to_string()),
            ("airboss.launch", "1"),
        ],
    );
    h.worker.handle(LaunchMsg::Settle, t0());
    // A T3 row is never matched against Herdr's labels.
    assert_eq!(h.rows(t.id)[0].2, "pending");
    assert!(
        !h.herdr
            .calls()
            .iter()
            .any(|c| c.argv.first().is_some_and(|a| a == "ls") && c.tool == Tool::Hc)
    );
}

#[test]
fn availability_needs_hc_and_herdr_app() {
    let mut h = H::new();
    assert_eq!(h.launch.herdr_availability(), Availability::Unknown);
    h.worker.handle(LaunchMsg::CheckAvailability, t0());
    assert_eq!(
        h.launch.herdr_availability(),
        Availability::Ready {
            version: "0.1.18".into()
        }
    );
    h.herdr.without_hc();
    let step = h.worker.handle(LaunchMsg::CheckAvailability, t0());
    assert!(step.changed);
    assert_eq!(
        h.launch.herdr_availability(),
        Availability::Missing {
            reason: HERDR_MISSING.into()
        }
    );
    h.herdr.uninstall();
    h.worker.handle(LaunchMsg::CheckAvailability, t0());
    assert!(matches!(
        h.launch.herdr_availability(),
        Availability::Missing { .. }
    ));
    // T3's availability is its own.
    assert!(matches!(
        h.launch.availability(),
        Availability::Ready { .. }
    ));
}

#[test]
fn briefs_for_herdr_are_the_agent_and_its_first_message() {
    let h = H::new();
    let web = repo(&h, "web");
    let t = task(&h, "X", &[&web]);
    let briefs = airboss_launch::briefs(&h.launch, &do_it(t.id)).unwrap();
    assert_eq!(briefs.len(), 1);
    assert_eq!(briefs[0].argv, ["claude"]);
    assert!(briefs[0].stdin.contains("herdr_terminal"));
    // A preview makes nothing.
    assert!(h.herdr.calls().is_empty());
    assert!(!h.git.calls().iter().any(|c| c.contains("worktree")));
}

#[test]
fn the_real_runner_finds_local_bin_then_path_and_drops_thread_variables() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    use airboss_launch::{HerdrCli, RealHerdr};

    let dir = common::TempDir::new();
    let home = dir.dir("home");
    let bin = dir.dir("path-bin");
    // Stand-ins that only print what they got: never the real Herdr.
    let script = "#!/bin/sh\nprintf '%s\\n' \"$0\" \"$@\"; env\n";
    for path in [home.join(".local/bin/herdr-app"), bin.join("hc")] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let env: Vec<(OsString, OsString)> = [
        ("PATH", format!("relative:{}:/usr/bin:/bin", bin.display())),
        ("HC_TERMINAL_ID", "t1".into()),
        ("T3CODE_THREAD_ID", "x".into()),
        ("KEEP", "yes".into()),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect();
    let mut real = RealHerdr::with_env(&home, env);
    let task = "a b; $(rm -rf /) \"q\"";
    let out = real
        .run(
            Tool::App,
            &["do".into(), "tab.new".into(), task.into()],
            Duration::from_secs(5),
        )
        .unwrap();
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(
        lines[0],
        home.join(".local/bin/herdr-app").display().to_string()
    );
    assert_eq!(lines[1..4], ["do", "tab.new", task]);
    assert!(out.stdout.contains("KEEP=yes"));
    assert!(!out.stdout.contains("HC_TERMINAL_ID"));
    assert!(!out.stdout.contains("T3CODE_THREAD_ID"));
    let out = real
        .run(Tool::Hc, &["--version".into()], Duration::from_secs(5))
        .unwrap();
    assert!(
        out.stdout
            .starts_with(&bin.join("hc").display().to_string())
    );
    // Nothing installed is NotFound.
    let mut none = RealHerdr::with_env(&dir.dir("empty"), Vec::<(OsString, OsString)>::new());
    let e = none
        .run(Tool::App, &["--version".into()], Duration::from_secs(5))
        .unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
}
