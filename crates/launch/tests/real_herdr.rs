//! The real Herdr and the real `git`, by hand only:
//!
//! ```sh
//! cargo test -p airboss-launch --test real_herdr -- --ignored --test-threads 1
//! ```
//!
//! `real_herdr_spaces_tabs_and_labels` needs Herdr open. It works only in a space of its own,
//! `airboss-test`, opened with `focus: false`, runs `sleep` (never an agent), and closes that
//! space (by the id it was given) when it's done, which ends its terminals. It touches no
//! other space, tab or terminal. `real_git_worktrees` runs `git` on a repo it makes in a temp
//! dir.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use airboss_core::TaskId;
use airboss_launch::herdr::{self, AppRun, Tool};
use airboss_launch::t3cli::Answer;
use airboss_launch::{Git, HerdrCli, RealGit, RealHerdr};
use airboss_services::Availability;
use common::TempDir;
use serde_json::{Value, json};

const SPACE: &str = "airboss-test";
const T: Duration = Duration::from_secs(30);

fn home() -> std::path::PathBuf {
    std::env::var_os("HOME").map(Into::into).unwrap()
}

/// Closes the test's own space, by id, even when an assertion fails.
struct CloseSpace<'a> {
    cli: &'a mut RealHerdr,
    id: Option<String>,
}

impl Drop for CloseSpace<'_> {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            let args = json!({"id": id, "confirmed": true}).to_string();
            let out = self
                .cli
                .run(Tool::App, &["do".into(), "space.close".into(), args], T);
            eprintln!("space.close {id}: {out:?}");
        }
    }
}

