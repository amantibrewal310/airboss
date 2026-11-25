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
