# Spec: sync

Module `sync` from [CAPABILITIES.md](CAPABILITIES.md). State: **draft 2, approved by the EM on 26 Nov 2026**.

It builds on [`core`](SPEC-core.md) v3 and implements `SyncService` from the shared contract (in [SPEC-attention.md](SPEC-attention.md#shared-contract), the one source for `crates/services`). Facts about `gh`, T3 and Claude Code were checked on this Mac on 25 and 26 Nov 2026 ([Checked on this Mac](#checked-on-this-mac)); the rest is under [Not verified yet](#not-verified-yet). What changed since draft 1 is [at the end](#changes-in-draft-2).

## Objective

`sync` brings live state in from outside, so status is read, not typed. It reads GitHub PRs through the `gh` CLI, T3 threads from T3's database (read-only, whenever its WAL is written), and Claude Code sessions run outside T3 from their session files. It records what it sees in its own tables. It changes a task's status only where `core` allows: `sync:gh` finishes a task when GitHub says its PRs merged, and a PR GitHub reports reopened reopens a done task (`core` rule 7).

**User stories**

- As me, I see each linked PR's state, review, checks and size, and a merged PR finishes its task once no agent on it is still working.
- As me, I see whether each linked thread is working, waiting on me or stopped, and what it asked; Open thread takes me to it in T3.
- As me, I see recent threads and my open PRs that aren't linked to any task, and T3's projects when I add repos at first run.
- As `attention`, `launch` and the app, I read one snapshot and get a signal when it changes.

## Decisions

- **Inside the app process.** A library, `crates/sync`, with two worker threads (T3 and Claude; GitHub) plus the `notify` watcher's thread. No tokio, no daemon.
- **Only GitHub decides PR state.** Every PR state that reaches `core` (`set_pr_state`, and the state a PR is linked with) was read by `sync:gh` from GitHub. A state seen only through T3 never reopens or finishes anything.
- **GitHub through one batched `gh api graphql` call per account per pass.** One process and 1 rate-limit point cover up to 50 PRs. Every github.com account `gh` is logged in to is used, each with its own health, backoff and rate limit. Tokens live in a `Secret` and only ever reach a child process's environment.
- **A task finishes** when all its PR links have a GitHub-read state of merged or closed, at least one merged, **and** none of its linked threads is `running` or `needs_me`. Until then the finish is held. The rule is the same for one repo or several: code isn't done while an agent on it is still working.
- **A parent is never finished** (SPEC-core rule 10): a task with steps shows what its steps add up to, so it's done when they are. The finish check skips it, even when it has PRs linked from before it had steps, and a PR T3 lists on its thread isn't linked to it.
- **T3 is read from its database, not its CLI** (`t3 thread show --json` costs 0.33 s of CPU a call; the database answers in about 1 ms). All T3-specific SQL lives in `src/t3/`. Anything unexpected there (an unknown status or kind, a required JSON key missing) marks T3 sync `Broken` with the reason. It never guesses.
- **Watch with kqueue, never poll.** FSEvents doesn't report writes to a WAL file that a process keeps open, so `sync` watches T3's `-wal` with kqueue (`notify`'s kqueue backend) and re-arms the watch when the file is recreated. GitHub is the only timer: every 5 minutes, plus on demand.
- **Notes:** only a failed run writes a history note (`sync:t3`). Questions and approvals are live state for Needs me, not history.
- **PRs that T3 lists on a linked thread** are linked to its tasks as `sync:t3`, only if T3 linked them at or after our thread link, and only with the state GitHub reports.
- **Own tables, own connection** (opened at `Store::path()`, its migration run through `Store::migrate_module("sync", …)`). `core`'s tables are touched only through `Store`. No foreign keys across modules.
- **Time:** deadlines come from the wall clock (`SystemTime`), no wait is longer than 15 minutes, and `woke()` re-checks everything that might be overdue.

## Tech stack

- Rust edition 2024, Rust 1.100, crate `airboss-sync`. Latest stable on crates.io: `airboss-core` and `airboss-services` (paths), `rusqlite` 0.43.0 `bundled`, `serde` 1.0.229, `serde_json` 1.0.154, `thiserror` 2.0.21.
- **One new crate: `notify` 9.0.0 with `default-features = false, features = ["macos_kqueue"]`.** That feature exists in 9.0.0 and makes `notify::KqueueWatcher` available (it pulls in `kqueue` 1.2.1 and `mio` 1). Its source registers `NOTE_DELETE | NOTE_WRITE | NOTE_EXTEND | NOTE_ATTRIB | NOTE_LINK | NOTE_RENAME | NOTE_REVOKE` per watched path, and drops a path's watch on delete, rename or revoke; re-arming is ours. The app uses the same crate and feature for `airboss.db-wal`.
- `core` v3 calls used: `snapshot()` once at start (for the open tasks), then only through `StoreChange`; `links_of_kind`, `links`, `tasks_for_link`, `set_pr_state`, `set_status`, `link`, `add_note`, `task`, `path`, `migrate_module`. PR refs are canonical in `core` (`owner/repo#n`, lowercase); `sync` doesn't normalise them itself. It builds refs only with `PrRef::new` / `PrRef::parse` from `airboss-services`.

## Interfaces

The public types (`SyncService`, `SyncSnapshot`, `ThreadView`, `PrView`, `PrRef`, `Refresh`, `Clock`) are in the shared contract. `Secret` is this crate's own. This crate adds:

```rust
/// Implements airboss_services::SyncService.
pub struct Sync { /* channels to both workers, Arc<SyncSnapshot> behind a lock, Changes */ }
pub fn start(cfg: SyncConfig, store: SharedStore, tools: Tools) -> Result<Arc<Sync>, ServiceError>;
pub fn run_once(cfg: SyncConfig, store: SharedStore, tools: Tools) -> Result<SyncSnapshot, ServiceError>; // CLI --once

pub struct SyncConfig {
    pub t3_dir: PathBuf,               // ~/.t3/userdata (statev2.sqlite, its -wal, environment-id)
    pub claude_sessions: PathBuf,      // ~/.claude/sessions
    pub gh_path: Option<PathBuf>,      // None: search
    pub pr_every: Duration,            // 5 min
    pub t3_min_gap: Duration,          // 2 s between T3 passes
    pub untracked_window: Duration,    // 7 days
}
impl SyncConfig { pub fn defaults(home: &Path) -> SyncConfig; }

pub struct Tools { pub gh: Box<dyn GhRunner>, pub t3: Box<dyn T3Source>, pub claude: Box<dyn ClaudeSource>,
                   pub watch: Box<dyn WalWatch>, pub clock: Arc<dyn Clock> }
impl Tools { pub fn real(cfg: &SyncConfig) -> Tools; }          // never called by tests

/// A token. No Display; Debug prints "Secret(…)". Only gh.rs calls expose(), to set a child's env.
pub struct Secret(String);
impl Secret { pub fn expose(&self) -> &str; }

pub trait GhRunner: Send {
    fn run(&mut self, call: &GhCall, token: Option<&Secret>, timeout: Duration) -> io::Result<CmdOutput>;
    fn token(&mut self, login: &str) -> io::Result<Secret>;     // `gh auth token --user <login>`
}
pub enum GhCall { AuthStatus, Graphql { query: String, vars: Vec<(String, GhVar)> } }
pub trait T3Source: Send {
    fn open(&mut self) -> Result<(), T3Error>;                   // schema and value checks included
    fn data_version(&mut self) -> Result<i64, T3Error>;
    fn read(&mut self, q: &T3Query) -> Result<T3Read, T3Error>;  // Q1–Q6 and Q8 in one read transaction
    fn find_thread(&mut self, project: &Path, branch: &str) -> Result<Option<String>, T3Error>; // Q7
    fn environment_id(&mut self) -> Result<Option<String>, T3Error>;
}
pub trait ClaudeSource: Send { fn sessions(&mut self) -> io::Result<Vec<ClaudeSession>>;
                               fn alive(&mut self, s: &[ClaudeSession]) -> io::Result<Vec<bool>>; }
pub trait WalWatch: Send { fn watch(&mut self, targets: &[WatchTarget], tx: Sender<WatchEvent>) -> Result<(), ServiceError>; }

/// Each worker is a plain state machine; the thread loop only waits and forwards. Tests drive it directly.
pub struct T3Worker  { /* state */ } impl T3Worker  { pub fn handle(&mut self, msg: T3Msg, now: SystemTime) -> Step; }
pub struct GhWorker  { /* state */ } impl GhWorker  { pub fn handle(&mut self, msg: GhMsg, now: SystemTime) -> Step; }
pub struct Step { pub changed: bool, pub next_wake: SystemTime }    // next_wake > now, always
```

`T3Msg` is `Wal`, `ClaudeDir`, `ClaudeFile(PathBuf)`, `Store(StoreSummary)`, `Refresh`, `FindThread { project, branch, reply }`, `Woke`, `Recheck`, `Tick`, `Shutdown`. `GhMsg` is `Store(StoreSummary)`, `Refresh(Refresh)`, `Read(Vec<PrRef>)`, `Woke`, `Recheck`, `Tick`, `Shutdown`. `StoreSummary` is the part of a `StoreChange` a worker needs: the open task ids and the link events.

**Trait methods:**

| Method | What `Sync` does |
|---|---|
| `connections()` | `name` "GitHub": ok when every account is ok; `state` "Signed in" or what's wrong; `detail` "Pull requests sync every 5 minutes. A merged PR finishes its task."; `fix` naming the login and "Run `gh auth login` in a terminal." `name` "T3 Code": `state` "Connected", or "Not updating since 14:02" with the reason as `fix`; `detail` "Shows which agents are working or waiting on you."; it merges with `launch`'s row of the same name (contract rule). `name` "Claude Code": only when Claude sessions can't be read (not "Agents", which is the app's row for the `airboss` command; rows with one name merge). |
| `recheck()` | Re-read `gh` accounts, reopen T3 if broken, then a full pass of each source, skipping the minimum gaps once |
| `changes()` | Bumped after each pass that changed the snapshot |
| `store_changed(c)` | Takes the open task ids from `c.snapshot.tree` and the link events from `c.events`; see [Following core](#following-core). No other `core` read. |
| `woke()` | Wall clock re-read: overdue GitHub pass runs now, T3 `data_version` and Claude liveness are checked, waits recomputed |
| `shutdown()` | Stops both workers and the watcher; returns after they exit (at most 2 s) |
| `snapshot()` | The current `Arc<SyncSnapshot>`, no I/O. Its `checked_at` is the oldest `last_ok_at` among sources (and `gh` accounts) that are ok, or `None` if none is |
| `refresh(what)` | Queues work and returns at once |
| `lookup_pr(pr)` | Blocking, one `gh` call with the account that can read it; for the "Link PR" dialog. Errors: `GhMissing`, `SignedOut`, `NoAccess`, `PrNotFound`, `RateLimited`, `GhFailed` |
| `find_thread(project, branch)` | Q7 on the T3 worker, answered within 2 s; `None` if not found, T3 is off or broken, or no answer in time |
| `thread_url(key)` | `t3code://threads/<environment>/<thread>` from the cached environment id, no I/O; or `None` (see T3) |

## Data / SQL

Migration `sync` version 1 (not shipped yet, so still editable), run with `migrate_module("sync", …)`. Loaded into an in-memory SQLite on 26 Nov 2026: valid rows went in, and each CHECK rejected its bad row.

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
  pr_seen    TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(pr_seen)),     -- T3-listed PRs already handled
  pr_waiting TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(pr_waiting)),  -- T3-listed PRs waiting for a GitHub read
  noted_run_id TEXT,                             -- the failed run sync already wrote a note about
  last_activity_at TEXT,
  first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  changed_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (source, ref),
  CHECK ((request_id IS NULL) = (request_kind IS NULL)),
  CHECK ((state = 'needs_me') = (request_id IS NOT NULL))
);
CREATE INDEX observed_threads_linked_idx ON observed_threads(linked, last_activity_at);

