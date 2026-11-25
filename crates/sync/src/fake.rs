//! Fakes for tests, behind the `fakes` feature: every outside source `sync` reads, a
//! settable clock (re-exported from `airboss-services`), and a self-cleaning temp dir.
//! Nothing here runs `gh`, `t3` or `ps`, or opens a real T3 or Claude file.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

pub use airboss_services::fakes::FakeClock;

/// A temp dir removed on drop, so tests need no extra crate.
#[derive(Debug)]
pub struct TempDir(PathBuf);

impl TempDir {
    /// A new, empty dir under the system temp dir.
    #[allow(clippy::expect_used)]
    pub fn new() -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "airboss-sync-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a temp dir");
        // Canonical, so paths compare equal to what the kernel and SQLite report (/private/var…).
        TempDir(std::fs::canonicalize(&path).unwrap_or(path))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Default for TempDir {
    fn default() -> TempDir {
        TempDir::new()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The environment id every [`T3Fixture`] writes.
pub const FIXTURE_ENVIRONMENT: &str = "0b8a1f9e-1d2c-4b3a-9f8e-7d6c5b4a3f21";

/// A fake T3 data folder: a real SQLite file in a temp dir with T3's real schema
/// (`tests/fixtures/t3_schema.sql`), in WAL mode as T3 keeps it, plus `environment-id`.
/// Tests write to it through [`exec`](T3Fixture::exec), as T3 would, and `T3Db` reads it.
pub struct T3Fixture {
    dir: TempDir,
    conn: rusqlite::Connection,
}

impl T3Fixture {
    /// T3's schema, no rows.
    #[allow(clippy::expect_used)]
    pub fn empty() -> T3Fixture {
        let dir = TempDir::new();
        let conn = rusqlite::Connection::open(dir.path().join("statev2.sqlite"))
            .expect("a fixture database");
        conn.pragma_update(None, "journal_mode", "WAL")
            .expect("WAL mode");
        conn.execute_batch(include_str!("../tests/fixtures/t3_schema.sql"))
            .expect("T3's schema loads");
        std::fs::write(
            dir.path().join("environment-id"),
            format!("{FIXTURE_ENVIRONMENT}\n"),
        )
        .expect("environment-id");
        T3Fixture { dir, conn }
    }

    /// T3's schema with `tests/fixtures/t3_rows.sql`: one thread per state.
    pub fn with_rows() -> T3Fixture {
        let f = T3Fixture::empty();
        f.exec(include_str!("../tests/fixtures/t3_rows.sql"));
        f
    }

    /// The fake `~/.t3/userdata`.
    pub fn dir(&self) -> &Path {
        self.dir.path()
    }

    /// Runs SQL as T3 would, committing at once.
    #[allow(clippy::expect_used)]
    pub fn exec(&self, sql: &str) {
        self.conn
            .execute_batch(sql)
            .unwrap_or_else(|e| panic!("fixture SQL failed: {e}\n{sql}"));
    }

    /// Adds a top-level thread with every required key, in `project`.
    pub fn add_thread(&self, id: &str, project: &str, updated_at: &str) {
        self.exec(&format!(
            "INSERT INTO orchestration_v2_projection_threads
               (thread_id, project_id, title, default_provider, runtime_mode, interaction_mode,
                created_at, updated_at, payload_json)
             VALUES ('{id}', '{project}', 'Thread {id}', 'claudeAgent', 'full-access', 'default',
                '{updated_at}', '{updated_at}',
                json_object('branch', 'feat/{id}', 'worktreePath', NULL,
                  'lineage', json_object('parentThreadId', NULL, 'rootThreadId', '{id}'),
                  'settledAt', NULL, 'pullRequests', json_array()))"
        ));
    }

    /// Adds a run to a thread; `started_at` goes in its payload.
    pub fn add_run(&self, thread: &str, ordinal: u32, status: &str, at: &str) {
        self.exec(&format!(
            "INSERT INTO orchestration_v2_projection_runs
               (run_id, thread_id, ordinal, provider, status, requested_at, completed_at, payload_json)
             VALUES ('{thread}-r{ordinal}', '{thread}', {ordinal}, 'claudeAgent', '{status}', '{at}',
               CASE WHEN '{status}' IN ('completed','interrupted','failed','cancelled','rolled_back')
                    THEN '{at}' END,
               json_object('startedAt', '{at}'))"
        ));
    }

    /// Adds a pending request, with its turn item unless `item_json` is `None`.
    pub fn add_request(
        &self,
        thread: &str,
        id: &str,
        kind: &str,
        at: &str,
        item_json: Option<&str>,
    ) {
        self.exec(&format!(
            "INSERT INTO orchestration_v2_projection_runtime_requests
               (runtime_request_id, thread_id, node_id, kind, status, created_at, payload_json)
             VALUES ('{id}', '{thread}', 'n-{id}', '{kind}', 'pending', '{at}', '{{}}')"
        ));
        if let Some(item) = item_json {
            let item_type = if kind == "user_input" {
                "user_input_request"
            } else {
                "approval_request"
            };
            self.add_item(thread, id, item_type, at, item);
        }
    }

    /// Adds the turn item of request `id`.
    pub fn add_item(&self, thread: &str, id: &str, item_type: &str, at: &str, json: &str) {
        self.exec(&format!(
            "INSERT INTO orchestration_v2_projection_turn_items
               (turn_item_id, thread_id, run_id, node_id, ordinal, type, status, updated_at, payload_json)
             VALUES ('i-{id}', '{thread}', NULL, 'n-{id}', 1, '{item_type}', 'pending', '{at}', '{json}')"
        ));
    }
}

/// Claude sessions from real files in a temp folder, with liveness set by the test instead
/// of `ps`. Clones share one state, so a test keeps one and gives the other away.
#[derive(Clone, Default)]
pub struct FakeClaude {
    inner: std::sync::Arc<std::sync::Mutex<FakeClaudeState>>,
}

#[derive(Default)]
struct FakeClaudeState {
    dir: PathBuf,
    dead: std::collections::HashSet<u32>,
    alive_calls: u32,
}

impl FakeClaude {
    /// Sessions read from `dir`; every pid alive until [`kill`](FakeClaude::kill).
    pub fn new(dir: &Path) -> FakeClaude {
        let fake = FakeClaude::default();
        fake.state().dir = dir.to_path_buf();
        fake
    }

    fn state(&self) -> std::sync::MutexGuard<'_, FakeClaudeState> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Marks a pid's process as ended (or its pid reused).
    pub fn kill(&self, pid: u32) {
        self.state().dead.insert(pid);
    }

    /// How many times liveness was checked.
    pub fn alive_calls(&self) -> u32 {
        self.state().alive_calls
    }

    /// Writes `<pid>.json` the way Claude Code does.
    #[allow(clippy::expect_used)]
    pub fn write(&self, pid: u32, session_id: &str, status: &str, entrypoint: &str) {
        let dir = self.state().dir.clone();
        let json = serde_json::json!({
            "pid": pid, "sessionId": session_id, "cwd": "/work/web", "startedAt": 1,
            "procStart": "Wed Nov 25 17:19:03 2026", "kind": "interactive",
            "entrypoint": entrypoint, "name": format!("session {pid}"), "status": status,
            "updatedAt": 1_795_600_000_000_i64, "statusUpdatedAt": 1_795_600_001_000_i64,
        });
        std::fs::write(dir.join(format!("{pid}.json")), json.to_string()).expect("a session file");
    }
}

impl crate::claude::ClaudeSource for FakeClaude {
    fn sessions(&mut self) -> std::io::Result<Vec<crate::claude::ClaudeSession>> {
        let dir = self.state().dir.clone();
        crate::claude::read_sessions(&dir)
    }

    fn alive(&mut self, sessions: &[crate::claude::ClaudeSession]) -> std::io::Result<Vec<bool>> {
        let mut state = self.state();
        state.alive_calls += 1;
        Ok(sessions
            .iter()
            .map(|s| !state.dead.contains(&s.pid))
            .collect())
    }
}

/// A [`T3Source`](crate::t3::T3Source) over a real [`T3Db`](crate::t3::T3Db) (usually on a
/// [`T3Fixture`]), that can be told to fail and counts its reads.
pub struct FakeT3 {
    db: crate::t3::T3Db,
    state: std::sync::Arc<std::sync::Mutex<FakeT3State>>,
}

#[derive(Default)]
struct FakeT3State {
    fail: std::collections::VecDeque<crate::t3::T3Error>,
    reads: u32,
    opens: u32,
    panic: bool,
}

/// Controls a [`FakeT3`] after it was handed to a worker.
#[derive(Clone)]
pub struct FakeT3Handle(std::sync::Arc<std::sync::Mutex<FakeT3State>>);

impl FakeT3 {
    pub fn new(t3_dir: &Path) -> FakeT3 {
        FakeT3 {
            db: crate::t3::T3Db::new(t3_dir),
            state: Default::default(),
        }
    }

    pub fn handle(&self) -> FakeT3Handle {
        FakeT3Handle(self.state.clone())
    }

    fn take_failure(&self) -> Option<crate::t3::T3Error> {
        lock(&self.state).fail.pop_front()
    }
}

fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl FakeT3Handle {
    /// The next call (of any kind) fails with `e`.
    pub fn fail_next(&self, e: crate::t3::T3Error) {
        lock(&self.0).fail.push_back(e);
    }

    /// The next `read` panics, as a bug would.
    pub fn panic_next(&self) {
        lock(&self.0).panic = true;
    }

    /// How many times `read` ran.
    pub fn reads(&self) -> u32 {
        lock(&self.0).reads
    }

    /// How many times `open` ran.
    pub fn opens(&self) -> u32 {
        lock(&self.0).opens
    }
}

impl crate::t3::T3Source for FakeT3 {
    fn open(&mut self) -> Result<(), crate::t3::T3Error> {
        lock(&self.state).opens += 1;
        match self.take_failure() {
            Some(e) => Err(e),
            None => self.db.open(),
        }
    }

    fn data_version(&mut self) -> Result<i64, crate::t3::T3Error> {
        match self.take_failure() {
            Some(e) => Err(e),
            None => self.db.data_version(),
        }
    }

    fn read(&mut self, q: &crate::t3::T3Query) -> Result<crate::t3::T3Read, crate::t3::T3Error> {
        let panic = {
            let mut s = lock(&self.state);
            s.reads += 1;
            std::mem::take(&mut s.panic)
        };
        if panic {
            panic!("FakeT3: an injected panic");
        }
        match self.take_failure() {
            Some(e) => Err(e),
            None => self.db.read(q),
        }
    }

    fn find_thread(
        &mut self,
        project: &Path,
        branch: &str,
    ) -> Result<Option<String>, crate::t3::T3Error> {
        match self.take_failure() {
            Some(e) => Err(e),
            None => self.db.find_thread(project, branch),
        }
    }

    fn environment_id(&mut self) -> Result<Option<String>, crate::t3::T3Error> {
        self.db.environment_id()
    }
}

/// A watcher that records what it was asked to watch and fires only when a test says so.
#[derive(Clone, Default)]
pub struct FakeWatch(std::sync::Arc<std::sync::Mutex<FakeWatchState>>);

#[derive(Default)]
struct FakeWatchState {
    calls: Vec<Vec<crate::WatchTarget>>,
    tx: Option<std::sync::mpsc::Sender<crate::T3Msg>>,
    fail: Option<String>,
}

impl FakeWatch {
    pub fn new() -> FakeWatch {
        FakeWatch::default()
    }

    /// Every `watch` call's targets, oldest first.
    pub fn calls(&self) -> Vec<Vec<crate::WatchTarget>> {
        lock(&self.0).calls.clone()
    }

    /// Sends `msg` the way the real watcher would.
    pub fn fire(&self, msg: crate::T3Msg) {
        if let Some(tx) = &lock(&self.0).tx {
            let _ = tx.send(msg);
        }
    }

    /// Makes every later `watch` call fail with `why`.
    pub fn fail_with(&self, why: &str) {
        lock(&self.0).fail = Some(why.to_string());
    }
}

impl crate::WalWatch for FakeWatch {
    fn watch(
        &mut self,
        targets: &[crate::WatchTarget],
        tx: std::sync::mpsc::Sender<crate::T3Msg>,
    ) -> Result<(), airboss_services::ServiceError> {
        let mut s = lock(&self.0);
        s.calls.push(targets.to_vec());
        s.tx = Some(tx);
        match &s.fail {
            Some(why) => Err(airboss_services::ServiceError::new(
                airboss_services::ErrorCode::Internal,
                why.clone(),
            )),
            None => Ok(()),
        }
    }
}

/// A PR in [`FakeGh`]'s GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakePr {
    pub state: &'static str,
    pub draft: bool,
    pub review: Option<&'static str>,
    pub rollup: Option<&'static str>,
    pub author: String,
    pub head: String,
    pub title: String,
    pub additions: u32,
    pub deletions: u32,
    pub updated_at: String,
}

impl FakePr {
    /// An open PR by `author`.
    pub fn open(author: &str) -> FakePr {
        FakePr {
            state: "OPEN",
            draft: false,
            review: None,
            rollup: None,
            author: author.into(),
            head: "feat/x".into(),
            title: "A change".into(),
            additions: 10,
            deletions: 2,
            updated_at: "2026-11-26T11:00:00Z".into(),
        }
    }
}

/// How the next `gh` call for an account fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeFailure {
    /// Exit 4: signed out.
    SignedOut,
    /// Exit 1 with no data, and this on stderr.
    Exit(String),
    /// Killed after the timeout.
    Timeout,
    /// GitHub's secondary rate limit.
    Secondary,
}

