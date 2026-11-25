# Spec: core

Module `core` from [CAPABILITIES.md](CAPABILITIES.md). State: **approved 25 Nov 2026** (draft 2).

Draft 2 folds in a review by a reader who had not seen draft 1 (25 Nov 2026). The [changes from draft 1](#changes-from-draft-1) are listed at the end.

## Objective

`core` is the single source of truth for tasks. It stores:

- the task tree
- which task waits on which
- every change made to a task
- the outside things linked to each task

It also enforces the rules that keep status honest.

Every other module reads and writes tasks only through `core`: `mcp` for agents, `app` for me, `sync` for GitHub and T3, and `launch` for new threads. None of them touches the tables directly.

**User stories**

- As me, I add a task of any size and nest it anywhere: a bug fix stays one task, a product gets phases and steps under it.
- As me, I can see the history of every task: who changed what, when, and why. That includes the proposals I rejected.
- As an agent, I can:
  - move my task between `doing`, `in_review` and `blocked`
  - add notes and links
  - propose tasks
  - mark a task `done`, but only if it has no linked PR, and only with a note saying what I did

  I can't create tasks directly, rename or move tasks, or drop them.
- As `sync:gh`, I record each linked PR's state. I mark a task done when its PRs have merged.
- As `plan`, the actor for a plan I've settled, my phases, steps and their dependencies are created straight away, and running the same plan again changes nothing.

## Tech stack

- **Rust edition 2024**, on stable Rust 1.100, pinned in `rust-toolchain.toml`. `core` is a library crate, `airboss-core`, with no Tauri dependency, so it builds and tests fast on its own.
- **Dependencies:** `rusqlite` 0.43 (with the `bundled` feature), `serde` 1 and `thiserror` 2. Nothing else without asking.
- **SQLite settings, applied on open, outside any transaction:**
  - `journal_mode = WAL`
  - `synchronous = NORMAL`, which is safe with WAL and avoids a disk sync on every commit
  - `foreign_keys = ON`
  - `busy_timeout = 2000`
- **Time:** UTC text written by SQLite, `strftime('%Y-%m-%dT%H:%M:%fZ','now')`. Times are only for display. Ordering and change-tracking use event ids, never times.

## Commands

```
cargo test -p airboss-core                              # unit and rule tests
cargo test -p airboss-core --release -- --ignored perf  # performance checks
cargo clippy -p airboss-core -- -D warnings
cargo fmt --check
```

## Project structure

```
Cargo.toml                 Workspace: lists the crates below
rust-toolchain.toml        Pins Rust 1.100
crates/core/
  Cargo.toml               airboss-core
  src/lib.rs               Public API: Store and its types
  src/db.rs                Opening, settings, migrations
  src/migrations/001_core.sql
  src/tasks.rs             Add, edit, move, delete, tree
  src/status.rs            Status changes and who may make them
  src/deps.rs              Dependencies, loop check, run order
  src/links.rs             Links and PR state
  src/events.rs            History and change notification
  tests/rules.rs           One test per rule
  tests/tree.rs            Nesting, moving, ordering
  tests/deps.rs            Dependencies and run order
  tests/perf.rs            #[ignore] performance checks
```

Later modules get their own crates (`crates/mcp`, `crates/sync`, `crates/launch`) plus `app/`. A module that needs tables owns its migrations and records them in the shared `schema_migrations` table under its own module name. For example, `launch` keeps the T3 project for each repo in its own table.

## Data model

Five tables, plus `schema_migrations`. This is the full first migration. On 25 Nov 2026 it was loaded into a throwaway in-memory SQLite database: the valid rows went in, and each of the 10 constraint checks rejected its bad row.

```sql
CREATE TABLE schema_migrations (
  module      TEXT NOT NULL,             -- 'core', 'sync', 'launch' …
  version     INTEGER NOT NULL,
  applied_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (module, version)
);

CREATE TABLE repos (                     -- one git repo tasks can point at
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL UNIQUE,
  path        TEXT NOT NULL UNIQUE,
  remote_url  TEXT                       -- null: local-only repo
);

CREATE TABLE tasks (                     -- one thing to get done, at any depth
  id             INTEGER PRIMARY KEY,
  parent_id      INTEGER REFERENCES tasks(id) ON DELETE CASCADE,  -- null: top level
  repo_id        INTEGER REFERENCES repos(id),                    -- null: not tied to one repo
  title          TEXT NOT NULL CHECK (length(trim(title)) > 0),
  body           TEXT,                                            -- null: no notes yet
  status         TEXT NOT NULL DEFAULT 'todo'
                 CHECK (status IN ('proposed','todo','doing','in_review','blocked','done','dropped')),
  blocked_reason TEXT,                                            -- set exactly when blocked
  position       INTEGER NOT NULL CHECK (position >= 1),          -- 1-based among siblings
  origin         TEXT NOT NULL CHECK (origin IN ('me','agent','plan')),
  plan_key       TEXT,                   -- the plan's own id for this step; null unless origin = plan
  created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  CHECK ((status = 'blocked') = (blocked_reason IS NOT NULL)),
  CHECK (parent_id IS NULL OR parent_id <> id),
  CHECK (plan_key IS NULL OR origin = 'plan')
);
CREATE INDEX tasks_parent_position_idx ON tasks(parent_id, position);
CREATE UNIQUE INDEX tasks_plan_key_idx ON tasks(ifnull(parent_id, 0), plan_key) WHERE plan_key IS NOT NULL;

CREATE TABLE task_events (               -- one change to one task
  id         INTEGER PRIMARY KEY,        -- also the change cursor
  task_id    INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL CHECK (kind IN ('created','edited','moved','status','note','linked',
                                           'unlinked','link_state','waits_on_added','waits_on_removed')),
  to_status  TEXT CHECK (to_status IN ('proposed','todo','doing','in_review','blocked','done','dropped')),
  body       TEXT,                       -- the note, or why; null when nothing to say
  actor      TEXT NOT NULL,
  at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  CHECK ((kind = 'status') = (to_status IS NOT NULL)),
  CHECK (kind <> 'note' OR body IS NOT NULL)
);
CREATE INDEX task_events_task_idx ON task_events(task_id, id);

CREATE TABLE task_deps (                 -- one task waiting on another
  task_id     INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  waits_on_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  added_by    TEXT NOT NULL,
  added_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (task_id, waits_on_id),
  CHECK (task_id <> waits_on_id)
);
CREATE INDEX task_deps_waits_on_idx ON task_deps(waits_on_id);

CREATE TABLE links (                     -- one outside thing attached to a task
  id         INTEGER PRIMARY KEY,
  task_id    INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL CHECK (kind IN ('t3_thread','claude_session','pr','slack','doc','github_issue')),
  ref        TEXT NOT NULL CHECK (length(ref) > 0),   -- thread id, owner/repo#n, Slack permalink path, file path
  url        TEXT,                                    -- null: local file, or no web page
  pr_state   TEXT CHECK (pr_state IN ('open','merged','closed')),  -- set exactly for PR links
  linked_by  TEXT NOT NULL,
  linked_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  UNIQUE (task_id, kind, ref),
  CHECK ((kind = 'pr') = (pr_state IS NOT NULL))
);
CREATE INDEX links_kind_ref_idx ON links(kind, ref);
```

**What isn't stored, and why:**
- **"Waiting"** is worked out from `task_deps` and the blockers' statuses.
- **A task's "last activity"** is its newest event.
- **"Has a PR"** and **"all PRs merged"** come from `links`.
- **Positions** are stored, because order is something I choose, not something that can be worked out.

**No UNIQUE constraint on `(parent_id, position)`.** SQLite checks uniqueness row by row while renumbering, so a renumber would fail partway. It also treats null parents as all different, so the constraint couldn't protect top-level tasks anyway. Gap-free positions are enforced in code (rule 13) and tested.

**Actors:** `me`, `agent:<thread ref>`, `sync:gh`, `sync:t3`, `plan` and `launch`. `core` trusts the actor it's given. Proving who the caller is belongs to `mcp`: the agent token can only act as `agent:…`, and acting as `plan` needs its own token. The `mcp` spec will define both.

## Rules

### Enforced by the database (one test each)

1. A blocked task has a reason, and a task that isn't blocked has none.
2. A task can't be its own parent, a parent must exist, and a title can't be blank.
3. The same `(task, kind, ref)` can only be linked once. A PR link always has a state, and no other link has one.
4. A status event names the new status, and a note event has text.
5. Deleting a task deletes its subtree, along with their events, links and dependencies.

### Who may change status (enforced by code)

6. **Status changes follow this table.** Anything else gets `Error::NotAllowed`, or `Error::InvalidTransition` when the actor is allowed in general but not from this status.

   | Actor | From | To | Conditions |
   |---|---|---|---|
   | `me` | any | any except `proposed` | Reopening `done` or `dropped` is allowed |
   | an agent | `todo`, `doing`, `in_review`, `blocked` | `doing`, `in_review`, `blocked` | |
   | an agent | `todo`, `doing`, `in_review`, `blocked` | `done` | The task has **no** PR link, and the change carries a note saying what was done (otherwise `Error::NoteRequired`) |
   | `sync:gh` | `todo`, `doing`, `in_review`, `blocked` | `done` | The task has at least one PR link; none is `open`; at least one is `merged` |
   | `launch` | `todo`, `blocked` | `doing` | When it starts a thread for the task |
   | `sync:t3`, `plan` | (none) | (none) | They never change status |

   `proposed` is left only by `me`: accepting moves it to `todo`, and rejecting moves it to `dropped` with an optional reason. Rejected proposals stay in history and are hidden from the tree. Only a true rejection is hidden: a proposal I accepted and later dropped stays visible, and so does an open proposal dropped along with its parent (rule 9).
7. **An open PR reopens a done task.** If an actor other than `me` links an `open` PR to a `done` task, the task moves to `in_review` in the same change. A PR linked already `merged` or `closed` reopens nothing. And when `set_pr_state` records a PR as `open` again, every `done` task linked to it moves to `in_review`, with a `status` event after the `link_state` event, by the same actor. Code can't stay done while its PR is unmerged.

### Who may do everything else (enforced by code)

8. **Permissions for every other call:**

   | Call | `me` | an agent | `plan` | `sync:gh` | `sync:t3` | `launch` |
   |---|---|---|---|---|---|---|
   | `add_repo` | yes | (no) | (no) | (no) | (no) | (no) |
   | `add_task` | yes, as `todo` | yes, always as `proposed` | (no) | (no) | (no) | (no) |
   | `add_tasks` (a plan) | yes | (no) | yes | (no) | (no) | (no) |
   | `edit_task` (title, notes, repo) | yes | (no) | (no) | (no) | (no) | (no) |
   | `add_note` | yes | yes | (no) | yes | yes | yes |
   | `move_task` | yes | (no) | (no) | (no) | (no) | (no) |
   | `delete_task` | yes | (no) | (no) | (no) | (no) | (no) |
   | `link` | yes | yes | yes | yes | yes | yes |
   | `unlink` | yes | only links it added | (no) | (no) | (no) | (no) |
   | `set_pr_state` | yes | (no) | (no) | yes | (no) | (no) |
   | `add_dependency` / `remove_dependency` | yes | (no) | yes | (no) | (no) | (no) |

   An agent that is stuck sets `blocked` with a reason. Dropping is a status change, so only `me` can drop (rule 6).
9. **Dropping drops the subtree.** Every open task under it moves to `dropped` in the same change, with one event each. Tasks already `done` stay `done`.
10. **A parent is never finished automatically.** When its last subtask becomes `done`, `attention` suggests finishing it, and I decide.

### Tree and order (enforced by code)

11. **No cycles in the tree.** A task can't be moved under itself or under one of its own subtasks.
12. **Moves are rejected under a `proposed`, `done` or `dropped` parent** (`Error::InvalidTarget`). New tasks can't be added there either.
13. **Positions start at 1 and are gap-free.** Every add, move and delete renumbers the siblings it affects. An out-of-range position is clamped: 0 becomes 1, and anything past the end becomes the end. Renumbering writes no events. Only the moved task gets one.

### Dependencies (enforced by code)

14. **No dependency loops, anywhere in the tree.** Before adding A-waits-on-B, a recursive query follows B's own blockers across the whole tree. If it reaches A, the dependency is rejected with `Error::Loop`.
15. **No dependency between a task and its own ancestor or descendant** (`Error::InvalidTarget`). A parent waiting on its child, or the reverse, could never finish, because of rule 10.
16. **"Waiting" is worked out, never stored.** A task is waiting while any task it waits on is neither `done` nor `dropped`. Waiting never changes a task's status.
17. **Deleting a blocker** removes its dependencies and writes a `waits_on_removed` event on each task that was waiting.
18. **Run order is a topological sort of a parent's direct subtasks, in waves.**
    - A dependency between tasks deeper down counts as a dependency between their top-level ancestors under that parent. For example, if a step in Phase 2 waits on a step in Phase 1, Phase 2 waits on Phase 1.
    - Kahn's algorithm makes the waves: wave 1 is every open subtask that waits on nothing under the parent, wave 2 is everything that waits only on wave 1, and so on. Within a wave, tasks keep their sibling order.
    - A subtask that also waits on an open task outside the parent goes in a separate `waiting_outside` group at the end, not in a wave.
    - `done` and `dropped` subtasks are left out.
    - Run order is only shown. Nothing starts by itself.

### Plans (enforced by code)

19. **`add_tasks` takes a whole plan as a tree, all or nothing.**
    - Each new task has a `key` that's unique within the plan, optional children, and `depends_on` entries. Each entry is either another key in the same plan or an existing task id.
    - Every rule is checked before anything is written. If any check fails, nothing is written.
    - The keys are stored as `plan_key`.
20. **The same plan twice changes nothing.** A key that already exists under the same parent resolves to the existing task, for nesting and for dependencies alike. Its title isn't overwritten. Only new keys are added, so renaming a step in a revised plan doesn't create duplicates.

### History (enforced by code)

21. **Every call writes one event for each task it changes, in the same transaction**, except when a PR event reopens a task (rule 7), which records the link or PR state and the reopen as two events, so history shows both. If the call fails, nothing is written, not even events. One call can change several things about one task. For example, a planned task created with its dependencies gets one `created` event, with the dependencies listed in its body.
22. **Events are never edited.** They go only when their task is deleted. Rejected proposals are dropped, not deleted, so they stay in history (rule 6).
23. **Every committed change tells the app.** `Store::subscribe()` returns a channel that receives the new event ids after each commit. `core` sends them itself after committing, so no SQLite hook is needed. The app updates from this signal, never by polling.

## API shape

This is the public interface the other modules call. Exact signatures are settled during planning.

```rust
pub struct Store { /* one rusqlite::Connection */ }

impl Store {
    pub fn open(path: &Path) -> Result<Store>;           // applies settings, runs core's migrations
    pub fn open_in_memory() -> Result<Store>;
    pub fn subscribe(&mut self) -> Receiver<Vec<EventId>>;

    // repos
    pub fn add_repo(&mut self, name: &str, path: &Path, remote: Option<&str>, actor: &Actor) -> Result<Repo>;
    pub fn repos(&self) -> Result<Vec<Repo>>;

    // tasks
    pub fn add_task(&mut self, new: NewTask, actor: &Actor) -> Result<Task>;
    pub fn add_tasks(&mut self, parent: Option<TaskId>, plan: &[PlanTask], actor: &Actor) -> Result<PlanResult>;
    pub fn edit_task(&mut self, id: TaskId, edit: TaskEdit, actor: &Actor) -> Result<Task>;
    pub fn add_note(&mut self, id: TaskId, text: &str, actor: &Actor) -> Result<()>;
    pub fn move_task(&mut self, id: TaskId, parent: Option<TaskId>, position: u32, actor: &Actor) -> Result<()>;
    pub fn set_status(&mut self, id: TaskId, to: Status, note: Option<&str>, actor: &Actor) -> Result<Task>;
    pub fn delete_task(&mut self, id: TaskId, actor: &Actor) -> Result<()>;

    pub fn task(&self, id: TaskId) -> Result<Task>;      // Error::NotFound if missing
    pub fn tree(&self) -> Result<Vec<TaskNode>>;         // in order, with waiting flags; hides rejected proposals
    pub fn proposed(&self) -> Result<Vec<Task>>;
    pub fn history(&self, id: TaskId) -> Result<Vec<Event>>;
    pub fn events_after(&self, cursor: EventId) -> Result<Vec<Event>>;

    // links
    pub fn link(&mut self, id: TaskId, link: NewLink, actor: &Actor) -> Result<Link>;   // linking twice returns the existing link
    pub fn unlink(&mut self, link: LinkId, actor: &Actor) -> Result<()>;
    pub fn set_pr_state(&mut self, pr_ref: &str, state: PrState, actor: &Actor) -> Result<Vec<TaskId>>;
    pub fn tasks_for_link(&self, kind: LinkKind, r#ref: &str) -> Result<Vec<TaskId>>;
    pub fn links(&self, id: TaskId) -> Result<Vec<Link>>;

    // dependencies
    pub fn add_dependency(&mut self, task: TaskId, waits_on: TaskId, actor: &Actor) -> Result<()>;
    pub fn remove_dependency(&mut self, task: TaskId, waits_on: TaskId, actor: &Actor) -> Result<()>;
    pub fn waits_on(&self, id: TaskId) -> Result<Vec<Task>>;   // its open blockers
    pub fn ready(&self) -> Result<Vec<Task>>;                  // `todo`, no open blockers
    pub fn run_order(&self, parent: TaskId) -> Result<RunOrder>;  // { waves, waiting_outside }
}

pub struct PlanTask { key: String, title: String, body: Option<String>, repo: Option<RepoId>,
                      depends_on: Vec<DepRef>, children: Vec<PlanTask> }
pub enum DepRef { Key(String), Task(TaskId) }

pub enum Error { EmptyTitle, AlreadyExists, NotFound, NotAllowed, InvalidTransition, NoteRequired, InvalidTarget, Loop,
                 DuplicateKey, Db(rusqlite::Error) }
```

`Store` isn't shared between threads by itself. The app holds one `Store` behind a mutex. Every call is short, so `mcp`, `app` and `sync` waiting on one lock stays well under the 20 ms MCP target.

## Code style

```rust
/// Agents move work forward and can finish PR-less work; code finishes only when its PRs merge.
fn check_status_change(actor: &Actor, task: &TaskFacts, to: Status, note: Option<&str>) -> Result<()> {
    use Status::*;
    let open = matches!(task.status, Todo | Doing | InReview | Blocked);
    match (actor, to) {
        (Actor::Me, Proposed) => Err(Error::NotAllowed),
        (Actor::Me, _) => Ok(()),
        (Actor::Agent(_), Doing | InReview | Blocked) if open => Ok(()),
        (Actor::Agent(_), Done) if open && !task.has_pr => note.map(|_| ()).ok_or(Error::NoteRequired),
        (Actor::SyncGh, Done) if open && task.prs_all_merged_or_closed && task.any_pr_merged => Ok(()),
        (Actor::Launch, Doing) if matches!(task.status, Todo | Blocked) => Ok(()),
        (Actor::Agent(_) | Actor::SyncGh | Actor::Launch, _) if !open => Err(Error::InvalidTransition),
        _ => Err(Error::NotAllowed),
    }
}
```

- `rustfmt` defaults, and clippy clean with `-D warnings`.
- No `unwrap` or `expect` outside tests. One `thiserror` enum, `core::Error`.
- SQL lives in `.sql` files or `const` strings next to the function that runs it. No ORM.
- `TaskId(i64)`, `EventId(i64)` and similar wrapper types, never bare integers. Statuses, actors and link kinds are enums in Rust code.
- A short doc comment on every public item, saying why, not what.

## Testing strategy

- **Tests come first:** one failing test per rule before its code.
  - Rule tests live in `tests/rules.rs`, named after the behaviour, e.g. `agent_cannot_finish_task_with_pr`.
  - Rule 6 gets one test per row of its table, plus the cases it rejects.
  - Rule 8 gets one test per "no" in its table.
- **Isolation:** every test uses `Store::open_in_memory()`, so no test touches disk or another test's data.
- **Tree tests:** nesting 5 levels deep, moving a subtree, clamping and renumbering, moves into a `done` parent rejected, and cycles.
- **Dependency tests:**
  - a chain: C waits on B, which waits on A
  - loops of length 2, 3 and 5, rejected even when they cross different parents
  - a parent and child waiting on each other, rejected
  - a waiting task becoming ready when its blocker finishes
- **Run-order tests:**
  - a diamond: B and C wait on A, and D waits on B and C, giving waves [A], [B, C], [D]
  - steps in Phase 2 waiting on steps in Phase 1, which makes Phase 2 wait on Phase 1
  - a subtask with an open blocker outside the parent, which lands in `waiting_outside`
- **Plan tests:**
  - a three-level plan with dependencies inside it
  - the same plan twice, which changes nothing
  - a revised plan with one renamed step and one new step, which adds only the new step
  - a plan containing a loop, which writes nothing
- **Failure tests:** an error forced partway through a call leaves no rows and no events behind, and no notification goes out. They live in `src/` unit tests, because the fault-injection points exist only in test builds of the crate.
- **Concurrency:** several `Store` handles on one file must write at once without "database is locked". Every write takes the lock up front (an IMMEDIATE transaction), and opening sets `busy_timeout` before anything else.
- **Change notification:** one commit sends exactly the ids of the events it wrote.
- **Performance checks** run in release mode, `#[ignore]`d by default, on a file-backed database with `synchronous = NORMAL`. The numbers are the 95th percentile over 1,000 runs. Each run prints the machine (model, chip and macOS version) next to the numbers.

  | What | Target |
  |---|---|
  | `tree()`: 2,000 tasks with waiting flags and 1,000 dependencies | under 10 ms |
  | `add_task` | under 1 ms |
  | `set_status`, event included | under 1 ms |
  | `add_dependency`, loop check included, on that 2,000-task tree | under 2 ms |
  | `run_order`: a parent with 200 subtasks and 400 dependencies | under 2 ms |
- **Coverage:** no percentage is required. Every public function and every rule needs at least one test.

## Boundaries

- **Always:**
  - Run `fmt`, `clippy` and the tests before every commit.
  - Write events in the same transaction as the change.
  - Take the actor as an argument and never guess it.
  - Keep `core` free of network calls and of Tauri.
- **Ask first:**
  - Adding any dependency.
  - Changing a rule or one of the permission tables.
  - Changing the schema once the app is in daily use. That needs a new migration file, never an edited one.
- **Never:**
  - Read or write T3's or Claude Code's files from `core`.
  - Expose raw SQL or the connection to other modules.
  - Edit or delete an event, except through deleting its task.

## Success criteria

- [ ] `cargo test -p airboss-core` passes, with tests for every rule, every row of the rule 6 table and every "no" in the rule 8 table.
- [ ] An agent can't accept its own proposal, reopen a `done` or `dropped` task, drop a task, or finish a task that has a PR.
- [ ] `sync:gh` finishes a task only when none of its PRs is open and at least one has merged.
- [ ] Linking an open PR to a `done` task, or reopening its PR, moves it back to `in_review`.
- [ ] Rejected proposals are `dropped`, are hidden from `tree()`, and are still in history.
- [ ] Dependency loops of any length are rejected, including across parents, and nothing is written.
- [ ] `run_order` returns the expected waves for the diamond, the cross-phase case and the outside-blocker case.
- [ ] Running a plan twice changes nothing, and a revised plan adds only its new steps.
- [ ] A call that fails partway leaves no rows, no events and no notification.
- [ ] Each performance check meets its target, with the machine recorded next to the numbers.
- [ ] `clippy -D warnings` and `fmt --check` are clean.
- [ ] Opening an existing database runs only the pending `core` migrations, once, and keeps the data.

## Decisions (settled 25 Nov 2026)

- **Sibling order:** whole numbers, renumbered on every add, move and delete (rule 13).
- **Parent done:** never automatic. `attention` suggests it (rule 10).
- **`sync:t3`:** never changes status, to keep v1 simple (rule 6).
- **Dropping a parent** drops its open subtree (rule 9).
- **Agents finish only PR-less work,** with a note. Code finishes when its PRs merge (rules 6 and 7).
- **Dependencies:** any task can wait on any other except its own ancestor or descendant (rules 14–17). Outside blockers, such as a PR in another repo or a Linear ticket, aren't tracked in v1: set `blocked` with a reason.
- **Run order:** shown, never started automatically (rule 18).
- **Rejecting a proposal** drops it rather than deleting it, so history keeps it (rule 6).

## Open questions

None. The review's points are all handled above. Who proves an actor's identity belongs to the `mcp` spec.

## Changes from draft 1

The fresh review found 13 problems. Each one and its fix:

1. **No rules for moving between statuses**, so an agent could accept its own proposal or reopen a dropped task. **Fix:** the from/to table in rule 6.
2. **`core` couldn't know whether a PR had merged.** **Fix:** `links.pr_state`, `set_pr_state`, `tasks_for_link`, and the multi-PR condition in rule 6.
3. **Draft 1 contradicted itself on whether agents can drop tasks.** **Fix:** only `me` can drop (rules 6 and 8).
4. **The schema had no columns, and `add_tasks` couldn't express a nested plan.** **Fix:** the full schema above, tested, and a plan tree with keys (rules 19 and 20).
5. **One `PRAGMA user_version` can't track several modules' migrations.** **Fix:** the `schema_migrations(module, version)` table.
6. **Rights for anything other than status were undefined, and anyone could claim to be `plan`.** **Fix:** the permission table in rule 8, `unlink`, rule 7, and the `mcp` spec proving who is calling.
7. **The loop check only looked under one parent, and parent-child dependencies were allowed.** **Fix:** a loop check across the whole tree (rule 14), rule 15, dependencies counted at the top-level subtask (rule 18), a `waiting_outside` group, and a clearer diamond example.
8. **Rejecting a proposal deleted it, which broke "history is append-only".** **Fix:** rejected proposals are dropped instead (rules 6 and 22).
9. **Change-tracking used millisecond timestamps, and the app had to poll for changes.** **Fix:** event ids as the cursor, plus `subscribe()` (rule 23).
10. **`move_task` was underspecified.** **Fix:** rules 12 and 13.
11. **Re-running a plan matched steps by title.** **Fix:** `plan_key` (rule 20).
12. **The performance checks left settings unstated.** **Fix:** `synchronous = NORMAL`, every index named, and the machine recorded with each run.
13. **Smaller points:** "one event per task per call", a `repos` API, the T3 project moved to `launch`, `Error::NotFound`, and consistent actor names.