CREATE TABLE observed_prs (               -- what GitHub last said about one PR; written only after a gh read
  ref    TEXT PRIMARY KEY CHECK (length(ref) > 0),   -- canonical owner/repo#n, as core stores it
  linked INTEGER NOT NULL CHECK (linked IN (0,1)),   -- 0: one of my open PRs, offered as a link
  url TEXT, title TEXT,
  state  TEXT CHECK (state IN ('open','merged','closed')),   -- null only with an error
  draft  INTEGER NOT NULL DEFAULT 0 CHECK (draft IN (0,1)),
  review TEXT CHECK (review IN ('approved','changes_requested','review_required','none')),
  checks TEXT CHECK (checks IN ('passing','failing','pending','none')),
  additions INTEGER, deletions INTEGER, head_ref TEXT, author TEXT, gh_updated_at TEXT, merged_at TEXT,
  ready_since TEXT,                        -- first seen open and not draft
  account TEXT,                            -- the gh login that can read it
  error TEXT CHECK (error IN ('not_found','no_access','failed')), error_detail TEXT,
  checked_at TEXT NOT NULL,
  CHECK ((state IS NULL) = (error IS NOT NULL)),
  CHECK (ready_since IS NULL OR (state = 'open' AND draft = 0))
);

CREATE TABLE sync_health (                -- one row per source, and per gh account
  source TEXT NOT NULL CHECK (source IN ('gh','t3','claude')),
  scope  TEXT NOT NULL DEFAULT '',         -- the gh login; '' for t3 and claude
  state  TEXT NOT NULL CHECK (state IN ('ok','off','broken','rate_limited')),
  detail TEXT, since TEXT NOT NULL, last_ok_at TEXT,
  failures   INTEGER NOT NULL DEFAULT 0 CHECK (failures >= 0),  -- drives backoff
  next_try_at TEXT,                        -- backoff or rate-limit end
  PRIMARY KEY (source, scope),
  CHECK (state <> 'rate_limited' OR next_try_at IS NOT NULL)
);