/// One call [`FakeGh`] answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeGhCall {
    /// "auth_status", "graphql" or "token".
    pub kind: &'static str,
    /// The account the call ran as.
    pub account: Option<String>,
    /// The PRs a GraphQL call asked for, in order.
    pub prs: Vec<String>,
    pub viewer: bool,
    /// Whether a token was handed to the child.
    pub token_set: bool,
    /// The argv `gh` would have been run with.
    pub argv: Vec<String>,
}

/// A GitHub world `gh` answers from: accounts, the repos each can see, PRs, budgets, and
/// scripted failures. It answers by call kind and arguments (account, PRs), never by exact
/// argv, and records every call. Clones share one world.
#[derive(Clone, Default)]
pub struct FakeGh(std::sync::Arc<std::sync::Mutex<FakeGhWorld>>);

#[derive(Default)]
struct FakeGhWorld {
    missing: bool,
    auth_broken: bool,
    accounts: Vec<(String, bool, bool)>,
    visible: std::collections::HashMap<String, std::collections::HashSet<String>>,
    prs: std::collections::BTreeMap<String, FakePr>,
    remaining: std::collections::HashMap<String, (u64, String)>,
    failures: std::collections::HashMap<String, std::collections::VecDeque<FakeFailure>>,
    calls: Vec<FakeGhCall>,
}

