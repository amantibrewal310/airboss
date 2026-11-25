# Spec: attention

Module `attention` from [CAPABILITIES.md](CAPABILITIES.md). State: **draft 1, waiting for review**.

It builds on [`core`](SPEC-core.md), including the v2 additions, and on [`sync`](SPEC-sync.md). The app shows what it computes. The item kinds, their order and their actions come from [DESIGN.md](DESIGN.md) ("Needs me") and its prototype.

## Objective

`attention` decides what is waiting on me: the "Needs me" list at the top of the main window, the callouts in a task's detail panel, and the count on the menu-bar icon. It changes nothing itself. Every item points at a decision, and acting on it (through the app, `core` or `launch`) makes the item go away.

**User stories**

- As me, I open Airboss and see, in a fixed order: questions and approvals from agents, PRs to review, failed runs, proposed subtasks, tasks that just became ready, parents whose subtasks are all done, and work that has gone quiet for 5+ days.
- As me, I can dismiss an item, or snooze a quiet one for 3 days, and that survives a restart.
- As me, the menu-bar count is always the length of that list, and the list updates within a moment of anything changing.

## Decisions

- **A library inside the app process**, `crates/attention`, with one worker thread. It's driven by events, never by a polling timer.
- **One pure function decides everything.** `compute(inputs, hidden, now, config)` returns the items and the next time anything time-based can change. The worker caches the inputs, re-reads only those that changed, and calls `compute` again.
- **Eight kinds in the design's seven groups.** Untracked threads aren't a kind, because the design has no group for them. `sync` lists them for the app.
- **Each item has a key that names one occurrence**, such as one request, one PR, or one quiet stretch. Dismissing hides that key. A new occurrence gets a new key and shows again.
- **Snooze** is a dismissal with an end time. The design offers it on Quiet ("Snooze 3 days"), but the API accepts it for any item.
- **The count** is the number of visible items. The menu bar shows the count and the first 6 items.
- **Thresholds** are 5 days for quiet and 3 days for snooze (both from the design), and a 7-day window for "became ready" (my choice: older readiness isn't news). They're fields of `AttentionConfig`. The app may persist overrides; there's no UI for them in v1.
- **Reviews wait for the agent.** An open PR isn't shown as a review while one of its task's threads is still running or waiting on me, because the agent isn't finished with it.

## Tech stack

- Rust edition 2024, Rust 1.100, crate `airboss-attention`.
- Dependencies: `airboss-core` and `airboss-sync` (paths), `rusqlite` 0.43.0 with `bundled`, `serde` 1.0.229 and `thiserror` 2.0.21. These are the latest stable on crates.io as of 25 Nov 2026. **No new crates.** Time handling uses `airboss_sync::time`; tests use `airboss-sync` with its `fake` feature.
- **Uses from `core` v2:** `tree()` with `TaskNode.last_event_at` and `subtree_last_event_at`; `ready() -> [{task, since}]`; `dependencies(id)`; `proposed()`; and a public `migrate` (if it isn't public, copy it, as in `sync`).

## Interfaces

```rust
pub type SharedStore = Arc<Mutex<airboss_core::Store>>;

pub struct AttentionConfig {
    pub db_path: PathBuf,        // airboss.db
    pub quiet_after: Duration,   // 5 days
    pub snooze_for: Duration,    // 3 days
    pub ready_window: Duration,  // 7 days
    pub menu_items: usize,       // 6
    pub settle: Duration,        // 150 ms: changes this close together make one recompute
}
impl AttentionConfig { pub fn defaults(db_path: PathBuf) -> AttentionConfig; }

pub fn start(cfg: AttentionConfig, core: Box<dyn CoreReads>, sync: airboss_sync::SyncHandle,
             clock: Arc<dyn airboss_sync::Clock>) -> Result<AttentionHandle, AttentionError>;

#[derive(Clone)]
pub struct AttentionHandle { /* channel to the worker, shared item list */ }
impl AttentionHandle {
    pub fn items(&self) -> Arc<Vec<Item>>;                 // in display order
    pub fn summary(&self) -> Summary;
    pub fn for_task(&self, task: TaskId) -> Vec<Item>;     // the detail panel's callouts
    pub fn subscribe(&self) -> Receiver<Summary>;          // sent only when the list changes
    pub fn on_core_events(&self, events: &[airboss_core::Event]);  // the app forwards every core change
    pub fn dismiss(&self, key: &ItemKey) -> Result<(), AttentionError>;   // returns once saved
    pub fn snooze(&self, key: &ItemKey, until: Option<String>) -> Result<(), AttentionError>; // None: now + snooze_for
    pub fn restore(&self, key: &ItemKey) -> Result<(), AttentionError>;   // undo either
    pub fn shutdown(&self);
}

pub struct Summary { pub count: usize, pub top: Vec<Item>, pub checked_at: Option<String> }

pub struct Item {
    pub key: ItemKey, pub kind: Kind, pub group: Group,
    pub task: TaskId, pub task_title: String,
    pub since: String,                 // UTC text; the app shows "6m"
    pub headline: String,              // the 17px line on the card
    pub context: String,               // the one line under the title in the list
    pub target: Target,                // what the main action opens
    pub proposed: Vec<(TaskId, String)>,  // Proposed only
    pub stale: bool,                   // from a sync source that is broken or off
}
#[derive(Clone, PartialEq, Eq, Hash)] pub struct ItemKey(pub String);
pub enum Kind { Question, Approval, Review, FailedRun, Proposed, Ready, FinishIt, Quiet }
pub enum Group { Questions, Reviews, FailedRuns, Proposed, Ready, FinishIt, Quiet }  // display order
pub enum Target { Thread(airboss_sync::ThreadKey), Pr { r#ref: String, url: Option<String> }, Task(TaskId) }

/// Everything attention shows follows from these. Pure; tests call it directly.
pub fn compute(input: &Inputs, hidden: &[Hidden], now: &str, cfg: &AttentionConfig) -> Computed;
pub struct Inputs {
    pub nodes: Vec<TaskNode>, pub ready: Vec<ReadyTask>, pub blockers: HashMap<TaskId, Vec<Task>>,
    pub proposed: Vec<Task>, pub sync: Arc<airboss_sync::Snapshot>,
}
pub struct Hidden { pub key: ItemKey, pub until: Option<String> }
pub struct Computed { pub items: Vec<Item>, pub live_keys: Vec<ItemKey>, pub next_wake: Option<String> }

/// The core reads attention makes, behind a trait so tests can count them.
pub trait CoreReads: Send {
    fn tree(&mut self) -> Result<Vec<TaskNode>, core::Error>;
    fn ready(&mut self) -> Result<Vec<ReadyTask>, core::Error>;
    fn proposed(&mut self) -> Result<Vec<Task>, core::Error>;
    fn blockers(&mut self, id: TaskId) -> Result<Vec<Task>, core::Error>; // dependencies(id).waits_on
}
impl CoreReads for SharedStore { /* locks per call */ }
```

## Data / SQL

Migration `attention` version 1, recorded in `schema_migrations` as `('attention', 1)`. Loaded into an in-memory SQLite on 25 Nov 2026; each CHECK rejected its bad row.

```sql
CREATE TABLE attention_hidden (           -- one dismissed or snoozed item
  item_key  TEXT PRIMARY KEY CHECK (length(item_key) > 0),
  task_id   INTEGER NOT NULL,             -- no foreign key: core owns tasks
  kind      TEXT NOT NULL CHECK (kind IN ('question','approval','review','failed_run',
                                          'proposed','ready','finish','quiet')),
  action    TEXT NOT NULL CHECK (action IN ('dismissed','snoozed')),
  until     TEXT,                         -- set exactly for snoozes
  hidden_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  CHECK ((action = 'snoozed') = (until IS NOT NULL))
);
CREATE INDEX attention_hidden_task_idx ON attention_hidden(task_id);
```

An item is hidden while a row with its key exists and `until` is null or still in the future. A row whose key hasn't been live for 30 days is deleted, checked at most once a day during a recompute.

## Behaviour

### The kinds

"Open" below means `todo`, `doing`, `in_review` or `blocked`. "Linked" means through a `t3_thread`, `claude_session` or `pr` link, as in `sync`'s `thread_tasks` and `pr_tasks`.

| Kind (group) | Shown when | Key | `since` | Headline · context | Main action |
|---|---|---|---|---|---|
| Question (Questions) | a linked thread is `needs_me` with a `Question` request, and its task is open | `question:<request id>` | request time | the question · "Agent in {thread title} asks" | Open thread |
| Approval (Questions) | the same, with an `Approval` request | `approval:<request id>` | request time | "Allow: {first line of the command}" · "Agent in {thread title} wants approval" | Open thread |
| Review (Reviews) | a linked PR is open, not a draft and readable; its task is open; none of that task's threads is `running` or `needs_me` | `review:<pr ref>` | `ready_since` | "PR #{n} · {title}" · "+{add} −{del} · checks {passing/failing/pending} · {approved/changes requested/review required}" | Open PR |
| Failed run (Failed runs) | a linked T3 thread is `errored`, and its task is open | `failed:<run id>` | the thread's `last_activity_at` | "{thread title} stopped: {error}" · "Thread stopped · {error}" | Open thread |
| Proposed (Proposed) | `proposed()` has tasks. One item per parent; a top-level proposal is its own item | `proposed:<parent id>:<largest proposed id>`, or `proposed:<id>` | newest `created_at` in the group | "The agent proposed {n} subtasks" · "{n} proposed subtasks" | Accept all, Reject all, or each one |
| Ready (Ready) | in `ready()`, has at least one blocker (`dependencies(id).waits_on`, all finished), and `since` is within `ready_window` | `ready:<task id>:<since>` | `since` | "Ready to start: {ids} {is/are} done" · "{ids} done · ready to start" | Do it |
| Finish it? (Finish it?) | an open parent whose subtasks that aren't dropped are all `done`, with at least one of them | `finish:<parent id>:<since>` | latest `last_event_at` of those subtasks | "All {n} subtasks are done. Mark the parent done?" · "All {n} subtasks done" | Mark done, or Keep open (dismisses) |
| Quiet (Quiet) | status `doing`, `in_review` or `blocked`; quiet for `quiet_after` or longer; no ancestor shown as quiet | `quiet:<task id>:<activity>` | the last activity | "No activity for {d} days." plus " {n} open PRs." if any · "Quiet for {d} days" | Open, or Snooze 3 days |

Further rules:

- **One item per request or PR.** When a thread or PR is linked to several open tasks, the item goes to the lowest task id. It still shows as a callout on each of those tasks (`for_task`).
- **Proposed children count as not done**, so a parent with an open proposal isn't "Finish it?". A parent whose subtasks are all dropped isn't either. `core` never finishes a parent itself (rule 10).
- **Ready names its blockers** by id, in id order, up to two ("143 and 145 are done"); more than two read "3 tasks it waited on are done". If one blocker's PR merged, the context reads "143 merged · ready to start", from `sync`'s `pr_tasks`.
- **Last activity**, for Quiet, is the latest of these, across the task and every task under it:
  - `TaskNode.subtree_last_event_at`, which includes `sync:t3`'s notes;
  - each linked thread's `last_activity_at`, where a `running` or `needs_me` thread counts as now;
  - each linked PR's GitHub `updated_at`.

  It's worked out bottom-up over `tree()` in one pass. Quiet is checked top-down, so a quiet parent hides its quiet subtasks.

### Order

- Groups come in the design's fixed order: Questions, Reviews, Failed runs, Proposed, Ready, Finish it?, Quiet.
- Inside a group, the newest `since` comes first. In Quiet, the longest-quiet comes first.
- Ties go by task id, then key.
- `for_task` uses the same order.

### What each action calls

`attention` only computes. Listed here so the app spec matches:

| Action | Call |
|---|---|
| Open thread / Open PR | the app opens `target` |
| Accept, Accept all | `set_status(id, Todo, None, me)` for each task |
| Reject, Reject all | `set_status(id, Dropped, reason, me)`; `core` records a `rejected` event |
| Do it | `launcher.start(&StartRequest { task, mode: DoIt, place: TaskRepos, again: false })` |
| Mark done | `set_status(parent, Done, None, me)` |
| Keep open, Dismiss (⌘⌫) | `dismiss(key)` |
| Snooze 3 days | `snooze(key, None)` |

Each action's effect comes back as a `core` or `sync` change, and the item drops out on the next recompute. `restore` backs the toast's Undo.

### Menu-bar count and "checked"

- `Summary.count` is `items().len()`, and `Summary.top` is the first `menu_items` of them. The app sets the menu-bar title and the sidebar badge from each `Summary` it receives. A count of 0 shows no number.
- `checked_at` is the oldest `checked_at` among `sync`'s sources that are `Ok`, or none if no source is. It drives "checked 10:44" and the empty state.

### Staying cheap

The worker owns the cached `Inputs`, the hidden rows and its own connection. It wakes only for these messages:

| Message | Re-reads | Then |
|---|---|---|
| `on_core_events` (any event) | `tree()`, `ready()`, `proposed()`, and `blockers` for ready tasks inside the window | recompute |
| `SyncChange` (any) | `sync.snapshot()`: an `Arc` clone | recompute |
| `dismiss`, `snooze`, `restore` | its own table: one write | recompute, then reply |
| timer at `next_wake` | nothing | recompute |

- Messages that arrive within `settle` (150 ms) of each other make one recompute.
- `next_wake` is the earliest of: when the next task turns quiet (last activity plus `quiet_after`), when a Ready item leaves its window, and when a snooze ends. The worker waits on its channel with `recv_timeout` until then. That is a single wake-up at a known time, not a polling timer.
- A `Summary` is sent only when the visible list changes (keys, `since`, headline or context).

## Errors and degraded modes

| Case | Behaviour |
|---|---|
| A `sync` source broken or off | Its items keep their last known data with `stale: true`. The app may dim them; the footer says why. Nothing is invented or dropped because a source is down. |
| A `core` read fails (lock timeout, I/O) | Keep the previous items, log once, and retry on the next message or after 5 s, once. |
| `attention_hidden` can't be read | Show everything (hide nothing) and log. Showing too much beats hiding a question. |
| `dismiss` or `snooze` can't be saved | Return the error; the item stays. The app shows a toast. |
| A task named in a hidden row is deleted | The row expires with the 30-day pruning. |

## Performance targets

| What | Target |
|---|---|
| `compute` for 2,000 tasks, 50 linked threads, 100 PRs, 200 hidden rows | under 5 ms (95th percentile) |
| `core` reads per change batch (`tree`, `ready`, `proposed`, up to 50 `blockers`) | under 25 ms |
| A change reaching `attention` to `Summary` delivered | under 200 ms, `settle` included |
| `dismiss` to updated list | under 20 ms |
| CPU when nothing changes | 0%: no wake-ups except at `next_wake` |
| Memory | under 5 MB |

## Testing

Tests use `airboss_sync::fake` snapshots and a `FakeCoreReads`. They never touch `gh`, `t3` or T3's database.

- **`tests/kinds.rs`** calls `compute` directly with built inputs:
  - one test per kind showing, and one per rule that hides it: a question on a done task, a draft PR, a PR while its agent runs, a ready task with no blockers, readiness older than the window, a parent with a proposed or only dropped children, a quiet child under a quiet parent;
  - the lowest-task-id rule for shared threads and PRs;
  - Ready naming three blockers and a merged PR.
- **`tests/order.rs`:** group order, newest first, Quiet longest first, ties.
- **`tests/quiet.rs`:** activity from core events, a running thread, a PR update; exactly 5 days; `next_wake` equal to last activity plus 5 days.
- **`tests/hidden.rs`** uses a temp `airboss.db` and `FakeClock`:
  - dismiss hides until the key changes (a new request, a new quiet stretch);
  - snooze hides until `until`;
  - `restore`;
  - rows survive reopening;
  - pruning after 30 days.
- **`tests/worker.rs`** drives the worker's `State::handle(message, now)` directly, with no real thread or real time:
  - a `SyncChange` causes no core reads;
  - a burst of 20 core events causes one `tree()`;
  - a `Summary` is sent only on a visible change;
  - the timer fires at `next_wake`.
- **`tests/migrations.rs`:** each CHECK rejects its bad row; opening twice applies once.
- **Performance** (`#[ignore]`): `compute` on a generated tree of 2,000 tasks, with the machine recorded.

## Project structure

```
crates/attention/
  Cargo.toml               airboss-attention
  src/lib.rs               start, AttentionHandle, Item and the other public types
  src/compute.rs           compute(): runs each kind, applies hidden rows, sorts, finds next_wake
  src/kinds.rs             one function per kind
  src/quiet.rs             last activity per subtree
  src/hidden.rs            attention_hidden reads, writes, pruning
  src/db.rs                own connection and migration
  src/migrations/001_attention.sql
  src/worker.rs            State::handle, the thread loop, settle and next_wake
  src/core_reads.rs        CoreReads and its SharedStore impl
  tests/…                  as listed above
```

## Boundaries

- **Always:** derive items only from `core`, `sync` and `attention_hidden`; keep `compute` pure; recompute only on a message or at `next_wake`.
- **Ask first:** a new kind or group (the design fixes seven groups); changing a default threshold; a new crate.
- **Never:** change a task, a link or a status; run `gh` or `t3` or read T3's files; poll on a timer; show untracked threads as items.

## Success criteria

- [ ] Every kind in the table shows from realistic inputs, and each hiding rule has a test.
- [ ] The list follows the design's group order, and the count equals its length.
- [ ] Dismiss and snooze survive a restart; a new occurrence shows again.
- [ ] A question asked in a linked thread is in Needs me within 3 s of T3 writing it, and leaves when it's answered.
- [ ] A task with no activity for 5 days shows as Quiet without any polling; its quiet subtasks don't.
- [ ] `compute` meets 5 ms for 2,000 tasks on this Mac, with the machine recorded.

## Checked on this Mac

25 Nov 2026:

- **DESIGN.md:** the seven groups and their order; the actions per kind; "Snooze 3 days"; the 6-item menu-bar dropdown; the empty state with "checked".
- **The prototype** (`docs/design/prototype.html`): the eight kinds (`ask`, `shield`, `pr`, `fail`, `plus`, `play`, `flag`, `moon`), their headline and context wording, and a snooze toast that ends 3 days later.
- **T3's real data:** questions carry text and a header, and approvals carry the command, as `sync` reads them.

## Not verified yet

- **`core` v2's exact names and shapes** (`ready()`'s `since`, `subtree_last_event_at`, `dependencies`). They're being built in parallel; this spec uses the names in the program brief. If they differ, change the calls in `core_reads.rs`, not the behaviour.
- **Choices the design doesn't make:** the 7-day ready window, holding Reviews back while the agent runs, and one item per request at the lowest task id. They are mine, and listed for review.
- **"The oldest open PR from 4 Nov"** in the prototype's Quiet text needs a PR's creation date, which `sync` doesn't read yet. v1 says only "{n} open PRs".
