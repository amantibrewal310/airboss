//! A `Launch` on an in-memory `core`, with fake `t3`, `git`, sync and clock, and a temp file
//! for launch's own tables. Nothing here runs `t3` or touches `~/.t3`.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

pub mod temp;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use airboss_core::{Actor, NewTask, Repo, RepoId, Store, Task, TaskId};
use airboss_launch::fake::{FakeGit, FakeT3Cli};
use airboss_launch::{CoreAccess, Launch, LaunchConfig, LaunchTools, LaunchWorker};
use airboss_services::fakes::{FakeClock, FakeSync};
use airboss_services::{Mode, Place, SharedStore, Source, StartRequest, ThreadKey};
use rusqlite::Connection;

pub use temp::TempDir;

/// `(repo, mode, state, thread, branch)` of one `launch_threads` row.
pub type RowView = (Option<i64>, String, String, Option<String>, Option<String>);

/// 26 Nov 2026, 10:00 UTC.
pub fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_795_687_200)
}

pub struct H {
    pub dir: TempDir,
    pub store: SharedStore,
    pub core: CoreAccess,
    pub t3: FakeT3Cli,
    pub git: FakeGit,
    pub sync: Arc<FakeSync>,
    pub clock: Arc<FakeClock>,
    pub cfg: LaunchConfig,
    pub launch: Arc<Launch>,
    pub worker: LaunchWorker,
}

impl H {
    pub fn new() -> H {
        let dir = TempDir::new();
        let store: SharedStore = Arc::new(Mutex::new(Store::open_in_memory().unwrap()));
        let core = CoreAccess::new(store.clone());
        let t3 = FakeT3Cli::new();
        let git = FakeGit::new();
        let sync = Arc::new(FakeSync::new());
        let clock = Arc::new(FakeClock::at(t0()));
        let home = dir.dir("home");
        let mut cfg = LaunchConfig::defaults(&home);
        // A file that exists, so availability can be checked; the fake answers for it.
        cfg.t3_path = dir.path().join("t3-never-run");
        std::fs::write(&cfg.t3_path, "not t3").unwrap();
        cfg.research_dir = dir.dir("research");
        cfg.db_path = Some(dir.path().join("launch.db"));
        let (launch, worker) = open(&cfg, &core, &t3, &git, &sync, &clock);
        H {
            dir,
            store,
            core,
            t3,
            git,
            sync,
            clock,
            cfg,
            launch,
            worker,
        }
    }

    /// Airboss quit and opened again: a new `Launch` on the same store and table file.
    pub fn restart(&mut self) {
        let (launch, worker) = open(
            &self.cfg,
            &self.core,
            &self.t3,
            &self.git,
            &self.sync,
            &self.clock,
        );
        self.launch = launch;
        self.worker = worker;
    }

    /// A registered repo, in a real folder.
    pub fn repo(&self, name: &str) -> Repo {
        let path = self.dir.dir(&format!("src/{name}"));
        self.store
            .lock()
            .unwrap()
            .add_repo(name, &path, None, &Actor::Me)
            .unwrap()
    }

    pub fn task(&self, title: &str, repos: &[&Repo]) -> Task {
        let mut new = NewTask::new(title);
        for r in repos {
            new = new.in_repo(r.id);
        }
        self.store
            .lock()
            .unwrap()
            .add_task(new, &Actor::Me)
            .unwrap()
    }

    pub fn get(&self, id: TaskId) -> Task {
        self.store.lock().unwrap().task(id).unwrap()
    }

    pub fn db(&self) -> Connection {
        Connection::open(self.cfg.db_path.as_ref().unwrap()).unwrap()
    }

    /// `(repo, mode, state, thread, branch)` for every row of a task, oldest first.
    pub fn rows(&self, task: TaskId) -> Vec<RowView> {
        rows_at(self.cfg.db_path.as_ref().unwrap(), task)
    }

    pub fn path_of(&self, repo: &Repo) -> PathBuf {
        PathBuf::from(&repo.path)
    }
}

pub fn rows_at(db: &std::path::Path, task: TaskId) -> Vec<RowView> {
    let c = Connection::open(db).unwrap();
    let mut s = c
        .prepare(
            "SELECT repo_id, mode, state, thread_id, branch FROM launch_threads
             WHERE task_id = ?1 ORDER BY id",
        )
        .unwrap();
    s.query_map([task.0], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

fn open(
    cfg: &LaunchConfig,
    core: &CoreAccess,
    t3: &FakeT3Cli,
    git: &FakeGit,
    sync: &Arc<FakeSync>,
    clock: &Arc<FakeClock>,
) -> (Arc<Launch>, LaunchWorker) {
    airboss_launch::open(
        cfg.clone(),
        core.clone(),
        sync.clone(),
        LaunchTools {
            t3: Box::new(t3.clone()),
            git: Box::new(git.clone()),
            clock: clock.clone(),
        },
    )
    .unwrap()
}

pub fn do_it(task: TaskId) -> StartRequest {
    StartRequest {
        task,
        mode: Mode::DoIt,
        place: Place::TaskRepos,
        again: false,
        launcher: airboss_services::Launcher::T3,
    }
}

pub fn plan_it(task: TaskId) -> StartRequest {
    StartRequest {
        task,
        mode: Mode::PlanIt,
        place: Place::TaskRepos,
        again: false,
        launcher: airboss_services::Launcher::T3,
    }
}

pub fn again(mut req: StartRequest) -> StartRequest {
    req.again = true;
    req
}

pub fn t3_key(id: &str) -> ThreadKey {
    ThreadKey {
        source: Source::T3,
        r#ref: id.to_string(),
    }
}

pub fn ids(repos: &[&Repo]) -> Vec<RepoId> {
    repos.iter().map(|r| r.id).collect()
}