CREATE TABLE sync_finish_held (           -- a task whose PRs merged while an agent on it still works
  task_id INTEGER PRIMARY KEY,             -- no foreign key: core owns tasks
  since   TEXT NOT NULL
);
```

Rows are written only when something changed. On start the snapshot is loaded from these tables, marked `stale` until each source's first pass.

## Behaviour

### Workers and time

- Each worker thread loops: `step = worker.handle(msg, clock.now())`, then waits on its channel with `recv_timeout(min(step.next_wake − now, 15 min))`; a timeout sends `Tick`. `handle` asserts `next_wake > now` (an overdue deadline is acted on inside `handle`, not by waiting zero).
- Deadlines are `SystemTime`: the next GitHub pass (5 min after the last ended), per-account backoff and rate-limit ends, the end of the T3 gap, the next untracked read. After a sleep the Mac's wall clock has moved, so the first `Tick` or `woke()` finds them overdue and acts. When the clock is set back, the GitHub worker pulls its next pass to at most one interval from now, and forgets refresh and tick times in the future, so however far back the clock goes, a merge is seen within two intervals: the wait already under way, then one more.
- A panic inside `handle` is caught (`catch_unwind`), logged, and turns the source's health to `Broken`; the thread keeps running.

### GitHub PRs

- **Finding `gh`:** `cfg.gh_path`, else the first of `/opt/homebrew/bin/gh`, `/usr/local/bin/gh`, then `PATH`. A Mac app started from Finder has a short `PATH` without Homebrew.
- **Accounts:** `gh auth status --json hosts` (always exits 0; it has no tokens): `hosts["github.com"]` entries with `state == "success"`, active first. Read at start, hourly, on `recheck()`, and after an exit code 4. Each non-active account's token comes from `gh auth token --user <login>` into a `Secret`, held in memory and fetched again after an auth error.
- **Per account:** health (`ok`, `broken`, `rate_limited`), failures and next try are kept per login in `sync_health`. One account backing off or rate-limited holds only the PRs it reads.
- **Which PRs:** every PR link whose task is open and whose GitHub-read state is `open` or unknown; PRs waiting from T3 threads (`pr_waiting`); `failed` or `no_access` rows (after backoff); and each account's own open PRs (`viewer`, first 50), to offer as links and to notice a closed PR reopened. Merged and closed PRs aren't read again, except by `refresh(PrsOf(task))`.
- **When:** 3 s after start, then 5 minutes after each pass ends. On demand: `Refresh::Prs`/`All` at most once a minute, `PrsOf(task)` at most every 10 s per task, and 5 s after a PR link or a T3-listed PR appears. One `gh` process at a time; requests during a pass join the next one.

The command, once per account with PRs to read, up to 50 PRs each (aliases `p0`…`p49`). Owner, repo and number come from `PrRef::parts()` and go in as variables, never into the query text. Child environment: `GH_TOKEN` and `GITHUB_TOKEN` removed, then `GH_TOKEN=<secret>` for a non-active account, plus `GH_PROMPT_DISABLED=1`, `GH_NO_UPDATE_NOTIFIER=1`, `GH_SPINNER_DISABLED=1`, `NO_COLOR=1`. Timeout 60 s, then `gh` is killed; once it has exited or been killed, its output gets 1 s more, as in `launch`'s runner, so a process it left holding the pipes can't hold the worker. The token never appears in argv, logs, errors or tables.

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

- **Answers:** stdout has the JSON even on exit 1. Exit 0, or 1 with `data`: use it. `NOT_FOUND` at `path: ["pN"]`: this account can't see the repo; retry with the next account in the same pass, else `no_access`. At `["pN","pullRequest"]`: `not_found`. Exit 4: that account signed out. Anything else: `failed` with stderr's first line. The account that read a PR is saved and tried first next time. A viewer PR's ref is `PrRef::new(nameWithOwner, number)`.

| GitHub | `observed_prs` | `core` |
|---|---|---|
| `state` OPEN / MERGED / CLOSED | `state` open / merged / closed | `set_pr_state(ref, …, sync:gh)` when it differs from the link's `pr_state` |
| `isDraft` | `draft` | |
| `reviewDecision` APPROVED / CHANGES_REQUESTED / REVIEW_REQUIRED / null | `review` approved / changes_requested / review_required / none | |
| rollup `state` SUCCESS / FAILURE, ERROR / PENDING, EXPECTED / no rollup | `checks` passing / failing / pending / none | |
| `additions`, `deletions`, `title`, `url`, `headRefName`, `author.login`, `updatedAt`, `mergedAt` | same | |

**Finishing.** The *finish check* for a task passes when it has no steps (`has_steps` is false: a parent's status follows its steps, SPEC-core rule 10); it is `todo`, `doing`, `in_review` or `blocked`; every one of its PR links has an `observed_prs` state (a GitHub read); none is `open`; and at least one is `merged`. It runs only on these triggers, never because I changed a task:

1. **A PR left open:** GitHub says merged or closed where `observed_prs` or `core` still said `open`. Check every task linked to it.
2. **A link's first read:** a PR link with no `observed_prs` row yet (or one `sync:t3` just made) is read, and is merged or closed. Check that link's tasks.
3. **A hold ends:** see below.

When the check passes: first `set_pr_state(ref, <GitHub state>, sync:gh)` for each of the task's PR links whose `core` state differs, then, if none of the task's linked threads (T3 or Claude) is `running` or `needs_me`, `set_status(task, Done, Some("PRs merged: <refs>"), sync:gh)`. Otherwise it inserts `sync_finish_held(task)`. Held tasks are checked again after every T3 or Claude pass that changed a thread state, and at start. A hold is dropped when the task finishes, leaves the open statuses, or a PR of it reads `open` again. If the app stops between steps, the next pass repeats them; each step is safe to repeat. A task I reopen after it finished stays open: no trigger fires for it.

**Reopened.** GitHub says open where `core` says closed: `set_pr_state(ref, Open, sync:gh)`, and `core` moves done tasks linked to it back to `in_review` (rule 7).

**PRs found on threads.** Each T3 pass reads `$.pullRequests[*]` of each linked thread (open tasks only). An entry counts when its `host` is `github.com` and its `linkedAt` is at or after the earliest `linked_at` of that thread's `t3_thread` links. Its ref is `PrRef::new(repository, number)`. A ref not in `pr_seen` or `pr_waiting` goes into `pr_waiting`, and a GitHub read follows 5 s later. Once GitHub has read it: `link(task, NewLink::pr(ref).with_url(url).with_title(title).with_pr_state(<GitHub state>), sync:t3)` for each task whose thread link is no newer than the entry's `linkedAt`, then, only once every one of those links is in `core`, the ref moves to `pr_seen` (so a link I remove isn't re-added), then trigger 2. A link `core` refuses, such as on a busy file, leaves the ref waiting, so the next pass links it. `HasSteps` is the exception: a parent takes no PR (a thread on a whole parent links its PRs to the steps), so that task counts as done with and nothing is retried. A ref GitHub can't read stays waiting with its error shown, retried with the account's backoff. T3's own `snapshot.state` is never used.

### T3 threads

**Opening:** `file:<t3_dir>/statev2.sqlite?mode=ro` with `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_URI`, then `PRAGMA query_only = 1`, `busy_timeout = 200`, `cache_size = -2000` (2 MB; the file is 3 GB). Never `immutable=1`, which would skip the WAL. A pass is one short `BEGIN … COMMIT`, never held open, so T3's checkpoints aren't held back. `environment-id` in the same folder is read (trimmed; it must be a UUID) at open.

**Checks at open, hourly, and after any query error.** Each failure marks T3 sync `Broken` with a reason naming the table, column, key or value:

- Columns, with `SELECT name FROM pragma_table_info(?1)`:

| Table | Columns used |
|---|---|
| `orchestration_v2_projection_threads` | `thread_id, project_id, title, created_at, updated_at, archived_at, deleted_at, payload_json` |
| `orchestration_v2_projection_runs` | `run_id, thread_id, ordinal, status, requested_at, completed_at, payload_json` |
| `orchestration_v2_projection_runtime_requests` | `runtime_request_id, thread_id, node_id, kind, status, created_at` |
| `orchestration_v2_projection_turn_items` | `thread_id, run_id, node_id, type, status, updated_at, payload_json` |
| `projection_projects` | `project_id, title, workspace_root, updated_at, deleted_at, archived_at` |

- Values (Q0 below), against T3 0.1.12's own schema: run status ∈ `preparing, queued, starting, running, waiting, completed, interrupted, failed, cancelled, rolled_back`; request status ∈ `pending, resolved, expired, cancelled`; request kind ∈ `user_input, command, file-read, file-change, mcp-elicitation, permission, dynamic_tool_call, auth_refresh`. Every pass also checks each value it reads against these lists.
- Required JSON keys (present on all 909 threads and required by T3's schema): threads `$.lineage.parentThreadId`, `$.lineage.rootThreadId`, `$.settledAt`, `$.branch`, `$.worktreePath`; runs `$.startedAt`; question items `$.questions[0].question`; error items `$.failure.message`; thread PR entries `url`, `linkedAt`, `repository`, `number`, `host`. A key may hold JSON `null`; absent is `Broken`. Absence is tested with `json_type(…) IS NULL`, which is NULL only when the key is missing. Optional keys, by T3's schema or data: `$.pullRequests` (absent on 14 of 909 threads: no PRs), `$.lineage.sideChat`, an approval's `$.prompt`.

**Watching.** `notify::KqueueWatcher` watches two targets: the folder `~/.t3/userdata` (non-recursive; a write to the folder means an entry was added, removed or renamed) and the file `statev2.sqlite-wal`. A data or size event on the WAL means T3 wrote. A remove, rename or revoke means the WAL is gone (T3 closed its last connection); `notify` drops that watch, and the next folder event re-adds it once the file exists again. Within `t3_min_gap` (2 s) of the last pass, one pass is scheduled for the end of the gap; otherwise it runs now. A pass first reads `PRAGMA data_version` and stops if it hasn't changed. `woke()` and `recheck()` also read `data_version`, as the safety net.

**Queries.** `?1` is a JSON array of the T3 thread ids linked to **open** tasks (`todo`, `doing`, `in_review`, `blocked`); threads of other tasks aren't read. `j(x,'$.p')` means `CASE WHEN json_valid(x.payload_json) THEN json_extract(x.payload_json,'$.p') END`, and `jt(x,'$.p')` the same with `json_type`; one helper writes both, so every JSON read is guarded. All ran read-only on the real database using T3's indexes; for 60 threads each was under 1 ms (median), Q0 0.4 ms, Q7 0.4 ms.

```sql
-- Q0 value check (at open and hourly)
SELECT 'run_status', status FROM orchestration_v2_projection_runs GROUP BY status
UNION ALL SELECT 'request_status', status FROM orchestration_v2_projection_runtime_requests GROUP BY status
UNION ALL SELECT 'request_kind', kind FROM orchestration_v2_projection_runtime_requests GROUP BY kind;
-- Q1 linked threads, with required-key presence
SELECT t.thread_id, t.title, t.updated_at, t.archived_at IS NOT NULL OR t.deleted_at IS NOT NULL,
       j(t,'$.branch'), j(t,'$.worktreePath'), j(t,'$.lineage.parentThreadId'), j(t,'$.settledAt'),
       j(t,'$.pullRequests'), p.workspace_root,
       jt(t,'$.branch'), jt(t,'$.worktreePath'), jt(t,'$.lineage.parentThreadId'),
       jt(t,'$.lineage.rootThreadId'), jt(t,'$.settledAt'),
       -- the last reply: newest finished top-level assistant message (thread_ordinal index),
       -- trimmed and cut to its last 6,000 characters; the panel shows its next steps
       (SELECT j(m,'$.text') FROM orchestration_v2_projection_turn_items AS m
        WHERE m.thread_id = t.thread_id AND m.type = 'assistant_message' AND m.status = 'completed'
          AND m.parent_item_id IS NULL ORDER BY m.ordinal DESC LIMIT 1)
