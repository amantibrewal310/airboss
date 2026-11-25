# Spec: sync

Module `sync` from [CAPABILITIES.md](CAPABILITIES.md). State: **draft 1, waiting for review**.

It builds on [`core`](SPEC-core.md), including the v2 additions, and is read by [`attention`](SPEC-attention.md) and the app. Facts about `gh`, T3 and Claude Code were checked on this Mac on 25 Nov 2026 ([Checked on this Mac](#checked-on-this-mac)); the rest is under [Not verified yet](#not-verified-yet).

## Objective

`sync` brings live state in from outside, so status is read, not typed. It reads GitHub PRs through the `gh` CLI, T3 threads from T3's database (read-only, whenever it changes), and Claude Code sessions run outside T3 from their session files. It records what it sees in its own tables, and changes a task's status only where `core` allows: `sync:gh` finishes a task when its PRs merge, and a reopened PR reopens a done task (`core` rule 7).

**User stories**

- As me, I see each linked PR's state, review, checks and size, and a merged PR finishes its task by itself.
- As me, I see whether each linked thread is working, waiting on me or stopped, and what it asked; and I see recent threads and my open PRs that aren't linked to any task.
- As `attention`, I get one snapshot of all of this, and a signal when it changes.

## Decisions

- **Inside the app process.** A library, `crates/sync`, on two plain threads (T3 and Claude; GitHub) plus the file watcher's thread. No tokio, no daemon.
- **GitHub through one batched `gh api graphql` call per account per pass**, not `gh pr view` per PR: one process and 1 rate-limit point cover up to 50 PRs, and GitHub returns the combined check state.
- **Every github.com account `gh` is logged in to is used.** This Mac has two, and the active one can't see the private repo this project lives in. A PR is read with whichever account can see it.
- **T3 is read from its database, not its CLI.** `t3 thread show --json` has the state, but costs 0.33 s of CPU per call; the database answers for 60 threads in about 1 ms. All T3-specific SQL lives in one adapter, `src/t3/`.
- **Watch, don't poll.** FSEvents wakes `sync` when T3's WAL or the Claude sessions folder changes. GitHub is the only timer: every 5 minutes, plus on demand.
- **`sync:t3` never changes status** (rule 6). It adds notes (a question, an approval request or a failed run, once each) and links PRs that T3 lists on a linked thread, so a PR the agent forgot to link still counts. `core` allows both (rule 8).
- **Own tables, own connection** to `airboss.db`: `observed_threads`, `observed_prs`, `sync_health`. `core`'s tables are touched only through `Store`. No foreign keys across modules, so a `core` table rebuild can't cascade into `sync`.
- **Times** are UTC text in `core`'s format (`2026-11-25T17:19:07.841Z`). GitHub's `…:08Z` and Claude's epoch milliseconds are converted on the way in, so text order is time order.

## Tech stack

- Rust edition 2024, Rust 1.100, crate `airboss-sync`. Latest stable on crates.io, 25 Nov 2026: `airboss-core` (path), `rusqlite` 0.43.0 `bundled`, `serde` 1.0.229, `serde_json` 1.0.154, `thiserror` 2.0.21.
- **One new crate: `notify` 9.0.0** (default `macos_fsevent`; MSRV 1.91; released 21 Nov 2026). The app should use the same version to watch `airboss.db`.
- No time crate: `src/time.rs` converts epoch milliseconds to text and back.
- **Needs from `core` v2**, with the fallback in brackets: `airboss_core::migrate(conn, module, steps)` made public [copy `db::migrate`]; `Store::links_of_kind(kind) -> Result<Vec<Link>>` [walk `tree()` and call `links(id)`].

## Interfaces

```rust
pub type SharedStore = Arc<Mutex<airboss_core::Store>>;
pub fn start(cfg: SyncConfig, store: SharedStore, tools: Tools) -> Result<SyncHandle, SyncError>;
pub fn run_once(cfg: SyncConfig, store: SharedStore, tools: Tools) -> Result<Snapshot, SyncError>; // one pass of each; CLI --once

pub struct SyncConfig {
    pub db_path: PathBuf, pub t3_db: PathBuf /* ~/.t3/userdata/statev2.sqlite */,
    pub claude_sessions: PathBuf /* ~/.claude/sessions */, pub gh_path: Option<PathBuf> /* None: search */,
    pub pr_every: Duration /* 5 min */, pub t3_min_gap: Duration /* 2 s */, pub untracked_window: Duration /* 7 days */,
}
impl SyncConfig { pub fn defaults(db_path: PathBuf, home: &Path) -> SyncConfig; }
pub struct Tools { pub gh: Box<dyn GhRunner>, pub t3: Box<dyn T3Source>, pub claude: Box<dyn ClaudeSource>,
                   pub watch: Box<dyn Watch>, pub clock: Arc<dyn Clock> }
impl Tools { pub fn real(cfg: &SyncConfig) -> Tools; }           // never called by tests

#[derive(Clone)] pub struct SyncHandle { /* channels to the workers, shared snapshot */ }
impl SyncHandle {
    pub fn snapshot(&self) -> Arc<Snapshot>;                      // replaced whole after each change
    pub fn subscribe(&self) -> Receiver<SyncChange>;
    pub fn on_core_events(&self, events: &[airboss_core::Event]); // the app forwards every core change
    pub fn refresh(&self, what: Refresh);                         // returns at once
    pub fn lookup_pr(&self, pr_ref: &str) -> Result<PrView, SyncError>; // blocking, one gh call ("Link PR")
    pub fn shutdown(&self);
}
pub enum Refresh { All, Prs, PrsOf(TaskId), T3 }
pub enum SyncChange { Threads, Prs, Untracked, Health }

pub struct Snapshot {
    pub threads: HashMap<ThreadKey, ThreadView>, pub prs: HashMap<String, PrView>,  // PRs by links.ref
    pub thread_tasks: HashMap<ThreadKey, Vec<TaskId>>, pub pr_tasks: HashMap<String, Vec<TaskId>>,
    pub health: Health,
}
impl Snapshot {
    pub fn threads_for(&self, task: TaskId) -> Vec<&ThreadView>;
    pub fn prs_for(&self, task: TaskId) -> Vec<&PrView>;
    pub fn untracked(&self) -> Vec<&ThreadView>;                 // newest activity first
    pub fn my_open_prs(&self) -> Vec<&PrView>;                   // open, mine, not linked
    pub fn suggested_prs(&self, branch: &str) -> Vec<&PrView>;   // my open PRs from this branch
}
#[derive(Clone, PartialEq, Eq, Hash)] pub struct ThreadKey { pub source: Source, pub r#ref: String }
pub enum Source { T3, Claude }
pub struct ThreadView {
    pub key: ThreadKey, pub linked: bool, pub state: ThreadState, pub title: Option<String>,
    pub request: Option<Request>, pub error: Option<String>, pub run_id: Option<String>,
    pub project_path: Option<String>, pub branch: Option<String>, pub worktree_path: Option<String>,
    pub parent_ref: Option<String>, pub child_count: u32, pub last_activity_at: Option<String>, pub stale: bool,
}
pub enum ThreadState { Running, NeedsMe, Errored, Idle, Settled, Gone }
pub struct Request { pub id: String, pub kind: RequestKind, pub text: String, pub at: String }
pub enum RequestKind { Question, Approval }
pub struct PrView {
    pub r#ref: String, pub linked: bool, pub url: Option<String>, pub title: Option<String>,
    pub state: Option<PrState>, pub draft: bool, pub review: Review, pub checks: Checks,
    pub additions: Option<u32>, pub deletions: Option<u32>, pub head_ref: Option<String>, pub author: Option<String>,
    pub updated_at: Option<String>, pub ready_since: Option<String>, pub error: Option<PrError>, pub stale: bool,
}
pub enum Review { Approved, ChangesRequested, Required, None }
pub enum Checks { Passing, Failing, Pending, None }
pub enum PrError { UnreadableRef, NotFound, NoAccess, Failed(String) }
pub struct Health { pub gh: SourceHealth, pub t3: SourceHealth, pub claude: SourceHealth }
pub enum SourceHealth { Ok { checked_at: String }, Off(OffReason), Broken { since: String, detail: String },
                        RateLimited { until: String } }
pub enum OffReason { NotInstalled, SignedOut }

// Adapters: real ones in gh.rs, t3/, claude.rs, watch.rs; fakes in fake.rs.
pub trait GhRunner: Send { fn run(&mut self, args: &[String], env: &[(String, String)], timeout: Duration) -> io::Result<CmdOutput>; }
pub trait T3Source: Send { fn data_version(&mut self) -> Result<i64, T3Error>;
                           fn read(&mut self, linked: &[String], untracked_since: &str) -> Result<T3Read, T3Error>; }
pub trait ClaudeSource: Send { fn sessions(&mut self) -> io::Result<Vec<ClaudeSession>>;
                               fn alive(&mut self, s: &[ClaudeSession]) -> io::Result<Vec<bool>>; }
pub trait Watch: Send { fn watch(&mut self, dirs: &[PathBuf], tx: Sender<PathBuf>) -> Result<(), SyncError>; }
pub trait Clock: Send + Sync { fn now(&self) -> String; }

pub mod time;   // from_epoch_ms, to_epoch_ms, plus(text, Duration), normalize(GitHub time); attention uses it
#[cfg(any(test, feature = "fake"))] pub mod fake;   // fakes; attention's tests enable the `fake` feature
```

The app calls `lookup_pr` from a background thread, never the UI thread.

## Data / SQL

Migration `sync` version 1, recorded in `schema_migrations` as `('sync', 1)`. Loaded into an in-memory SQLite on 25 Nov 2026: valid rows went in, and each CHECK rejected its bad row.

```sql
CREATE TABLE observed_threads (           -- one T3 thread or Claude session, linked or untracked
  source TEXT NOT NULL CHECK (source IN ('t3','claude')),
  ref    TEXT NOT NULL CHECK (length(ref) > 0),  -- T3 thread id or Claude session id; same text as links.ref
  linked INTEGER NOT NULL CHECK (linked IN (0,1)),
  state  TEXT NOT NULL CHECK (state IN ('running','needs_me','errored','idle','settled','gone')),
  title TEXT, project_path TEXT, branch TEXT, worktree_path TEXT, parent_ref TEXT,
  child_count INTEGER NOT NULL DEFAULT 0 CHECK (child_count >= 0),
  run_id TEXT, error_text TEXT,                  -- latest run; its failure message when errored
  request_id TEXT, request_kind TEXT CHECK (request_kind IN ('question','approval')),
  request_text TEXT, request_at TEXT,
  pr_refs TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(pr_refs)),  -- PRs T3 listed on it, already linked
  noted_request_id TEXT, noted_run_id TEXT,      -- what sync already wrote a note about
  last_activity_at TEXT,
  first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  changed_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (source, ref),
  CHECK ((request_id IS NULL) = (request_kind IS NULL)),
  CHECK ((state = 'needs_me') = (request_id IS NOT NULL))
);
CREATE INDEX observed_threads_linked_idx ON observed_threads(linked, last_activity_at);

CREATE TABLE observed_prs (               -- one linked PR, or one of my open PRs (linked = 0)
  ref    TEXT PRIMARY KEY CHECK (length(ref) > 0),   -- exactly as in links.ref
  linked INTEGER NOT NULL CHECK (linked IN (0,1)),
  owner TEXT, repo TEXT, number INTEGER CHECK (number > 0),  -- null: ref unreadable
  url TEXT, title TEXT,
  state  TEXT CHECK (state IN ('open','merged','closed')),
  draft  INTEGER NOT NULL DEFAULT 0 CHECK (draft IN (0,1)),
  review TEXT CHECK (review IN ('approved','changes_requested','review_required','none')),
  checks TEXT CHECK (checks IN ('passing','failing','pending','none')),
  additions INTEGER, deletions INTEGER, head_ref TEXT, author TEXT, gh_updated_at TEXT, merged_at TEXT,
  ready_since TEXT,                        -- first seen open and not draft
  account TEXT,                            -- the gh login that can read it
  error TEXT CHECK (error IN ('unreadable_ref','not_found','no_access','failed')), error_detail TEXT,
  checked_at TEXT,
  first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  CHECK ((owner IS NULL) = (error IS 'unreadable_ref')),
  CHECK (ready_since IS NULL OR (state = 'open' AND draft = 0))
);

CREATE TABLE sync_health (                -- survives restarts, so "broken since" stays true
  source TEXT PRIMARY KEY CHECK (source IN ('gh','t3','claude')),
  state  TEXT NOT NULL CHECK (state IN ('ok','off','broken','rate_limited')),
  detail TEXT, since TEXT NOT NULL, last_ok_at TEXT, until TEXT,
  CHECK ((state = 'rate_limited') = (until IS NOT NULL))
);
```

Rows are written only when something changed; `sync_health` once per pass. On start, the snapshot is loaded from these tables, marked `stale` until each source's first pass.

## Behaviour

### GitHub PRs

- **Finding `gh`:** `cfg.gh_path`, else the first of `/opt/homebrew/bin/gh`, `/usr/local/bin/gh`, then `PATH`. A Mac app started from Finder has a short `PATH` without Homebrew.
- **Accounts:** `gh auth status --json hosts` (always exits 0; no tokens in it): `hosts["github.com"]` entries with `state == "success"`, active first. Read at start, hourly, and after an exit code 4. A non-active account's token comes from `gh auth token --user <login>` and stays in memory only.
- **Which PRs:** every PR link whose `core` state is `open`; links not yet in `observed_prs`; rows with `error` `failed` or `no_access`; and each account's own open PRs (`viewer`, first 50), to offer as links and to notice a closed PR reopened. Merged and closed PRs aren't read again, except by `refresh(PrsOf(task))`.
- **When:** 3 s after start, then 5 minutes after each pass ends. On demand: `refresh(Prs | All)` at most once a minute, `PrsOf(task)` at most every 10 s per task, and 5 s after a PR link appears. One `gh` process at a time; requests during a pass join the next one.

The command, once per account with PRs to read, up to 50 PRs each (aliases `p0`…`p49`). Owners, repos and numbers go in as variables, never into the query text, because refs come from agents. Environment: `GH_TOKEN` and `GITHUB_TOKEN` removed (then `GH_TOKEN` set for a non-active account), plus `GH_PROMPT_DISABLED=1`, `GH_NO_UPDATE_NOTIFIER=1`, `GH_SPINNER_DISABLED=1`, `NO_COLOR=1`. Timeout 60 s.

```
gh api graphql -f query=<document> -f o0=<owner> -f r0=<repo> -F n0=<number> …
```
```graphql
query($o0: String!, $r0: String!, $n0: Int!) {          # one triple per PR
  rateLimit { cost remaining resetAt }
  viewer { login pullRequests(states: OPEN, first: 50, orderBy: {field: UPDATED_AT, direction: DESC}) { nodes { ...pr } } }
  p0: repository(owner: $o0, name: $r0) { pullRequest(number: $n0) { ...pr } }
}
fragment pr on PullRequest {
  number url title state isDraft merged mergedAt closedAt updatedAt reviewDecision
  additions deletions headRefName repository { nameWithOwner } author { login }
  commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }
}
```

- **Answers:** stdout has the JSON even on exit 1. Exit 0, or 1 with `data`: use it. An `errors[]` entry `NOT_FOUND` at `path: ["pN"]` means this account can't see the repo: retry with the next account in the same pass, else `no_access`. At `["pN","pullRequest"]` the PR doesn't exist: `not_found`. Exit 4: signed out. Anything else: `failed` with stderr's first line. The account that read a PR is saved and tried first next time.
- **Refs:** `owner/repo#n` or `https://github.com/owner/repo/pull/n`, owner and repo matching `[A-Za-z0-9_.-]+`; otherwise `unreadable_ref`. `set_pr_state` always gets the ref exactly as stored in `links`.

| GitHub | `observed_prs` | `core` |
|---|---|---|
| `state` OPEN / MERGED / CLOSED | `state` open / merged / closed | `set_pr_state(ref, …, sync:gh)` when it differs from the link's `pr_state` |
| `isDraft` | `draft` | |
| `reviewDecision` APPROVED / CHANGES_REQUESTED / REVIEW_REQUIRED / null | `review` approved / changes_requested / review_required / none | |
| rollup `state` SUCCESS / FAILURE, ERROR / PENDING, EXPECTED / none | `checks` passing / failing / pending / none | |
| `additions`, `deletions`, `title`, `url`, `headRefName`, `author.login`, `updatedAt`, `mergedAt` | same | |

**Finishing (edge-triggered).** A PR *leaves open* when GitHub says merged or closed and `core`'s link state or `observed_prs.state` is still `open`. Then: (1) `set_pr_state(ref, Merged | Closed, sync:gh)`; (2) for each task in `tasks_for_link(Pr, ref)` whose status is `todo`, `doing`, `in_review` or `blocked`, if none of its PR links is open and at least one is merged, `set_status(task, Done, Some("PR <ref> merged"), sync:gh)`; (3) write the `observed_prs` row. If the app stops between steps, the next pass repeats them safely. A task is finished only when a PR *becomes* merged, never because its PRs *are* merged, so a task I reopen stays open, and a PR linked as already merged finishes nothing.

**Reopened.** GitHub says open while `core` says closed: `set_pr_state(ref, Open, sync:gh)`, and `core` moves done tasks linked to it back to `in_review` (rule 7).

**PRs found on threads.** Each T3 pass reads `$.pullRequests[*].url` of each linked thread. A ref not in that thread's `pr_refs` is linked to the thread's tasks as `sync:t3`, with the PR's title, as `open`, except on a task already done when T3 says merged or closed, where that state is used so nothing reopens by mistake. Then it joins `pr_refs` (so a link I remove isn't re-added), and a PR pass for it follows 5 s later.

### T3 threads

**Opening:** `file:<t3_db>?mode=ro` with `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_URI`, then `PRAGMA query_only = 1`, `busy_timeout = 200`, `cache_size = -2000` (2 MB; the file is 3 GB). Never `immutable=1`, which would skip the WAL. A pass is one short `BEGIN … COMMIT`, never held open, so T3's checkpoints aren't held back.

**Schema check** at open and after any query error, with `SELECT name FROM pragma_table_info(?1)`:

| Table | Columns used |
|---|---|
| `orchestration_v2_projection_threads` | `thread_id, project_id, title, created_at, updated_at, archived_at, deleted_at, payload_json` |
| `orchestration_v2_projection_runs` | `run_id, thread_id, ordinal, status, requested_at, completed_at, payload_json` |
| `orchestration_v2_projection_runtime_requests` | `runtime_request_id, thread_id, node_id, kind, status, created_at` |
| `orchestration_v2_projection_turn_items` | `thread_id, run_id, node_id, type, status, updated_at, payload_json` |
| `projection_projects` | `project_id, workspace_root` |

`orchestration_v2_projection_provider_threads` was inspected and isn't needed: its `active`/`idle` repeats what runs say.

**Watching:** `notify` on `~/.t3/userdata`, not recursive; only `statev2.sqlite-wal` and `statev2.sqlite` count. Within `t3_min_gap` (2 s) of the last pass, one pass is scheduled for the end of the gap; otherwise it runs now. A pass first reads `PRAGMA data_version` and stops if it hasn't changed. T3 rewrites its WAL every second or two while agents work, so this caps the cost; when T3 is idle, nothing runs.

**Queries.** `?1` is a JSON array of linked T3 thread ids. `j(x,'$.p')` means `CASE WHEN json_valid(x.payload_json) THEN json_extract(x.payload_json,'$.p') END`, written by one helper so every JSON read is guarded. Each ran read-only on the real database using T3's own indexes; for 60 threads the median was under 1 ms per query.

```sql
-- Q1 linked threads
SELECT t.thread_id, t.title, t.updated_at, t.archived_at IS NOT NULL OR t.deleted_at IS NOT NULL,
       j(t,'$.branch'), j(t,'$.worktreePath'), j(t,'$.lineage.parentThreadId'), j(t,'$.settledAt'),
       j(t,'$.pullRequests'), p.workspace_root
FROM json_each(?1) AS w JOIN orchestration_v2_projection_threads AS t ON t.thread_id = w.value
LEFT JOIN projection_projects AS p ON p.project_id = t.project_id;
-- Q2 latest run of each
SELECT r.thread_id, r.run_id, r.status, r.requested_at, j(r,'$.startedAt'), r.completed_at
FROM json_each(?1) AS w JOIN orchestration_v2_projection_runs AS r ON r.thread_id = w.value
 AND r.ordinal = (SELECT max(x.ordinal) FROM orchestration_v2_projection_runs AS x WHERE x.thread_id = w.value);
-- Q3 every pending request, with its question or command
SELECT q.runtime_request_id, q.thread_id, coalesce(j(t,'$.lineage.rootThreadId'), q.thread_id),
       q.kind, q.created_at, j(i,'$.questions[0].header'), j(i,'$.questions[0].question'), j(i,'$.prompt')
FROM orchestration_v2_projection_runtime_requests AS q
JOIN orchestration_v2_projection_threads AS t ON t.thread_id = q.thread_id
LEFT JOIN orchestration_v2_projection_turn_items AS i
  ON i.node_id = q.node_id AND i.type IN ('user_input_request','approval_request')
WHERE q.status = 'pending' ORDER BY q.created_at;
-- Q4 failure message, only when a thread's latest run failed with a new run id (?1 thread, ?2 run)
SELECT j(e,'$.failure.message'), j(e,'$.title') FROM orchestration_v2_projection_turn_items AS e
WHERE e.thread_id = ?1 AND e.run_id = ?2 AND e.type = 'error' AND e.status = 'failed'
ORDER BY e.updated_at DESC LIMIT 1;
-- Q5 child threads (subagents, forks) per linked thread
SELECT j(t,'$.lineage.rootThreadId'), count(*) FROM orchestration_v2_projection_threads AS t
WHERE j(t,'$.lineage.rootThreadId') IN (SELECT value FROM json_each(?1))
  AND t.thread_id NOT IN (SELECT value FROM json_each(?1)) AND t.deleted_at IS NULL GROUP BY 1;
-- Q6 untracked candidates: recent top-level threads (?1 = now minus untracked_window); at most once a minute
SELECT t.thread_id, t.title, t.created_at, t.updated_at, j(t,'$.createdBy'), j(t,'$.settledAt'),
       j(t,'$.branch'), p.workspace_root
FROM orchestration_v2_projection_threads AS t LEFT JOIN projection_projects AS p ON p.project_id = t.project_id
WHERE t.deleted_at IS NULL AND t.archived_at IS NULL AND t.updated_at >= ?1
  AND j(t,'$.lineage.parentThreadId') IS NULL AND coalesce(j(t,'$.lineage.sideChat'), 0) = 0
ORDER BY t.updated_at DESC LIMIT 200;
```

**State of a linked thread**, first match wins:

| State | Rule |
|---|---|
| `gone` | no row, or `archived_at` or `deleted_at` set |
| `needs_me` | a Q3 request whose `thread_id` or root is this thread. `kind = 'user_input'` is a question (text: `question`, else `header`); any other kind is an approval (text: `prompt`) |
| `running` | latest run `queued`, `preparing`, `starting`, `running` or `waiting` |
| `settled` | `$.settledAt` set (T3's "settled" flag) |
| `errored` | latest run `failed`; text from Q4 |
| `idle` | anything else: `completed`, `interrupted`, `cancelled`, `rolled_back`, or no runs |

`last_activity_at` is the latest of the run's `requested_at`, `startedAt`, `completed_at` and the pending request's `created_at`. `parent_ref` is `$.lineage.parentThreadId`; `child_count` comes from Q5.

**Notes.** A linked thread entering `needs_me` with a request id other than `noted_request_id` adds a note to each of its tasks as `sync:t3`, "Asked: …" or "Wants approval: …" (first 200 characters). `errored` with a new run id adds "Run failed: …". This puts the moment in history and keeps "quiet" honest.

**Untracked threads** are Q6 rows linked to no task, stored with `linked = 0` and state `settled` or `idle` only (their runs aren't read), and dropped once older than the window.

### Claude Code sessions

- `notify` also watches `~/.claude/sessions`. Each `<pid>.json` (digits only; the `.key` files beside them are never opened) has `pid`, `sessionId`, `cwd`, `entrypoint`, `name`, `status`, `procStart`, and `updatedAt`/`statusUpdatedAt` in epoch milliseconds.
- `sessionId` is a `claude_session` link's `ref`, and the id in `mcp`'s actor `agent:claude/<id>`.
- `busy` is `running`, `idle` is `idle`, any other value is logged and shown as `idle`; a missing file or dead process is `gone`. `last_activity_at` is `max(updatedAt, statusUpdatedAt)`.
- Liveness: `TZ=UTC LC_ALL=C ps -p <pid,…> -o pid=,lstart=` must print each pid with a start time equal to `procStart` (guards against reused pids). It runs when the set of files changes and on each GitHub tick, never per status change.
- Sessions with `entrypoint` `t3-code` are T3 threads: reported only when linked as `claude_session`, never untracked.

### Following `core`

`on_core_events` re-reads `links(task)` for `linked`, `unlinked` and `link_state` events (`NotFound` means the task was deleted) and updates `thread_tasks` and `pr_tasks`. A new thread or session link triggers a T3 or Claude pass at once, skipping the gap once; a new PR link triggers a PR pass 5 s later. The maps are rebuilt from `links_of_kind` on every GitHub tick, which also catches deleted tasks.

## Errors and degraded modes

Nothing here stops the app. Last known values stay in the snapshot marked `stale`, and `Health` says what's wrong in plain words for the connections footer.

| Case | Detected by | Health and behaviour |
|---|---|---|
| `gh` not found | no binary | `Off(NotInstalled)`: "Install the GitHub CLI to sync pull requests." Checked again each tick |
| Signed out | no `success` account, or exit 4 | `Off(SignedOut)`: "Run `gh auth login` in a terminal." |
| One account's token broken | its `state` isn't `success` | its PRs get `no_access`: "<login> needs `gh auth login` again." |
| Budget low | `rateLimit.remaining < 200` | `RateLimited { until: resetAt }`; no passes until then |
| Secondary limit | stderr mentions "secondary rate limit" | `RateLimited` for 15 minutes |
| Network or other failure | non-zero exit without `data`, or timeout | `Broken { since, detail }`; retry after 1, 2, 4 … 30 minutes; `refresh` tries once at once |
| T3 not installed | `t3_db` missing | `t3: Off(NotInstalled)`; checked again each GitHub tick |
| T3 layout changed | schema check or a query error | `t3: Broken`: "T3 sync broken since 14:02: <table or column>". Retried on the next WAL change; logged once per streak |
| T3 busy | `SQLITE_BUSY` after 200 ms | skip; the next change retries |
| Claude folder missing | no directory | `claude: Off(NotInstalled)` |
| `core` refuses a write | `NotAllowed`, `InvalidTransition` | logged and skipped, never retried in a loop |

A broken source never marks anything as changed. Recovery clears `stale` and sends `SyncChange::Health`.

## Performance targets

Measured on this Mac in release mode, with the machine recorded, as in `core`.

| What | Target |
|---|---|
| CPU while T3 and GitHub are idle | 0%: no wake-ups besides file events and the 5-minute timer |
| One T3 pass: 50 linked threads, 200 untracked candidates | under 5 ms (95th percentile) |
| CPU while T3 agents are busy | under 0.5% of one core |
| T3 question to `SyncChange` | under 3 s |
| One GitHub pass, 50 PRs, one account | 1 `gh` process, 1 rate-limit point, under 150 ms of our CPU |
| GitHub budget used | under 50 points an hour per account (limit 5,000) |
| Merge on GitHub to task done | under 5 min 10 s, or seconds after `refresh` |
| Memory added | under 10 MB |

## Testing

Tests never run the real `gh`, `t3` or `ps`, and never open T3's real database: they build `Tools` from fakes, never `Tools::real`.

- **Fakes** (`src/fake.rs`, also used by `attention`): `FakeGh` answers per argument list and records each call and its environment; `FakeT3`; `FakeClaude`; `FakeWatch` fires on demand; `FakeClock`.
- **The T3 database fake** is a real SQLite file built in a temp dir from `tests/fixtures/t3_schema.sql` (the five tables' DDL and indexes as found on 25 Nov 2026) and `t3_rows.sql`. `T3Db` runs its real queries against it.
- **GitHub fixtures** (`tests/fixtures/gh/*.json`) copy the shapes seen here, with made-up owners: full, partial `NOT_FOUND`, signed out (exit 4), low `rateLimit`.
- **`tests/prs.rs`:** each mapping row; finishing only when a PR leaves open; a reopened task stays open; one merged and one open PR finish nothing; closed then reopened goes back to `in_review`; a pass repeated after a crash between steps; the second account used only after `NOT_FOUND`; no token in tables or logs; unreadable refs; no `gh`.
- **`tests/t3_db.rs`:** each state row on the fixture database; question and approval text; the root-thread rule; untracked filtering (side chats, subagents, archived); a missing column named by the schema check.
- **`tests/t3_sync.rs`:** the 2 s gap with `FakeClock`; unchanged `data_version` reads nothing; one note per request and per failed run; PRs found on threads linked once and not re-added after I unlink; broken, then recovered.
- **`tests/claude.rs`:** states, a reused pid, `t3-code` sessions never untracked. **`tests/migrations.rs`:** each CHECK; opening twice applies once.
- **By hand, not a test:** `cargo run -p airboss-sync --example t3_check` compares `T3Db`'s view of recent threads with `t3 thread show --json` (state, branch, settled). Run it after T3 updates.
- **Performance** (`#[ignore]`): a T3 pass on the fixture grown to 50 linked and 2,000 total threads.

## Project structure

```
crates/sync/
  Cargo.toml                 airboss-sync
  src/lib.rs                 start, run_once, SyncHandle, Snapshot and its types
  src/db.rs                  own connection, migration, observed_* reads and writes
  src/migrations/001_sync.sql
  src/gh.rs  src/prs.rs      finding gh, accounts, query, parsing; mapping, finishing, reopening
  src/t3/mod.rs  src/t3/db.rs   T3Source and T3Db: the only code that knows T3's layout
  src/threads.rs             state rules, notes, PRs found on threads, untracked
  src/claude.rs  src/watch.rs  src/time.rs  src/fake.rs
  examples/t3_check.rs       tests/…
```

## Boundaries

- **Always:** open T3's database read-only; keep T3 SQL in `src/t3/`; act only as `sync:gh` or `sync:t3`; write rows only on change; keep tokens in memory.
- **Ask first:** a new crate; writing to `core` in a new way; reading another T3 table or Claude file.
- **Never:** write to any T3 or Claude Code file; run `gh auth switch` or change `gh`'s setup; finish a task unless a PR left open; poll T3 or the sessions folder on a timer.

## Success criteria

- [ ] A PR merged on GitHub finishes its task within one pass; a task I reopen stays open.
- [ ] A closed PR reopened on GitHub moves its done task back to `in_review`.
- [ ] PRs in the second account's private repos are read, and no token is written anywhere.
- [ ] A question in a linked T3 thread shows as `needs_me` with its text within 3 s, with one note in history.
- [ ] Renaming a column in the fixture T3 database gives "T3 sync broken since …" naming it, and the app keeps working.
- [ ] Without `gh`, or signed out, PR sync is off with a plain reason and nothing else changes.
- [ ] Untracked lists recent top-level T3 threads and outside Claude sessions linked to no task.
- [ ] Every performance target is met on this Mac, with the machine recorded.

## Checked on this Mac

25 Nov 2026, all read-only:

- `gh` 2.105.0 at `/opt/homebrew/bin/gh`, two github.com accounts. The query above ran as written (cost 1, limit 5,000), and the second account's token reached a private repo the active one can't. `gh pr view --json` also has `state`, `reviewDecision` (`""` when none) and `statusCheckRollup` (per check, `CheckRun` or `StatusContext`), and prints a hint on stderr only. With an empty `GH_CONFIG_DIR`, `gh api` exits 4 and `gh auth status --json hosts` prints `{"hosts":{}}`.
- T3's database: 889 threads, 3 GB, WAL. Every query above and its query plan. Request kinds seen: `user_input`, `command`. Run statuses: `completed`, `failed`, `cancelled`, `interrupted`, `rolled_back`, `running`. The WAL changed every 2 s while an agent worked.
- `t3 thread show --json` has `status`, `pendingRequestCount`, `settled`, `branch`, `parentThreadId`; 0.33 s of CPU per call.
- `~/.claude/sessions/*.json` has the fields above; `ps … lstart` in UTC equals `procStart`.

## Not verified yet

- **A pending request's rows.** None was pending while checking. The queries match resolved ones (`questions[0].question`, `prompt`); `status = 'pending'` comes from T3's own index definition.
- **Request kinds** other than `user_input` and `command`; all are treated as approvals.
- **Opening T3's database read-only while T3 is closed**, if `statev2.sqlite-shm` is missing.
- **FSEvents timing** for WAL writes through `notify` 9.0.0, four days old.
- **Claude `status` values** besides `busy` and `idle`; whether waiting for permission has its own; whether the file goes when a session ends.
- **The secondary rate limit's wording**, which wasn't triggered.
- **`gh event listen`** (in preview) could replace the 5-minute timer later; not tried.