#[test]
#[ignore = "needs Herdr open; opens and closes its own airboss-test space"]
fn real_herdr_spaces_tabs_and_labels() {
    let dir = TempDir::new();
    let folder = dir.dir("a");
    let other = dir.dir("b");
    let mut cli = RealHerdr::new(&home());
    assert!(matches!(
        herdr::availability(&mut cli),
        Availability::Ready { .. }
    ));
    let before = match herdr::run_app(&mut cli, &herdr::ls_argv(), T) {
        AppRun::Ran(out) => herdr::read_ls(&out.stdout).unwrap(),
        other => panic!("{other:?}"),
    };
    assert!(
        herdr::find_space(&before, SPACE, &folder).is_none(),
        "close the old {SPACE} space first"
    );
    let program = vec!["sh".to_string(), "-c".to_string(), "sleep 60".to_string()];
    let task = "line 1\n\"quoted\" $HOME `x` --flag";

    // A new space as Do it makes it for a repo with none: in the repo's folder, with a
    // shell tab named after it.
    let made = std::time::SystemTime::now();
    let argv = herdr::space_shell_argv(SPACE, &folder.display().to_string(), SPACE);
    let out = match herdr::run_app(&mut cli, &argv, T) {
        AppRun::Ran(out) => out,
        other => panic!("{other:?}"),
    };
    let guard = CloseSpace {
        id: herdr::read_space_id(&out),
        cli: &mut cli,
    };
    assert!(guard.id.is_some(), "{out:?}");
    let shell = match herdr::read_opened(AppRun::Ran(out)) {
        Answer::Started { thread_id, .. } => thread_id,
        other => panic!("{other:?}"),
    };
    // Then the agent's tab, in another folder, as if its answer was lost: it's the one
    // unlabelled terminal started there since.
    let space = guard.id.clone().unwrap();
    let argv = herdr::tab_new_argv(
        &space,
        "probe 1",
        &other.display().to_string(),
        &program,
        task,
    );
    let first = match herdr::read_opened(herdr::run_app(guard.cli, &argv, T)) {
        Answer::Started { thread_id, .. } => thread_id,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        herdr::unlabelled_in(guard.cli, &other, made),
        Some(vec![first.clone()])
    );
    assert_eq!(
        herdr::unlabelled_in(guard.cli, &folder, made),
        Some(vec![shell.clone()])
    );

    // The space is found by name, and a second tab goes in it.
    let spaces = match herdr::run_app(guard.cli, &herdr::ls_argv(), T) {
        AppRun::Ran(out) => herdr::read_ls(&out.stdout).unwrap(),
        other => panic!("{other:?}"),
    };
    let space = herdr::find_space(&spaces, SPACE, Path::new("/nowhere")).unwrap();
    assert_eq!(Some(&space.id), guard.id.as_ref());
    assert!(herdr::same_path(
        Path::new(space.folder.as_deref().unwrap()),
        &folder
    ));
    let argv = herdr::tab_new_argv(
        &space.id,
        "probe 2",
        &other.display().to_string(),
        &program,
        task,
    );
    let second = match herdr::read_opened(herdr::run_app(guard.cli, &argv, T)) {
        Answer::Started { thread_id, .. } => thread_id,
        other => panic!("{other:?}"),
    };
    assert_ne!(first, second);

    // A refusal is HerdrFailed with herdr-app's own words, and makes nothing.
    let argv = herdr::tab_new_argv(
        "spnope",
        "probe 3",
        &other.display().to_string(),
        &program,
        task,
    );
    match herdr::read_opened(herdr::run_app(guard.cli, &argv, T)) {
        Answer::Failed(e) => assert_eq!(e.message, "no space \"spnope\""),
        other => panic!("{other:?}"),
    }

    // Labels, then finding the terminal by them.
    let label = guard
        .cli
        .run(Tool::Hc, &herdr::label_argv(&second, TaskId(0), 0), T)
        .unwrap();
    assert_eq!(label.status, Some(0), "{label:?}");
    let found = guard
        .cli
        .run(Tool::Hc, &herdr::find_argv(TaskId(0), 0), T)
        .unwrap();
    assert_eq!(herdr::read_found(&found), Some(Some(second.clone())));
    // Labelled, it's no longer a candidate.
    assert_eq!(
        herdr::unlabelled_in(guard.cli, &other, made),
        Some(vec![first.clone()])
    );

    // The task reached the program as its last argument, byte for byte.
    let got = guard
        .cli
        .run(
            Tool::Hc,
            &["get".into(), second.clone(), "--json".into()],
            T,
        )
        .unwrap();
    let v: Value = serde_json::from_str(got.stdout.trim()).unwrap();
    let argv: Vec<&str> = v["argv"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(argv.last(), Some(&task));
    drop(guard);

    // Closing the space ends its terminals, within a moment.
    let mut found = None;
    for _ in 0..50 {
        let out = cli
            .run(Tool::Hc, &herdr::find_argv(TaskId(0), 0), T)
            .unwrap();
        found = herdr::read_found(&out);
        if found == Some(None) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(found, Some(None));
    // Remove the two finished records this test made.
    for t in [&shell, &first, &second] {
        let rm = cli.run(Tool::Hc, &["rm".into(), t.clone()], T).unwrap();
        assert_eq!(rm.status, Some(0), "{rm:?}");
    }
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

#[test]
#[ignore = "runs the real git, on a repo in a temp dir"]
fn real_git_worktrees() {
    let dir = TempDir::new();
    let repo = dir.dir("web");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "first"]);
    let mut g = RealGit::new(dir.path());
    let wt = dir.path().join("wt/web/task-1-x");
    g.add_worktree(&repo, &wt, "task-1/x", Some("main"))
        .unwrap();
    let list = g.worktrees(&repo).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[1].path, wt);
    assert_eq!(list[1].branch.as_deref(), Some("task-1/x"));
    // The branch exists now: a second worktree on it is git's refusal, in git's words.
    let e = g
        .add_worktree(&repo, &dir.path().join("wt/web/again"), "task-1/x", None)
        .unwrap_err();
    assert!(e.to_string().contains("task-1/x"), "{e}");
    // A killed add leaves the lock `initializing`; such a worktree is removed with -f -f.
    let name = list[1]
        .path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    std::fs::write(
        repo.join(".git/worktrees").join(&name).join("locked"),
        "initializing",
    )
    .unwrap();
    let half = &g.worktrees(&repo).unwrap()[1];
    assert!(half.unfinished(), "{half:?}");
    assert!(g.has_head(&wt).unwrap());
    g.remove_worktree(&repo, &wt).unwrap();
    assert_eq!(g.worktrees(&repo).unwrap().len(), 1);
    assert!(!g.has_head(&wt).unwrap_or(false));
    g.add_worktree(&repo, &wt, "task-1/x", None).unwrap();
    // A missing base is refused too.
    let e = g
        .add_worktree(
            &repo,
            &dir.path().join("wt/web/y"),
            "y",
            Some("origin/main"),
        )
        .unwrap_err();
    assert!(!e.to_string().is_empty());
}