FROM json_each(?1) AS w JOIN orchestration_v2_projection_threads AS t ON t.thread_id = w.value
LEFT JOIN projection_projects AS p ON p.project_id = t.project_id;
-- Q2 latest run of each
SELECT r.thread_id, r.run_id, r.status, r.requested_at, j(r,'$.startedAt'), jt(r,'$.startedAt'), r.completed_at
FROM json_each(?1) AS w JOIN orchestration_v2_projection_runs AS r ON r.thread_id = w.value
 AND r.ordinal = (SELECT max(x.ordinal) FROM orchestration_v2_projection_runs AS x WHERE x.thread_id = w.value);
-- Q3 every pending request, with its question or command
SELECT q.runtime_request_id, q.thread_id, j(t,'$.lineage.rootThreadId'), q.kind, q.created_at, i.type,
       j(i,'$.questions[0].question'), jt(i,'$.questions[0].question'), j(i,'$.prompt')
FROM orchestration_v2_projection_runtime_requests AS q
JOIN orchestration_v2_projection_threads AS t ON t.thread_id = q.thread_id
LEFT JOIN orchestration_v2_projection_turn_items AS i
  ON i.node_id = q.node_id AND i.type IN ('user_input_request','approval_request')
WHERE q.status = 'pending' ORDER BY q.created_at;
-- Q4 failure message, only when a linked thread's latest run failed with a new run id (?1 thread, ?2 run)
SELECT j(e,'$.failure.message'), jt(e,'$.failure.message') FROM orchestration_v2_projection_turn_items AS e
WHERE e.thread_id = ?1 AND e.run_id = ?2 AND e.type = 'error' AND e.status = 'failed'
ORDER BY e.updated_at DESC LIMIT 1;
-- Q5 child threads (subagents, forks) per linked thread
SELECT j(t,'$.lineage.rootThreadId'), count(*) FROM orchestration_v2_projection_threads AS t
WHERE j(t,'$.lineage.rootThreadId') IN (SELECT value FROM json_each(?1))
  AND t.thread_id NOT IN (SELECT value FROM json_each(?1)) AND t.deleted_at IS NULL GROUP BY 1;
