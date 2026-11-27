# Spec: attention

Module `attention` from [CAPABILITIES.md](CAPABILITIES.md). State: **draft 2, approved by the EM on 26 Nov 2026**, plus the Agent stopped kind from the EM's decision on integration QA finding R5 (26 Nov 2026).

It builds on [`core`](SPEC-core.md) v3 and on `SyncService`, and implements `AttentionService`. This spec also holds the **[shared contract](#shared-contract)**: every type and trait in `crates/services`, the one source for that crate's builder, for [SPEC-sync.md](SPEC-sync.md), [SPEC-launch.md](SPEC-launch.md) and the app. The item kinds, their order and their actions come from [DESIGN.md](DESIGN.md) ("Needs me") and its prototype. Changes since draft 1 are [at the end](#changes-in-draft-2), and the Agent stopped addition [after them](#after-integration-qa-agent-stopped).

## Objective

`attention` decides what is waiting on me: the "Needs me" list at the top of the main window, the callouts in a task's panel, and the count on the menu-bar icon. It changes no task. Every item points at a decision, and acting on it (through the app, `core` or `launch`) makes the item go away.

**User stories**

- As me, I open Airboss and see, in a fixed order: questions and approvals from agents, PRs to review, failed runs, agents that stopped without a PR, proposed subtasks, tasks that just became ready, parents whose subtasks are all done, and work quiet for 5+ days.
- As me, I can dismiss an item, or snooze a quiet one for 3 days, and that survives a restart.
- As me, the menu-bar count is always the length of that list, and Quiet items appear on time even if the Mac slept through the moment.

## Decisions

- **A library in the app process**, `crates/attention`, with one worker thread and one thread that waits on `sync`'s `Changes`. Driven by changes, plus one wake-up at the next known deadline (never more than 15 minutes away).
- **One pure function decides everything:** `compute(inputs, hidden, now, config)` returns the items and the next deadline. Its inputs are the latest `StoreChange.snapshot` and `sync.snapshot()`, both `Arc`s. A change costs no `core` read at all.
- **Eight kinds in seven groups:** the design's seven, plus Stopped for Agent stopped (EM, 26 Nov 2026, integration QA R5: a run that ends without a PR otherwise leaves its task `doing` with nothing in Needs me until it turns Quiet), less "Finish it?" (27 Nov 2026: a parent's status follows its steps, SPEC-core rule 10, so there's nothing to finish). Untracked threads aren't a kind; the design has no group for them.
- **Each item's key names one occurrence** (one request, one PR, one quiet stretch). Dismiss hides that key; a new occurrence has a new key and shows. Snooze is a dismissal with an end time.
- **The count** is the number of visible items; the menu bar shows it and the first 6 items.
- **Thresholds** (EM, 25 Nov): quiet after 5 days, snooze for 3 days, "became ready" shown for 7 days, and a PR isn't a review while an agent on its task is still running. Agent stopped waits 10 minutes (EM, 26 Nov).
- **Time** is the wall clock (`SystemTime`). `woke()` recomputes at once, so a deadline passed during sleep shows on wake.

## Tech stack

- Rust edition 2024, Rust 1.100, crate `airboss-attention`. Latest stable on crates.io: `airboss-core` and `airboss-services` (paths), `rusqlite` 0.43.0 `bundled`, `serde` 1.0.229, `thiserror` 2.0.21. **No new crates.** It doesn't depend on the `airboss-sync` crate, only on `SyncService`.
- `core` v3 calls: `snapshot()` once at start (before the first `StoreChange` arrives), `path()` and `migrate_module`. Nothing else.

## Interfaces

```rust
/// Implements airboss_services::AttentionService.
pub struct Attention { /* worker channel, Arc<NeedsList>, Changes */ }
pub fn start(cfg: AttentionConfig, store: SharedStore, sync: Arc<dyn SyncService>, clock: Arc<dyn Clock>)
    -> Result<Arc<Attention>, ServiceError>;

pub struct AttentionConfig {
    pub quiet_after: Duration,   // 5 days
    pub snooze_for: Duration,    // 3 days (snooze with until = None)
    pub ready_window: Duration,  // 7 days
    pub stopped_after: Duration, // 10 minutes: how long a doing task's agents stay stopped, with nothing changing, before Agent stopped
    pub menu_items: usize,       // 6
    pub settle: Duration,        // 150 ms: changes this close together make one recompute
}
impl Default for AttentionConfig { /* the values above */ }

/// Pure. Everything attention shows follows from these.
pub fn compute(input: &Inputs, hidden: &[Hidden], now: SystemTime, cfg: &AttentionConfig) -> Computed;
pub struct Inputs { pub core: Arc<CoreSnapshot>, pub sync: Arc<SyncSnapshot> }
pub struct Hidden { pub key: NeedsKey, pub until: Option<SystemTime> }
pub struct Computed { pub items: Vec<NeedsItem>, pub live_keys: Vec<NeedsKey>, pub next_wake: Option<SystemTime> } // next_wake > now

/// The worker's state machine; tests drive it with chosen times.
pub struct AttentionWorker { /* inputs, hidden rows, own connection, last list */ }
impl AttentionWorker { pub fn handle(&mut self, msg: AttentionMsg, now: SystemTime) -> Step; }
pub enum AttentionMsg { Store(Arc<CoreSnapshot>), Sync, Hide(NeedsKey, Option<SystemTime>, Reply), Restore(NeedsKey, Reply),
                        Recheck, Woke, Tick, Shutdown }
```

**Trait methods:**

| Method | What `Attention` does |
|---|---|
| `connections()` | Empty: attention depends on nothing outside |
| `recheck()`, `woke()` | Queue a recompute with the wall clock; return at once |
| `changes()` | Bumped when the visible list changes |
| `store_changed(c)` | Queues `c.snapshot` (an `Arc` clone); returns at once. Events aren't needed |
| `shutdown()` | Stops both threads (at most 1 s) |
| `needs_me()` | The current `Arc<NeedsList>`; no I/O |
| `for_task(task)` | Items whose `task` is this one, in list order |
| `dismiss`, `snooze`, `restore` | Send to the worker and wait for its reply (one write, then a recompute); under 20 ms |

## Data / SQL

Migration `attention` version 1, run with `migrate_module("attention", …)` on its own connection at `Store::path()`. Loaded into an in-memory SQLite on 25 Nov 2026; each CHECK rejected its bad row.

```sql
CREATE TABLE attention_hidden (           -- one dismissed or snoozed item
  item_key  TEXT PRIMARY KEY CHECK (length(item_key) > 0),
  task_id   INTEGER NOT NULL,             -- no foreign key: core owns tasks
  kind      TEXT NOT NULL CHECK (kind IN ('question','approval','review','failed_run',
                                          'proposed','ready','finish','quiet')),   -- v2 adds 'stopped'
  action    TEXT NOT NULL CHECK (action IN ('dismissed','snoozed')),
  until     TEXT,                         -- set exactly for snoozes
  hidden_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  CHECK ((action = 'snoozed') = (until IS NOT NULL))
);
CREATE INDEX attention_hidden_task_idx ON attention_hidden(task_id);
```

Version 2 adds the kind `'stopped'` (Agent stopped) to the CHECK. SQLite can't change a CHECK in place, so it creates `attention_hidden_v2` with the new list, copies every row, drops the old table, renames the new one and creates the index again (`src/migrations/002_agent_stopped.sql`). A test opens a version 1 file with a row in it and checks the row survives.

An item is hidden while a row with its key exists and `until` is null or later than now. A row whose key hasn't been live for 30 days is deleted, at most once a day during a recompute.

## Behaviour

### Inputs

From `CoreSnapshot`: `tree` (statuses, titles, `last_event_at`, `subtree_last_event_at`; open proposals are the nodes with status `proposed`), `edges` (`(task, waits_on)` pairs, finished blockers included) and `ready` (`ReadyTask { task, since }`). From `SyncSnapshot`: thread and PR views, `thread_tasks`, `pr_tasks`, `checked_at`. "Open" means `todo`, `doing`, `in_review` or `blocked`.

### The kinds

| Kind (group) | Shown when | Key | `since` | Headline · context | Main action |
|---|---|---|---|---|---|
| Question (Questions) | a linked thread is `NeedsMe` with a `Question` request, and its task is open | `question:<request id>` | request time | the question · "Agent in {thread title} asks" | Open thread |
| Approval (Questions) | the same, with an `Approval` request | `approval:<request id>` | request time | "Allow: {first line of the text}" · "Agent in {thread title} wants approval" | Open thread |
| Review (Reviews) | a linked PR is `open`, not a draft, without a problem; its task is open; none of that task's threads is `Running` or `NeedsMe` | `review:<pr ref>` | `ready_since` | "PR #{n} · {title}" · "+{add} −{del} · checks {passing/failing/pending} · {approved/changes requested/review required}" | Open PR |
| Failed run (Failed runs) | a linked T3 thread is `Errored`, and its task is open | `failed:<run id>` | the thread's `last_activity_at` | "{thread title} stopped: {error}" · "Thread stopped · {error}" | Open thread |
| Agent stopped (Stopped) | the task is `doing` and has no steps (a parent shows `doing` because a step is, SPEC-core rule 10); it has at least one linked thread; every one is `Idle`, `Settled` or `Gone`; no linked PR is open (or of unknown state); and `stopped_after` (10 min) has passed since the last change | `stopped:<task id>:<last change>` | the last change: the newest of the task's own `last_event_at` and its threads' `last_activity_at` | "Agent stopped · no PR yet" (or "· no open PR" when its only PRs are closed or merged) · "{thread title} stopped", or "All {n} threads stopped" | Open thread (the most recently active one) |
| Proposed (Proposed) | tree nodes with status `proposed`: one item per parent; a top-level proposal is its own item | `proposed:<parent id>:<largest proposed id>`, or `proposed:<id>` | newest `created_at` in the group | "The agent proposed {n} subtasks" · "{n} proposed subtasks" | Accept all, Reject all, or each |
| Ready (Ready) | in `ready`, with at least one edge `(task, _)` (so every blocker finished), and `since` within `ready_window` | `ready:<task id>:<since>` | `since` | "Ready to start: {ids} {is/are} done" · "{ids} done · ready to start" | Do it |
| Quiet (Quiet) | `doing`, `in_review` or `blocked`; quiet for `quiet_after` or longer; no ancestor shown as quiet | `quiet:<task id>:<activity>` | last activity | "No activity for {d} days." plus " {n} open PRs." if any · "Quiet for {d} days" | Open, or Snooze 3 days |

- **One item per request or PR.** A thread or PR linked to several open tasks gives one item, at the lowest task id; `for_task` shows it as a callout on each of those tasks.
- **No "Finish it?".** A parent shows what its steps add up to (SPEC-core rule 10), so it's done when they are and nothing asks to finish it. Rows hidden under the old `finish` kind are skipped when read.
- **Ready names its blockers** from `edges`, in id order, up to two ("143 and 145 are done"); more read "3 tasks it waited on are done". If one blocker has a merged PR in `pr_tasks`, the context reads "143 merged · ready to start".
- **Last activity**, for Quiet, is the latest across the task and every task under it of: `subtree_last_event_at` (which includes `sync`'s failed-run notes); each linked thread's `last_activity_at`, where a `Running` or `NeedsMe` thread counts as now; and each linked PR's `updated_at`. One bottom-up pass over `tree`; Quiet is then checked top-down, so a quiet parent hides its quiet subtasks.
- **Agent stopped** is one item per `doing` task, not per thread, because its actions (Mark done, Set in review) act on the task. A thread linked to two `doing` tasks gives each its own item. It clears when any condition stops holding: an agent runs or asks again, a thread errors (a Failed run shows instead), a PR opens, or the task leaves `doing`. A thread `sync` has no view of yet counts as not stopped, and so does a PR whose state isn't known yet, so nothing shows before `sync`'s first read. The task's own `last_event_at` counts, not its subtree's: a subtask's change isn't a change to this task. A new stop after the agent was resumed has a newer last change, so a new key, and shows again after a dismiss.
- **Quiet gives way to Agent stopped:** a Quiet item isn't shown for a task with a visible Agent stopped item, so one task never has both. Once Agent stopped is dismissed or snoozed, Quiet can show for it again.
- **`stale`** is set on an item built from a `ThreadView` or `PrView` marked `stale`.

### Order

Groups in the design's order, with Stopped right after Failed runs (both are a run that ended): Questions, Reviews, Failed runs, Stopped, Proposed, Ready, Quiet. Inside a group, newest `since` first; in Quiet, longest quiet first. Ties by task id, then key. `for_task` uses the same order.

### What each action calls

| Action | Call |
|---|---|
| Open thread / Open PR | the app opens `sync.thread_url(key)` / the PR's `url` |
| Accept, Accept all | `set_status(id, Todo, None, me)` per task |
| Reject, Reject all | `set_status(id, Dropped, reason, me)`; `core` records a `rejected` event |
| Do it | `launch.start(&StartRequest { task, mode: DoIt, place: TaskRepos, again: false })` |
| Mark done | `set_status(parent, Done, None, me)`; for Agent stopped, the task itself |
| Set in review (Agent stopped) | `set_status(task, InReview, None, me)` |
| Keep open, Dismiss (⌘⌫) | `dismiss(key)` |
| Snooze 3 days (Quiet, Agent stopped) | `snooze(key, None)` |
| Undo | `restore(key)` |

### The count and "checked"

`NeedsList.count` is `items.len()`. `checked_at` is `sync.snapshot().checked_at`. `version` goes up by one every time the visible list changes (keys, `since`, headline, context or `stale`), and `changes()` is bumped with it.

### Staying cheap, and on time

| Message | Reads | Then |
|---|---|---|
| `Store(snapshot)` | nothing: the snapshot came with the change | recompute |
| `Sync` (from the `Changes` waiter) | `sync.snapshot()`: an `Arc` clone | recompute |
| `Hide`, `Restore` | its own table: one write | recompute, then reply |
| `Recheck`, `Woke`, `Tick` | nothing | recompute with the wall clock |

- Messages within `settle` (150 ms) of each other make one recompute.
- `next_wake` is the earliest of: the next task turning quiet (its last activity plus `quiet_after`), a Ready item leaving its window, a snooze ending, and a `doing` task whose agents have stopped reaching `stopped_after` since its last change. `compute` asserts it's later than `now`; anything already due is in the list. The worker waits with `recv_timeout(min(next_wake − now, 15 min))`, so after a sleep the first `Tick` or `woke()` puts a newly quiet task in the list.
- The `Changes` waiter loops on `sync.changes().wait_past(seen, 15 min)` and sends `Sync` when the count moved.
- **Wake-ups:** on each core or sync change (coalesced), at deadlines, and at most 4 an hour otherwise. **Idle CPU:** 0%; each wake-up is one `compute` (under 5 ms at 2,000 tasks).

## Errors and degraded modes

| Case | Behaviour |
|---|---|
| A `sync` source broken or off | Its items keep their last known data with `stale: true`; the app may dim them. Nothing is invented or dropped because a source is down |
| `snapshot()` fails at start | Start with an empty list and wait for the first `StoreChange`; log once |
| `attention_hidden` can't be read | Hide nothing and log. Showing too much beats hiding a question |
| `dismiss` or `snooze` can't be saved | `ServiceError { code: Storage }`; the item stays |
| `restore` of a key that isn't hidden | `ServiceError { code: NotFound }` |
| A panic in `handle` | Caught and logged; the previous list stays; the worker keeps running |

## Performance targets

| What | Target |
|---|---|
| `compute` for 2,000 tasks, 1,000 edges, 50 linked threads, 100 PRs, 200 hidden rows | under 5 ms (95th percentile) |
| `core` reads per change | none |
| A change reaching `attention` to `changes()` bumped | under 200 ms, `settle` included |
| `dismiss` to updated list | under 20 ms |
| Wake-ups with nothing changing | at most 4 an hour |
| Memory | under 5 MB |

## Testing

Tests use a `FakeSync` (a `SyncService` returning chosen snapshots and bumping `Changes` on demand), `CoreSnapshot`s built by hand, and `FakeClock`. They never touch `gh`, `t3` or T3's database.

- **`tests/kinds.rs`** calls `compute` directly: one test per kind showing, and one per rule hiding it (a question on a done task, a draft PR, a PR while its agent runs, a ready task with no edges, readiness older than the window, a parent whose steps are all done, a parent in progress through its steps never "stopped", a quiet child under a quiet parent); the lowest-task-id rule; Ready naming blockers from `edges` and a merged PR.
- **`tests/order.rs`:** group order, newest first, Quiet longest first, ties.
- **`tests/stopped.rs`** (Agent stopped): a table of one case per rule (each stopped state, too early by 1 ms, a thread still running or waiting, one errored, no thread, an open, draft or unknown PR, only a closed PR, each other status); the last change counted from the task and every thread, not subtasks; `next_wake` at the 10 minutes and the config's value; one item per `doing` task; the order after Failed runs; Quiet giving way; a new stop being a new key; the worker's `Tick` at the deadline and `Woke` after a sleep.
- **`tests/time.rs`:** exactly 5 days; `next_wake` equals last activity plus 5 days and is later than `now`; a sleep of 6 days, then `Woke`, puts the task in Quiet at once; a snooze ending; the 15-minute cap with nothing due.
- **`tests/hidden.rs`** (temporary file): dismiss hides until the key changes; snooze until `until`; `restore`; rows survive reopening; pruning after 30 days.
- **`tests/worker.rs`** drives `AttentionWorker::handle(msg, now)`: a `Store` message makes no `core` call (the store handle panics if touched after start); a burst of 20 messages makes one recompute; `version` and `changes()` move only on a visible change.
- **`tests/migrations.rs`:** each CHECK; opening twice applies once; version 2 keeps version 1's rows and accepts `stopped`.
- **Performance** (`#[ignore]`): `compute` on a generated tree of 2,000 tasks, with the machine recorded.

## Project structure

```
crates/attention/
  Cargo.toml                 airboss-attention
  src/lib.rs                 Attention (AttentionService), start, AttentionConfig
  src/compute.rs  src/kinds.rs  src/quiet.rs   compute; one function per kind; last activity per subtree
  src/stopped.rs             Agent stopped
  src/hidden.rs  src/db.rs   attention_hidden; own connection and migrations (src/migrations/001_attention.sql, 002_agent_stopped.sql)
  src/worker.rs              AttentionWorker::handle, the thread loop, the Changes waiter
  tests/…                    as listed above
```

## Boundaries

- **Always:** derive items only from the snapshots and `attention_hidden`; keep `compute` pure; recompute only on a message or at `next_wake`.
- **Ask first:** a new kind or group (the design fixes seven groups, Stopped included, "Finish it?" gone); changing a default threshold; a new crate.
- **Never:** change a task, link or status; read `core` after start; run `gh` or `t3` or read T3's files; poll; show untracked threads as items.

## Success criteria

- [x] Every kind shows from realistic inputs, and each hiding rule has a test (`tests/kinds.rs`, e.g. `a_question_in_a_linked_thread_shows`, `a_question_on_a_closed_task_is_hidden`; `tests/hidden.rs`).
- [x] The list follows the design's group order, and the count equals its length (`groups_come_in_the_designs_order`, `the_order_never_depends_on_hash_order`).
- [x] Dismiss and snooze survive a restart; a new occurrence shows again (`dismiss_and_snooze_survive_a_restart`, `dismiss_hides_an_item_until_a_new_occurrence`).
- [ ] A question asked in a linked thread is in Needs me within 3 s of T3 writing it, and leaves when it's answered (manual: needs a real thread asking; `a_question_in_a_linked_thread_shows` covers the logic).
- [x] A `doing` task whose agents all stopped with no PR shows as Agent stopped 10 minutes after the last change, on time after a sleep, and leaves when an agent runs, a PR opens or the task moves on. (`a_doing_task_whose_agent_stopped_with_no_pr_shows`, `quiet_gives_way_to_a_visible_agent_stopped_item_on_the_same_task`, the sleep tests in `tests/worker.rs`).
- [x] A task with no activity for 5 days shows as Quiet on time, including after the Mac slept through the moment; its quiet subtasks don't (`exactly_five_days_is_quiet`, `after_a_long_sleep_the_quiet_task_is_there_at_once`, `a_quiet_parent_hides_its_quiet_subtasks`).
- [x] A change costs no `core` read, and `compute` meets 5 ms for 2,000 tasks on this Mac (`a_store_change_needs_no_core_read`, `perf_compute_at_2000_tasks`: 0.61 ms in the final audit).

## Shared contract

`crates/services` (`airboss-services`): types and traits only. It depends on `airboss-core` (ids, `Repo`, `PrState`, `Event`, `CoreSnapshot`) and `serde`, uses `std` only (no tokio, no threads), and has no logic beyond `Changes`, `PrRef`, `ThreadState::chip`, `NeedsKind::group`, the `SyncSnapshot` helpers, `time` and `log` (integration QA R1: `log::line(source, message)` goes to stderr until the app calls `log::set_sink`, then to the app's log file; every service logs through it). The value types derive `Clone`, `Debug` and `Serialize`; `SyncSnapshot` derives `Clone` and `Debug` only. Field names are the ones the app spec's draft 2 reads.

```rust
pub type SharedStore = Arc<Mutex<airboss_core::Store>>;
pub type Timestamp = String;                     // UTC text in core's format, "2026-11-25T17:19:07.841Z"
pub mod time { pub fn to_ts(t: SystemTime) -> Timestamp; pub fn from_ts(ts: &str) -> Option<SystemTime>; }
pub mod log { pub type Sink = fn(&str, &str); pub fn set_sink(s: Sink) -> bool; pub fn has_sink() -> bool; pub fn line(source: &str, message: &str); }
pub trait Clock: Send + Sync { fn now(&self) -> SystemTime; }
pub struct SystemClock;                          // impl Clock: SystemTime::now()

/// A change counter: bump() adds one and wakes every waiter.
#[derive(Clone, Default)] pub struct Changes(Arc<(Mutex<u64>, Condvar)>);
impl Changes {
    pub fn new() -> Changes;
    pub fn bump(&self);
    pub fn current(&self) -> u64;
    pub fn wait_past(&self, seen: u64, timeout: Duration) -> u64;  // returns once the count > seen, or at timeout
}

pub struct StoreChange { pub events: Vec<Event>, pub snapshot: Arc<CoreSnapshot> }

/// Every service. store_changed, recheck, refresh and woke queue work and return at once;
/// connections, changes and the snapshot getters read memory only.
pub trait Service: Send + Sync {
    fn connections(&self) -> Vec<Connection>;
    fn recheck(&self);
    fn changes(&self) -> Changes;
    fn store_changed(&self, c: &StoreChange);
    fn woke(&self);
    fn shutdown(&self);
}
/// One line of the connections popover. Rows with the same `name` from several services are one row:
/// ok when all are ok; `state`, `detail` and `fix` from the first that isn't.
pub struct Connection { pub name: String, pub ok: bool, pub state: String, pub detail: String, pub fix: Option<String> }

pub struct ServiceError { pub code: ErrorCode, pub message: String }   // message: one sentence for the user
pub enum ErrorCode {
    // launch
    T3Missing, T3NotRunning, T3Unreachable, T3TooOld, Timeout, NotAProject, RepoMissing, NoBaseBranch,
    BadBranch, TaskClosed, TaskProposed, NeedsChoice, Waiting, AlreadyStarting, BadAnswer, T3Failed,
    // sync
    GhMissing, SignedOut, NoAccess, PrNotFound, BadPrRef, RateLimited, GhFailed, T3Unavailable,
    // any service
    NotFound, Core, Storage, Internal, ShuttingDown,
}

// ---------- sync ----------
pub trait SyncService: Service {
    fn snapshot(&self) -> Arc<SyncSnapshot>;
    fn refresh(&self, what: Refresh);
    fn lookup_pr(&self, pr: &PrRef) -> Result<PrView, ServiceError>;
    fn find_thread(&self, project: &Path, branch: &str) -> Option<ThreadKey>;
    fn thread_url(&self, key: &ThreadKey) -> Option<String>;     // t3code://threads/<environment>/<thread>
}
pub enum Refresh { All, Prs, PrsOf(TaskId), T3 }
pub struct SyncSnapshot {
    pub threads: HashMap<ThreadKey, ThreadView>,      // linked and untracked
    pub prs: HashMap<PrRef, PrView>,                  // linked, and my open PRs
    pub thread_tasks: HashMap<ThreadKey, Vec<TaskId>>,
    pub pr_tasks: HashMap<PrRef, Vec<TaskId>>,
    pub projects: Vec<T3Project>,                     // T3's projects, for first run's "Add repos"
    pub checked_at: Option<Timestamp>,                // the oldest last-good read among sources that are ok
}
impl SyncSnapshot {
    pub fn threads_for(&self, task: TaskId) -> Vec<&ThreadView>;
    pub fn prs_for(&self, task: TaskId) -> Vec<&PrView>;
    pub fn untracked(&self) -> Vec<&ThreadView>;           // newest activity first
    pub fn my_open_prs(&self) -> Vec<&PrView>;             // open, mine, not linked
    pub fn suggested_prs(&self, branch: &str) -> Vec<&PrView>;  // my open PRs from this branch
}
pub struct T3Project { pub id: String, pub name: String, pub path: PathBuf }
#[derive(PartialEq, Eq, Hash)] pub struct ThreadKey { pub source: Source, pub r#ref: String }
pub enum Source { T3, Claude }
pub struct ThreadView {
    pub key: ThreadKey, pub linked: bool, pub state: ThreadState, pub title: Option<String>,
    pub request: Option<Request>, pub error: Option<String>, pub run_id: Option<String>,
    pub project_path: Option<String>, pub branch: Option<String>, pub worktree_path: Option<String>,
    pub parent_ref: Option<String>, pub child_count: u32, pub last_activity_at: Option<Timestamp>, pub stale: bool,
}
pub enum ThreadState { Running, NeedsMe, Errored, Idle, Settled, Gone }
impl ThreadState { pub fn chip(self) -> Option<Chip>; }  // Running: Working; NeedsMe: WaitingOnYou;
                                                         // Idle: YourTurn; Errored, Settled: Stopped; Gone: None
pub enum Chip { Working, WaitingOnYou, YourTurn, Stopped }
pub struct Request { pub id: String, pub kind: RequestKind, pub text: String, pub at: Timestamp }
pub enum RequestKind { Question, Approval }
/// A PR ref in core's canonical form, "owner/repo#n" in lowercase.
#[derive(PartialEq, Eq, Hash)] pub struct PrRef(String);
impl PrRef {
    pub fn parse(input: &str) -> Option<PrRef>;          // "Owner/Repo#12" or "https://github.com/Owner/Repo/pull/12[/…][?…][#…]"
    pub fn new(owner_repo: &str, number: u64) -> Option<PrRef>;   // "Owner/Repo", 12
    pub fn as_str(&self) -> &str;
    pub fn parts(&self) -> (&str, &str, u64);            // owner, repo, number
}
pub struct PrView {
    pub r#ref: PrRef, pub linked: bool, pub url: Option<String>, pub title: Option<String>,
    pub state: Option<PrState>, pub draft: bool, pub review: Review, pub checks: Checks,
    pub additions: Option<u32>, pub deletions: Option<u32>, pub head_ref: Option<String>, pub author: Option<String>,
    pub updated_at: Option<Timestamp>, pub ready_since: Option<Timestamp>, pub problem: Option<PrProblem>, pub stale: bool,
}
pub enum Review { Approved, ChangesRequested, Required, None }
pub enum Checks { Passing, Failing, Pending, None }
pub enum PrProblem { NotFound, NoAccess, Failed(String) }

// ---------- launch ----------
pub trait LaunchService: Service {
    fn availability(&self) -> Availability;
    fn prepare(&self, task: TaskId, mode: Mode) -> Result<Prepared, ServiceError>;
    fn start(&self, req: &StartRequest) -> Result<LaunchReport, ServiceError>;
    fn add_to_t3(&self, repo: RepoId) -> Result<(), ServiceError>;
}
pub enum Availability { Unknown, Ready { version: String }, Missing { reason: String } }
pub enum Mode { DoIt, PlanIt }
pub enum Place { TaskRepos, NoRepo(PathBuf) }
pub struct StartRequest { pub task: TaskId, pub mode: Mode, pub place: Place, pub again: bool }
pub struct Prepared {
    pub repos: Vec<Repo>, pub from_subtasks: bool, pub needs_choice: bool, pub choices: Vec<Repo>,
    pub branch: Option<String>, pub research_dir: PathBuf, pub waiting_on: Vec<TaskId>,
    pub started: Vec<StartedThread>,
}
pub struct LaunchReport { pub started: Vec<StartedThread>, pub skipped: Vec<RepoId>,
                          pub failed: Vec<(Option<RepoId>, ServiceError)>, pub branch: Option<String>,
                          pub moved_to_doing: bool }
pub struct StartedThread { pub key: ThreadKey, pub mode: Mode, pub repo: Option<RepoId>, pub title: String, pub linked: bool }

// ---------- attention ----------
pub trait AttentionService: Service {
    fn needs_me(&self) -> Arc<NeedsList>;
    fn for_task(&self, task: TaskId) -> Vec<NeedsItem>;
    fn dismiss(&self, k: &NeedsKey) -> Result<(), ServiceError>;
    fn snooze(&self, k: &NeedsKey, until: Option<Timestamp>) -> Result<(), ServiceError>;
    fn restore(&self, k: &NeedsKey) -> Result<(), ServiceError>;
}
pub struct NeedsList { pub items: Vec<NeedsItem>, pub count: u32, pub checked_at: Option<Timestamp>, pub version: u64 }
#[derive(PartialEq, Eq, Hash)] pub struct NeedsKey(pub String);   // e.g. "question:<request id>"; stable across restarts
pub struct NeedsItem {
    pub key: NeedsKey, pub kind: NeedsKind, pub group: NeedsGroup, pub task: TaskId, pub task_title: String,
    pub since: Timestamp, pub headline: String, pub context: String, pub target: Target,
    pub proposed: Vec<(TaskId, String)>,                // Proposed only
    pub stale: bool,
}
pub enum NeedsKind { Question, Approval, Review, FailedRun, AgentStopped, Proposed, Ready, Quiet }
impl NeedsKind { pub fn group(self) -> NeedsGroup; }
pub enum NeedsGroup { Questions, Reviews, FailedRuns, Stopped, Proposed, Ready, Quiet }   // display order
pub enum Target { Thread(ThreadKey), Pr { r#ref: PrRef, url: Option<String> }, Task(TaskId) }
```

- **`PrRef::parse` follows `core` v3's rule** (owner and repo of `[A-Za-z0-9_.-]+`, a positive number, lowercased). A contract test links a table of inputs through `core` and checks `PrRef::parse(input).as_str()` equals the ref `core` stored.
- **`time`** formats with milliseconds and `Z`, so text order is time order; `from_ts` also accepts GitHub's `…:08Z`.
- **Serialisation:** enums serialise as snake_case strings; `PrRef`, `NeedsKey` and `Timestamp` as plain strings.

## Not verified yet

- **`core` v3's exact names and shapes** (`CoreSnapshot.edges` as `(task, waits_on)`, `ReadyTask.since`, `subtree_last_event_at`). It's being built now; this spec uses the names in the coordinator's brief. If they differ, change `compute`'s reads, not the behaviour.
- **"The oldest open PR from 4 Nov"** in the prototype's Quiet text needs a PR's creation date, which `sync` doesn't read; v1 says "{n} open PRs".
- **Two rows named "T3 Code"** (from `launch` and `sync`) merging into one is a rule of this contract; the app spec's draft 2 reads `name` but doesn't state the merge yet.

## Changes in draft 2

1. **`AttentionService`:** `Attention` and `start(…)`; `NeedsList` with `count`, `checked_at` and `version`; `NeedsItem.since` and `group`; `Target` kept; `snooze(key, Option<Timestamp>)`.
2. **Cost:** inputs are `StoreChange.snapshot` and `sync.snapshot()`; no `tree()`, `ready()`, `proposed()` or `dependencies()` calls after start. Ready's blockers come from `edges`.
3. **Time:** wall-clock deadlines, `next_wake > now` asserted, a 15-minute cap, `woke()`; Quiet appears on time after a sleep.
4. **Testability:** `AttentionWorker::handle(msg, now)`; `FakeSync` instead of `sync`'s fakes; no dependency on the `airboss-sync` crate.
5. **Shared contract** added here: every type in `crates/services`, with the field names the app's draft 2 reads.

## After integration QA: Agent stopped

The EM's decision on finding R5 (26 Nov 2026), in `tasks/program.md`:

1. **A new kind, Agent stopped**, in a new group, Stopped, right after Failed runs. `NeedsKind::AgentStopped` serialises as `agent_stopped`, `NeedsGroup::Stopped` as `stopped`, and its hidden rows' kind is `stopped`.
2. **`AttentionConfig.stopped_after`**, 10 minutes by default, and a deadline in `next_wake` when it runs out.
3. **Migration `attention` version 2** adds `'stopped'` to the kind CHECK, keeping every row.
4. **Quiet gives way** to a visible Agent stopped item on the same task.
5. **Actions:** Open thread (main), Mark done and Set in review (as me), Snooze 3 days and Dismiss.

## Parents are containers: no "Finish it?" (27 Nov 2026)

SPEC-core rule 10 (task 53, agreed with the user on 27 Nov 2026): a task with steps has no status of its own; it's done when its steps are. So:

1. **The Finish it? kind and group are gone** (`NeedsKind::FinishIt`, `NeedsGroup::FinishIt`, `kinds::finish`). The migrations keep `'finish'` in the CHECK, since they're append-only; a hidden row of that kind is skipped when read, like any unknown kind.
2. **Agent stopped skips parents.** A parent shows `doing` while a step is, and a thread linked to it from before it had steps would otherwise ask to finish something only its steps finish.