/// The token [`FakeGh`] hands out for `login`.
pub fn fake_token(login: &str) -> String {
    format!("gho_fake_secret_for_{login}")
}

impl FakeGh {
    /// `gh` installed, no account signed in.
    pub fn new() -> FakeGh {
        FakeGh::default()
    }

    fn world(&self) -> std::sync::MutexGuard<'_, FakeGhWorld> {
        lock(&self.0)
    }

    /// No `gh` binary at all.
    pub fn missing(self) -> FakeGh {
        self.world().missing = true;
        self
    }

    /// A signed-in account; the first one added with `active` is the active one.
    pub fn account(self, login: &str, active: bool) -> FakeGh {
        self.world().accounts.push((login.into(), active, true));
        self
    }

    /// An account whose `state` isn't `success`.
    pub fn broken_account(self, login: &str) -> FakeGh {
        self.world().accounts.push((login.into(), false, false));
        self
    }

    /// Makes `gh auth status` print something that isn't its JSON.
    pub fn break_auth_status(&self) {
        self.world().auth_broken = true;
    }

    /// Signs every account out (as if `gh auth logout` ran everywhere).
    pub fn sign_out_all(&self) {
        self.world().accounts.clear();
    }

    /// Lets `login` see `owner/repo`.
    pub fn sees(self, login: &str, repo: &str) -> FakeGh {
        self.world()
            .visible
            .entry(login.into())
            .or_default()
            .insert(repo.to_ascii_lowercase());
        self
    }

    /// Puts a PR on GitHub, or replaces it.
    pub fn pr(self, r: &str, pr: FakePr) -> FakeGh {
        self.set_pr(r, pr);
        self
    }

    pub fn set_pr(&self, r: &str, pr: FakePr) {
        self.world().prs.insert(r.to_ascii_lowercase(), pr);
    }

    /// Changes a PR's state: "OPEN", "MERGED" or "CLOSED".
    pub fn set_state(&self, r: &str, state: &'static str) {
        if let Some(pr) = self.world().prs.get_mut(&r.to_ascii_lowercase()) {
            pr.state = state;
        }
    }

    /// What `rateLimit.remaining` says for `login`, and when it resets.
    pub fn budget(&self, login: &str, remaining: u64, reset_at: &str) {
        self.world()
            .remaining
            .insert(login.into(), (remaining, reset_at.into()));
    }

    /// The next call as `login` fails this way.
    pub fn fail_next(&self, login: &str, f: FakeFailure) {
        self.world()
            .failures
            .entry(login.into())
            .or_default()
            .push_back(f);
    }

    pub fn calls(&self) -> Vec<FakeGhCall> {
        self.world().calls.clone()
    }

    /// The GraphQL calls only.
    pub fn reads(&self) -> Vec<FakeGhCall> {
        self.calls()
            .into_iter()
            .filter(|c| c.kind == "graphql")
            .collect()
    }

    pub fn clear_calls(&self) {
        self.world().calls.clear();
    }
}