-- Q6 untracked candidates (?1 = now minus untracked_window); at most once a minute
SELECT t.thread_id, t.title, t.created_at, t.updated_at, j(t,'$.settledAt'), j(t,'$.branch'), p.workspace_root
FROM orchestration_v2_projection_threads AS t LEFT JOIN projection_projects AS p ON p.project_id = t.project_id
WHERE t.deleted_at IS NULL AND t.archived_at IS NULL AND t.updated_at >= ?1
  AND j(t,'$.lineage.parentThreadId') IS NULL AND coalesce(j(t,'$.lineage.sideChat'), 0) = 0
ORDER BY t.updated_at DESC LIMIT 200;
-- Q7 find_thread (?1 project id, ?2 branch): newest live thread on that branch
SELECT t.thread_id FROM orchestration_v2_projection_threads AS t
WHERE t.project_id = ?1 AND t.deleted_at IS NULL AND j(t,'$.branch') = ?2
ORDER BY t.created_at DESC LIMIT 1;
-- Q8 T3's projects (with Q6), for first run's "Add repos": SyncSnapshot.projects as T3Project { id, name: title, path: workspace_root }
SELECT project_id, title, workspace_root FROM projection_projects
WHERE deleted_at IS NULL AND archived_at IS NULL ORDER BY updated_at DESC;
```

**State of a linked thread**, first match wins:

| State | Rule |
|---|---|
| `gone` | no row, or `archived_at` or `deleted_at` set |
| `needs_me` | a Q3 request whose `thread_id` or root is this thread. `user_input` is a question: text `questions[0].question` (required). Every other known kind is an approval: text `prompt` if present, else the kind's phrase: run a command, read a file, change files, grant a permission, use a tool, answer an MCP server, sign in again. A pending request with no item yet is retried on the next pass; still missing after 10 s is `Broken` |
| `running` | latest run `preparing`, `queued`, `starting`, `running` or `waiting` |
| `settled` | `$.settledAt` not null (T3's "settled" flag) |
| `errored` | latest run `failed`; text from Q4 (a failed run with no error item shows "Run failed" and is logged) |
| `idle` | latest run `completed`, `interrupted`, `cancelled` or `rolled_back`, or no runs |

`chip()` maps `running` to `Some(Working)`, `needs_me` to `Some(WaitingOnYou)`, `errored`, `idle` and `settled` to `Some(Stopped)`, and `gone` to `None`. `last_activity_at` is the latest of the run's `requested_at`, `startedAt`, `completed_at` and the pending request's `created_at`. `parent_ref` is `$.lineage.parentThreadId`; `child_count` comes from Q5.

**Note:** a linked thread entering `errored` with a run id other than `noted_run_id` adds "Run failed: <message, first 200 characters>" to each of its open tasks as `sync:t3`. Nothing else writes history.

**Untracked threads** are Q6 rows linked to no task, stored with `linked = 0`, state `settled` or `idle` only (their runs aren't read), and dropped once older than the window.

**`find_thread(project, branch)`** resolves `project` to the deepest Q8 project whose `workspace_root` contains it (both canonicalised), then runs Q7. It answers `launch`'s pending records.

**`thread_url`** for a T3 key is `t3code://threads/<env>/<thread>`, each part percent-encoded (everything but `A–Z a–z 0–9 - _`); `None` for a Claude key, or when `environment-id` is missing or not a UUID (the "T3 Code" connection then says why).

### Claude Code sessions