fn not_found() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::NotFound, "no gh")
}

impl FakeGhWorld {
    fn node(&self, r: &str, pr: &FakePr) -> serde_json::Value {
        let (repo, n) = r.split_once('#').unwrap_or((r, "0"));
        let n: u64 = n.parse().unwrap_or(0);
        serde_json::json!({
            "number": n, "url": format!("https://github.com/{repo}/pull/{n}"),
            "title": pr.title, "state": pr.state, "isDraft": pr.draft,
            "merged": pr.state == "MERGED",
            "mergedAt": (pr.state == "MERGED").then_some("2026-11-26T11:30:00Z"),
            "closedAt": null, "updatedAt": pr.updated_at, "reviewDecision": pr.review,
            "additions": pr.additions, "deletions": pr.deletions, "headRefName": pr.head,
            "repository": { "nameWithOwner": repo }, "author": { "login": pr.author },
            "commits": { "nodes": pr.rollup.map(|s| vec![serde_json::json!({"commit": {"statusCheckRollup": {"state": s}}})]).unwrap_or_default() },
        })
    }

    fn answer(&mut self, login: &str, prs: &[String], viewer: bool) -> crate::gh::CmdOutput {
        if let Some(f) = self.failures.get_mut(login).and_then(|q| q.pop_front()) {
            let fail = |status, stderr: &str| crate::gh::CmdOutput {
                status,
                stdout: String::new(),
                stderr: stderr.into(),
            };
            return match f {
                FakeFailure::SignedOut => fail(Some(4), "HTTP 401: Bad credentials"),
                FakeFailure::Exit(e) => fail(Some(1), &e),
                FakeFailure::Timeout => fail(None, ""),
                FakeFailure::Secondary => fail(
                    Some(1),
                    "gh: You have exceeded a secondary rate limit. Please wait a few minutes.",
                ),
            };
        }
        let visible = self.visible.get(login).cloned().unwrap_or_default();
        let mut data = serde_json::Map::new();
        let (remaining, reset) = self
            .remaining
            .get(login)
            .cloned()
            .unwrap_or((4999, "2026-11-26T13:00:00Z".into()));
        data.insert(
            "rateLimit".into(),
            serde_json::json!({"cost": 1, "remaining": remaining, "resetAt": reset}),
        );
        let mut errors = Vec::new();
        for (i, r) in prs.iter().enumerate() {
            let alias = format!("p{i}");
            let repo = r.split('#').next().unwrap_or_default();
            if !visible.contains(repo) {
                data.insert(alias.clone(), serde_json::Value::Null);
                errors.push(serde_json::json!({"type": "NOT_FOUND", "path": [alias],
                    "message": format!("Could not resolve to a Repository with the name '{repo}'.")}));
            } else if let Some(pr) = self.prs.get(r) {
                data.insert(alias, serde_json::json!({"pullRequest": self.node(r, pr)}));
            } else {
                data.insert(alias.clone(), serde_json::json!({"pullRequest": null}));
                errors.push(
                    serde_json::json!({"type": "NOT_FOUND", "path": [alias, "pullRequest"],
                    "message": "Could not resolve to a PullRequest."}),
                );
            }
        }
        if viewer {
            let nodes: Vec<serde_json::Value> = self
                .prs
                .iter()
                .filter(|(r, pr)| {
                    pr.author == login
                        && pr.state == "OPEN"
                        && visible.contains(r.split('#').next().unwrap_or_default())
                })
                .map(|(r, pr)| self.node(r, pr))
                .collect();
            data.insert(
                "viewer".into(),
                serde_json::json!({"login": login, "pullRequests": {"nodes": nodes}}),
            );
        }
        let mut body = serde_json::json!({ "data": data });
        if !errors.is_empty() {
            body["errors"] = serde_json::Value::Array(errors);
        }
        crate::gh::CmdOutput {
            status: Some(if body.get("errors").is_some() { 1 } else { 0 }),
            stdout: body.to_string(),
            stderr: String::new(),
        }
    }
}

impl crate::gh::GhRunner for FakeGh {
    fn run(
        &mut self,
        call: &crate::gh::GhCall,
        token: Option<&crate::Secret>,
        _timeout: std::time::Duration,
    ) -> std::io::Result<crate::gh::CmdOutput> {
        let mut w = self.world();
        if w.missing {
            return Err(not_found());
        }
        let argv = crate::gh::args(call);
        match call {
            crate::gh::GhCall::AuthStatus => {
                w.calls.push(FakeGhCall {
                    kind: "auth_status",
                    account: None,
                    prs: Vec::new(),
                    viewer: false,
                    token_set: token.is_some(),
                    argv,
                });
                let entries: Vec<serde_json::Value> = w
                    .accounts
                    .iter()
                    .map(|(login, active, ok)| {
                        serde_json::json!({"state": if *ok { "success" } else { "error" },
                            "active": active, "host": "github.com", "login": login,
                            "tokenSource": "keyring", "gitProtocol": "ssh"})
                    })
                    .collect();
                if w.auth_broken {
                    return Ok(crate::gh::CmdOutput {
                        status: Some(1),
                        stdout: "not json".into(),
                        stderr: "gh: unexpected error".into(),
                    });
                }
                let hosts = if entries.is_empty() {
                    serde_json::json!({})
                } else {
                    serde_json::json!({"github.com": entries})
                };
                Ok(crate::gh::CmdOutput {
                    status: Some(0),
                    stdout: serde_json::json!({ "hosts": hosts }).to_string(),
                    stderr: String::new(),
                })
            }
            crate::gh::GhCall::Graphql { query, vars } => {
                let login = match token {
                    Some(t) => t
                        .expose()
                        .strip_prefix("gho_fake_secret_for_")
                        .unwrap_or("?")
                        .to_string(),
                    None => w
                        .accounts
                        .iter()
                        .find(|a| a.1)
                        .map(|a| a.0.clone())
                        .unwrap_or_default(),
                };
                let get = |name: &str| {
                    vars.iter().find(|(k, _)| k == name).map(|(_, v)| match v {
                        crate::gh::GhVar::Str(s) => s.clone(),
                        crate::gh::GhVar::Int(n) => n.to_string(),
                    })
                };
                let mut prs = Vec::new();
                for i in 0.. {
                    let (Some(o), Some(r), Some(n)) = (
                        get(&format!("o{i}")),
                        get(&format!("r{i}")),
                        get(&format!("n{i}")),
                    ) else {
                        break;
                    };
                    prs.push(format!("{o}/{r}#{n}"));
                }
                let viewer = query.contains("viewer {");
                w.calls.push(FakeGhCall {
                    kind: "graphql",
                    account: Some(login.clone()),
                    prs: prs.clone(),
                    viewer,
                    token_set: token.is_some(),
                    argv,
                });
                let signed_in = w.accounts.iter().any(|a| a.0 == login);
                if !signed_in {
                    return Ok(crate::gh::CmdOutput {
                        status: Some(4),
                        stdout: String::new(),
                        stderr: "HTTP 401: Bad credentials".into(),
                    });
                }
                Ok(w.answer(&login, &prs, viewer))
            }
        }
    }

    fn token(&mut self, login: &str) -> std::io::Result<crate::Secret> {
        let mut w = self.world();
        if w.missing {
            return Err(not_found());
        }
        w.calls.push(FakeGhCall {
            kind: "token",
            account: Some(login.into()),
            prs: Vec::new(),
            viewer: false,
            token_set: false,
            argv: crate::gh::token_args(login),
        });
        if w.accounts.iter().any(|a| a.0 == login) {
            Ok(crate::Secret::new(fake_token(login)))
        } else {
            Ok(crate::Secret::new(""))
        }
    }
}