- kqueue (same watcher) on `~/.claude/sessions` and on each `<pid>.json` in it (digits only; the `.key` files beside them are never opened). A folder event re-scans it and adjusts the file watches.
- Each file has `pid`, `sessionId`, `cwd`, `entrypoint`, `name`, `status`, `procStart`, and `updatedAt`/`statusUpdatedAt` in epoch milliseconds (Claude Code 2.1 adds more keys, which are ignored). `sessionId` is a `claude_session` link's `ref`. `pid`, `sessionId`, `status` and `procStart` are required.
- **Caught mid-write:** Claude Code writes a new session's file before its `status` (about 1.4 s). A file that isn't valid JSON, or that lacks a required field and was modified (mtime) less than 10 s ago, is skipped for that pass, and stays watched so its next write reads it again. One still lacking a field after 10 s marks Claude sync `Broken` ("Claude session file <path> has no <field>"), so nothing falls back silently.
- `busy` is `running`, `idle` is `idle`, `waiting` is `needs_me` (on a question or a permission; `waitingFor` names which, and isn't read). No request is read for it, so it shows the Waiting on you chip and holds finishing, but makes no Needs me item. `shell` is `running` too: the turn has ended but a background shell task still runs, which Claude Code's own session list counts as busy. Claude Code adds statuses over time, so any other `status` is `idle` and logged once by value; it never breaks Claude sync (an unknown value broke it twice in QA: `waiting`, then `shell`). A missing file or a dead process is `gone`. `last_activity_at` is `max(updatedAt, statusUpdatedAt)`.
- Liveness: `TZ=UTC LC_ALL=C ps -p <pid,…> -o pid=,lstart=` must print each pid with a start time equal to `procStart`. It runs when the set of files changes, on `woke()`, and on each GitHub tick.
- Sessions with `entrypoint` `t3-code` are T3 threads: reported only when linked as `claude_session`, never untracked.

### Herdr terminals

Added for [SPEC-herdr](SPEC-herdr.md) ("sync: the Herdr side"). Code: `src/herdr.rs`, and the Herdr part of the T3 worker.

- **The stream.** `HerdrSource::follow(stream, tx)` runs `hc events --json` (`SyncConfig::hc_path`, `~/.local/bin/hc`, else `PATH`) with stdin null and stderr discarded, and a thread reads its lines. `stop()` kills and waits for it, so no zombie is left: the worker calls it when a stream ends, before restarting, and on shutdown (`Drop` does too). Every message carries its stream's number; lines from a replaced stream are ignored.
- **What `hc` 0.1.22 prints** (checked 26 Nov 2026 with a probe terminal): first a snapshot, `{channel, seq, terminals:[…]}`, listing every terminal, finished ones too. Then JSON-RPC notifications, one per fact: `{"method":"event","params":{"type","terminal","seq","data"}}`, where `created` carries the whole terminal, `status` carries `{status: null | {state, app?, kind?}}` but **no `status_ms`**, `title` `{title}`, `finished` `{exit_code, finished_ms, …}`, `removed`. `heartbeat` comes every 15 s; `foreground`, `turn`, `size`, `labels` and `doc` events are dropped by the reader, so they never wake the worker. `missed` (behind) is followed by a fresh snapshot.
- **Applying them.** A snapshot replaces the table (a terminal it doesn't list is gone); each event changes one terminal; an event whose `seq` the last snapshot already covers is skipped. A status event replaces the word, kind and message, and is stamped with the time it arrived, as `status_ms`. A snapshot or `created`/`restarted` record keeps a terminal's old `status_ms` while all three are unchanged, and restamps it (now) when one changed, so a request's id survives stream restarts but a new message while blocked is a new request. Terminals are read tolerantly: a state other than `live` is not live, a status without a word is none, a missing field is unknown, and only a terminal without an address is skipped (logged once). `lost` clears the status word; `restarted` (the whole terminal again) and any status word make it live. An event missing what it names is skipped. Only a line that isn't JSON, or isn't a snapshot or notification, ends the stream.
- **States** (Decision 6): `live` and `working` is `running`, `blocked` is `needs_me` with a request built from the status's `kind` and `msg`: an approval when `kind` is `permission` and there's a message, else a question; its text the trimmed message, or "Waiting on you in Herdr."; its id `herdr/<address>/<status_ms>`. `error` is `errored` with no text, `done`, `idle`, no status or any other word is `idle`, and `finished` or not listed is `gone`. `title` is the terminal's title, else its name; `project_path` its `cwd`; `last_activity_at` its `status_ms`. Only `herdr_terminal` links on open tasks are reported, never untracked terminals, and `thread_url` is `None`.
- **Restarts.** When the stream ends it's restarted after 1 s, doubling to 60 s. A snapshot ends the streak; an end before it, within 60 s of the start, counts as one more failure in a row. After 3 failed restarts in a row (the fourth end) Herdr is `broken`, and it's still restarted every minute. `woke()` restarts the stream for a fresh snapshot; `recheck()` does too and forgets the streak.
- **Health:** a "Herdr" row. None while `hc` isn't installed (`off`, looked for again on each GitHub tick, wake and recheck; every linked terminal is then `gone`). "Connected" once the stream's snapshot arrives. "Herdr's status can't be read", with the reason, when broken; the views stay, marked `stale`.
- **`run_once`** waits up to 2 s for the snapshot, then stops the stream.
- **Migration 2** (`002_herdr.sql`) rebuilds `observed_threads` and `sync_health` to allow `source = 'herdr'`, and loosens one rule: `needs_me` no longer requires a request (only a request requires `needs_me`). Version 1 refused a waiting Claude session's row, and would have refused a blocked Herdr terminal's, which the finish check reads.
- **Tests:** `tests/herdr.rs` (captured lines, mapping, the real source against a script standing in for `hc`: lines, heartbeats dropped, exit, kill and reap), the Herdr section of `tests/t3_worker.rs` (each mapping row, snapshot then changes, a terminal that disappears, `seq`, backoff and broken, missing `hc`, woke, saved state), a held finish in `tests/prs.rs`, the service in `tests/service.rs`, and `tests/real_herdr.rs` (`#[ignore]`: `cargo test -p airboss-sync --test real_herdr -- --ignored` starts `airboss-probe-<pid>`, fakes each status with `printf '\033]7501;state=working\007'`; a message must be base64, `state=blocked:kind=permission:msg=dHJ1c3Q=`, or the whole report is ignored, then kills and removes it).

### Following core

`store_changed(c)` passes a `StoreSummary` to both workers: the open task ids from `c.snapshot.tree`, and `c.events` of kind `linked`, `unlinked` or `link_state`. For those, `sync` re-reads `links_of_kind` for the kinds they name (one indexed query each) and rebuilds `thread_tasks` and `pr_tasks`; tasks no longer in the tree drop out. A new thread link triggers a T3 or Claude pass at once (skipping the gap once), and a new PR link a GitHub read 5 s later. A task becoming open adds its threads to Q1's list. No `tree()`, `ready()` or `proposed()` calls.

## Errors and degraded modes

Nothing here stops the app. Last known values stay in the snapshot marked `stale`, and `connections()` says what's wrong in plain words.

| Case | Detected by | Health and behaviour |
|---|---|---|
| `gh` not found | no binary | GitHub off: "Install the GitHub CLI to sync pull requests." Checked again each tick |
| Signed out | no `success` account | GitHub off: "Run `gh auth login` in a terminal." |
| One account broken | its `state` isn't `success`, or exit 4 with its token | that login `broken`; its PRs wait; `fix`: "<login> needs `gh auth login` again." |
| An account's budget low | `rateLimit.remaining < 200` | that login `rate_limited` until `resetAt` |
| Secondary limit | stderr mentions "secondary rate limit" | that login `rate_limited` for 15 minutes |
| Network or other failure | non-zero exit without `data`, or timeout | that login `broken`; next try after 1, 2, 4 … 30 minutes; `recheck()` tries once at once |
| T3 not installed | no `statev2.sqlite` | T3 off; checked again each GitHub tick |
| T3 layout or values changed | column, value or key check, or any query error | `Broken`: "Not updating since 14:02: <what>". Retried on the next WAL event and hourly; logged once per streak |
| T3 busy | `SQLITE_BUSY` after 200 ms | skip; the next event retries |
| Claude folder missing | no directory | Claude off (no connection row) |
| `core` refuses a write | `NotAllowed`, `InvalidTransition` | logged and skipped, never retried in a loop |

A broken source never marks anything as changed. Recovery clears `stale` and bumps `changes()`.

## Performance targets

Measured on this Mac in release mode, with the machine recorded, as in `core`.

| What | Target |
|---|---|
| Wake-ups with T3 and GitHub idle | 12 an hour (the GitHub timer); nothing else |
| CPU with T3 and GitHub idle | under 0.05% averaged over an hour (one `gh` process every 5 minutes, about 50 ms each) |
| CPU while T3 agents are busy | at most one T3 pass per 2 s, under 5 ms each: under 0.5% of one core |
| One T3 pass: 50 threads on open tasks, 200 untracked candidates | under 5 ms (95th percentile) |
| T3 question to `changes()` bump | under 3 s |
| One GitHub pass, 50 PRs, one account | 1 `gh` process, 1 rate-limit point, under 150 ms of our CPU |
| GitHub budget used | under 50 points an hour per account (limit 5,000) |
| Merge on GitHub to task done (no agent still working) | under 5 min 10 s, or seconds after `refresh` |
| Memory added | under 10 MB |

## Testing

Tests never run the real `gh`, `t3` or `ps`, and never open T3's real database, except the one `#[ignore]` test below. They build `Tools` from fakes, never `Tools::real`.

- **Fakes** (`src/fake.rs`): `FakeGh` answers by *call kind and key arguments* (auth status; a GraphQL read for a given account and set of PR refs), not by exact argv, and records each call and whether a token was set; `FakeT3`; `FakeClaude`; `FakeWatch`, which fires on demand; `FakeClock` (a settable `SystemTime`).
- **Timing without real time:** every timing test calls `T3Worker::handle` or `GhWorker::handle` with chosen `now` values: the 2 s gap, the 5-minute timer, backoff per account, a rate limit ending, a 15-minute cap, a 6-hour sleep then `Woke`, and `next_wake > now` after every step.
- **The T3 database fake** is a real SQLite file built in a temp dir from `tests/fixtures/t3_schema.sql` (the five tables' DDL and indexes as found on 25 Nov 2026) and `t3_rows.sql`. `T3Db` runs its real queries against it.
- **`tests/prs.rs`:** each mapping row; each finish trigger; a reopened task stays open; one merged and one open PR finish nothing; a hold while a thread runs, released when it stops; closed then reopened goes back to `in_review`; a pass repeated after a crash between steps; a T3-listed PR linked only after GitHub read it, with GitHub's state, and only if T3 linked it after our thread link; T3 saying merged while GitHub says open changes nothing; the second account used only after `NOT_FOUND`; one account rate-limited while the other keeps reading; no token in argv, tables, logs or `Debug` output.
- **`tests/t3_db.rs`:** each state row on the fixture; question and approval text, including an approval without `prompt`; the root-thread rule; untracked filtering; Q7 and Q8; `Broken` for a missing column, an unknown run status, an unknown request kind, and a missing `$.settledAt` key; JSON `null` accepted.
- **`tests/watch.rs` (the real watcher):** a child process (this test binary re-run with an env flag) opens a WAL database in a temp dir, keeps the connection open and writes 5 times; the kqueue watcher must report each within 1 s. The child then closes (the WAL is deleted) and reopens and writes again; the watcher must re-arm and report that too.
- **`tests/claude.rs`:** states (`waiting` too), a reused pid, an unknown status, a file caught mid-write (skipped while fresh, `InvalidData` after 10 s), `t3-code` sessions never untracked. **`tests/migrations.rs`:** each CHECK; opening twice applies once.
- **`tests/real_t3.rs`** (`#[ignore]`, run by hand): opens `~/.t3/userdata/statev2.sqlite` with `mode=ro`, runs the checks and every query Q0–Q8 for the 50 most recent threads, and fails on `Broken`. Run it after every T3 update.
- **By hand:** `cargo run -p airboss-sync --example t3_check` compares `T3Db`'s view of recent threads with `t3 thread show --json` (state, branch, settled).
- **Performance** (`#[ignore]`): a T3 pass on the fixture grown to 50 linked and 2,000 total threads.

## Project structure

```
crates/sync/
  Cargo.toml                 airboss-sync
  src/lib.rs                 Sync (SyncService), start, run_once, SyncConfig, Tools
  src/db.rs                  own connection, migration (src/migrations/001_sync.sql), table reads and writes
  src/gh.rs  src/secret.rs   finding gh, accounts, the query, parsing, GhRunner; Secret
  src/prs.rs  src/finish.rs  mapping, reopening, T3-listed PRs; the finish check and holds
  src/t3/mod.rs  src/t3/db.rs   T3Source and T3Db: the only code that knows T3's layout
  src/threads.rs             state rules, the failed-run note, untracked, find_thread, thread_url
  src/claude.rs  src/watch.rs  src/worker.rs  src/fake.rs
  examples/t3_check.rs       tests/…
```

## Boundaries

- **Always:** open T3's database read-only; keep T3 SQL in `src/t3/`; act only as `sync:gh` or `sync:t3`; send `core` only PR states read from GitHub; write rows only on change; keep tokens in `Secret`.
- **Ask first:** a new crate; writing to `core` in a new way; reading another T3 table or Claude file.
- **Never:** write to any T3 or Claude Code file; run `gh auth switch` or change `gh`'s setup; finish a task without a trigger; guess at an unknown T3 value; poll a file on a timer.

## Success criteria

- [x] A PR merged on GitHub finishes its task within one pass when no agent on it is working, and after the agent stops otherwise; a task I reopen stays open (`a_pr_merging_finishes_its_task_within_one_pass`, `a_finish_is_held_while_a_thread_runs_and_released_when_it_stops`, `a_task_i_reopen_stays_open`).
- [x] A closed PR reopened on GitHub moves its done task back to `in_review`; T3 alone never does (`closed_then_reopened_goes_back_to_in_review`, `t3_saying_merged_while_github_says_open_changes_nothing`).
- [x] PRs in the second account's private repos are read; a rate limit on one account doesn't stop the other; no token is written anywhere (`the_second_account_is_used_only_after_not_found`, `one_account_rate_limited_while_the_other_keeps_reading`, `no_token_in_argv_tables_logs_or_debug`).
- [ ] A question in a linked T3 thread shows as `needs_me` with its text within 3 s of T3 writing it, seen through kqueue (manual: the kqueue path is tested by `kqueue_reports_every_wal_write_from_another_process_and_rearms`, but the 3 s needs a real thread asking).
- [x] An unknown status, kind or missing key in T3's data gives "Not updating since …" naming it, and the app keeps working (`an_unknown_run_status_is_broken`, `an_unknown_request_kind_or_status_is_broken`, `a_missing_required_key_is_broken`, `a_missing_table_is_broken`).
- [x] Without `gh`, or signed out, PR sync is off with a plain reason and nothing else changes (`without_gh_pr_sync_is_off_and_nothing_else_changes`, `every_account_failing_is_signed_out_and_stays_off`).
- [ ] `thread_url` opens the thread in T3, and `SyncSnapshot.projects` lists T3's projects (manual: each checked by hand once).
- [x] After the Mac sleeps past a deadline, `woke()` runs what's overdue at once (`after_a_six_hour_sleep_woke_runs_the_overdue_pass_at_once`, `after_a_six_hour_sleep_woke_reads_what_changed`). Setting the clock back never delays a pass past two intervals (`a_merge_is_seen_within_one_interval_after_the_clock_is_set_back_an_hour`).
- [ ] Every performance target is met on this Mac, with the machine recorded (manual: the passes, idle CPU, wake-ups and memory are measured in `tasks/sync-todo.md`; the 3 s question and CPU while agents are busy need real T3 threads).

## Checked on this Mac

25 and 26 Nov 2026, all read-only:

- `gh` 2.105.0 at `/opt/homebrew/bin/gh`, two github.com accounts. The query above ran as written (cost 1, limit 5,000), and the second account's token reached a private repo the active one can't. With an empty `GH_CONFIG_DIR`, `gh api` exits 4 and `gh auth status --json hosts` prints `{"hosts":{}}`; that JSON never has a token.
- T3's database: 909 threads, 3 GB, WAL. Every query above and its plan. Value lists from T3's own schema in its app bundle (`OrchestrationV2RunStatus`, `OrchestrationV2RuntimeRequest.kind` and `.status`). Which JSON keys are present on every row, and that `approval_request.prompt` is optional while `questions[].question`, `failure.message` and a thread PR's `url` and `linkedAt` are required.
- **kqueue on a WAL kept open:** a second process held a WAL database open and wrote 8 times; kqueue on the `-wal` reported 17 write events. When it closed, kqueue reported the delete; the folder's write event let the watcher re-arm, and the next writes were reported. (The reviewer verified that FSEvents misses these writes.)
- `notify` 9.0.0's source: the `macos_kqueue` feature, `KqueueWatcher`, its flags, and that it drops a watch on delete or rename.
- `~/.t3/userdata/environment-id` is a UUID plus a newline. T3's bundle registers the `t3code` scheme, and its link parser maps `t3code://threads/<env>/<thread>` to that thread.
- `~/.claude/sessions/*.json` fields; `ps … lstart` in UTC equals `procStart`; two sessions' files disappeared when they ended.

## Not verified yet

- **A pending request's rows.** None was pending while checking; the queries match resolved ones and T3's schema. Whether a pending request's item can lag behind it (hence the 10 s grace).
- **Text for approvals** of kinds other than `command`; only `command` was seen. Without `prompt`, the kind's phrase is shown.
- **Opening `t3code://threads/…` from outside T3** wasn't tried, so as not to change what T3 shows; the link format comes from T3's parser.
- **Opening T3's database read-only while T3 is closed**, if `statev2.sqlite-shm` is missing.
- **`notify` 9.0.0's kqueue backend** in our use (it's five days old); the kqueue behaviour itself was checked with Python's `select.kqueue`.
- **Claude session files:** whether they're rewritten in place or replaced (both are watched). `busy`, `idle` and `waiting` were seen with Claude Code 2.1.332 (26 Nov 2026); others may come.
- **The secondary rate limit's wording**, which wasn't triggered.

## Changes in draft 2

1. **Watching:** kqueue on T3's `-wal` (re-armed when recreated) through `notify`'s `macos_kqueue` feature, replacing FSEvents; a real two-process watcher test.
2. **PRs from T3 threads:** linked only if T3 linked them at or after our thread link, with GitHub's state; only GitHub-read states reach `core`.
3. **PR refs** come canonical from `core`; `sync` builds refs only through `PrRef`.
4. **Finishing** waits until no linked thread is `running` or `needs_me` (`sync_finish_held`), for every task.
5. **Core calls:** `links_of_kind` and `migrate_module`, no fallbacks; `StoreChange.snapshot` instead of extra reads.
6. **Notes:** only failed runs; question and approval notes removed.
7. **`gh`:** health, backoff and rate limit per account; `Secret`; `FakeGh` matches calls, not argv.
8. **T3 strictness:** value lists and required keys from T3's schema; anything else is `Broken`; an `#[ignore]` test against the real database.
9. **Time:** wall-clock deadlines, 15-minute cap, `woke()`, `next_wake > now`; workers are `handle(msg, now)` state machines.
10. **Cost:** Q1, Q2, Q4 and Q5 only for threads on open tasks; wake-ups and idle CPU stated.
11. **`SyncService`:** `find_thread` (Q7), `thread_url`, T3's project list in `SyncSnapshot.projects` (Q8), `chip() -> Option<Chip>` and `Connection` names, matching the app's draft 2.

## Parents are containers (27 Nov 2026)

SPEC-core rule 10 makes a task with steps a container whose status follows them, replacing proposal #50 (the finish check never looked at subtasks, so a parent with a merged PR could finish with steps open).

- The finish check skips a task with steps, and releases any hold on it. A merged PR linked to a parent before it had steps finishes nothing (`a_parent_with_an_old_pr_is_never_finished`).
- A PR T3 lists on a parent's thread isn't linked to the parent; `core`'s `HasSteps` counts as done with, so the ref moves to `pr_seen` and isn't retried (`a_pr_on_a_parents_thread_is_left_for_its_steps`).
