# Spec: core

Module `core` from [CAPABILITIES.md](CAPABILITIES.md). State: **approved 25 Nov 2026** (draft 2); **built** the same day in `crates/core`. **v2** (what the app design needs) was decided and built the same evening; see [v2](#v2-25-nov-2026). **v3** (what the app and service specs need) followed on 26 Nov 2026; see [v3](#v3-26-nov-2026).

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
- As me, I link a task to several repos, and `launch` starts one thread per repo on the task's branch.
- As `attention`, I can tell when a task became ready and when a subtree last changed, without reading every event.

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
cargo clippy -p airboss-core --all-targets -- -D warnings
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
  src/migrations/002_pr_refs.sql   PR refs already stored, in the canonical form
  src/tasks.rs             Add, edit, move, delete, tree
  src/repos.rs             Repos, and the repos each task links to
  src/status.rs            Status changes and who may make them
  src/deps.rs              Dependencies, loop check, run order
  src/links.rs             Links and PR state
  src/events.rs            History and change notification
  tests/rules.rs           One test per rule
  tests/tree.rs            Nesting, moving, ordering
  tests/deps.rs            Dependencies and run order
  tests/history.rs         Subtree history and tree activity
  tests/perf.rs            #[ignore] performance checks
```

Later modules get their own crates (`crates/mcp`, `crates/sync`, `crates/launch`) plus `app/`. A module that needs tables owns its migrations and records them in the shared `schema_migrations` table under its own module name. For example, `launch` keeps the T3 project for each repo in its own table. It opens its own `Store` on `path()` and runs them with `migrate_module` (rule 31).

## Data model

Six tables, plus `schema_migrations`. This is the full first migration. On 25 Nov 2026 it was loaded into a throwaway in-memory SQLite database: the valid rows went in, and each of the 10 constraint checks rejected its bad row.

v2 edited `001_core.sql` in place instead of adding a migration 002, because no real database existed yet. **Once `mcp` ships, migrations are append-only:** every later change is a new numbered file. `mcp` shipped on 26 Nov 2026. v3 added `002_pr_refs.sql`, which changes no table: it rewrites PR refs already stored into the canonical form (rule 29).

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
  title          TEXT NOT NULL CHECK (length(trim(title)) > 0),
  body           TEXT,                                            -- null: no notes yet
  status         TEXT NOT NULL DEFAULT 'todo'
                 CHECK (status IN ('proposed','todo','doing','in_review','blocked','done','dropped')),
  blocked_reason TEXT,                                            -- set exactly when blocked
  position       INTEGER NOT NULL CHECK (position >= 1),          -- 1-based among siblings
  origin         TEXT NOT NULL CHECK (origin IN ('me','agent','plan')),
  plan_key       TEXT,                   -- the plan's own id for this step; null unless origin = plan
  branch         TEXT,                   -- the branch its threads share; null: no branch yet
  created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  CHECK ((status = 'blocked') = (blocked_reason IS NOT NULL)),
  CHECK (parent_id IS NULL OR parent_id <> id),
  CHECK (plan_key IS NULL OR origin = 'plan')
);
CREATE INDEX tasks_parent_position_idx ON tasks(parent_id, position);
CREATE UNIQUE INDEX tasks_plan_key_idx ON tasks(ifnull(parent_id, 0), plan_key) WHERE plan_key IS NOT NULL;

CREATE TABLE task_repos (                -- one repo a task is linked to; a task has any number
  task_id    INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  repo_id    INTEGER NOT NULL REFERENCES repos(id),
  linked_by  TEXT NOT NULL,
  linked_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (task_id, repo_id)
);
CREATE INDEX task_repos_repo_idx ON task_repos(repo_id);

CREATE TABLE task_events (               -- one change to one task
  id         INTEGER PRIMARY KEY,        -- also the change cursor
  task_id    INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL CHECK (kind IN ('created','edited','moved','status','note','linked',
                                           'unlinked','link_state','waits_on_added','waits_on_removed',
                                           'rejected','repo_linked','repo_unlinked')),
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
  title      TEXT,                                    -- null: show the ref
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
- **A task's "last activity"** is its newest event, and a subtree's is the newest event in it (rule 28).
- **"Ready since"** comes from the blockers' events (rule 26).
- **A parent's repos** are only its own. The union over its subtasks is the app's to show.
- **"Has a PR"** and **"all PRs merged"** come from `links`.
- **Positions** are stored, because order is something I choose, not something that can be worked out.

**No UNIQUE constraint on `(parent_id, position)`.** SQLite checks uniqueness row by row while renumbering, so a renumber would fail partway. It also treats null parents as all different, so the constraint couldn't protect top-level tasks anyway. Gap-free positions are enforced in code (rule 13) and tested.

**Actors:** `me`, `agent:<thread ref>`, `sync:gh`, `sync:t3`, `plan` and `launch`. `core` trusts the actor it's given. Working out who the caller is belongs to `mcp` (see [SPEC-mcp.md](SPEC-mcp.md)): over MCP a caller is always `agent:…`, identified from its thread's environment, except `import_plan`, which acts as `plan`. Only the CLI and the app act as `me`.

## Rules

### Enforced by the database (one test each)

1. A blocked task has a reason, and a task that isn't blocked has none.
2. A task can't be its own parent, a parent must exist, and a title can't be blank.
3. The same `(task, kind, ref)` can only be linked once, and the same `(task, repo)` too. A PR link always has a state, and no other link has one.
4. A status event names the new status, and a note event has text.
5. Deleting a task deletes its subtree, along with their events, links, repo links and dependencies.

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

   `proposed` is left only by `me`: accepting moves it to `todo`, and rejecting moves it to `dropped` with an optional reason. A rejection writes a single `rejected` event (with the reason as its body) instead of a `status` event. The tree hides a task that is `dropped` and whose latest status change was a `rejected` event, so it depends on the event kind, never on the wording of a note. A proposal I accepted and later dropped stays visible, and so does an open proposal dropped along with its parent (rule 9). If I reopen a rejected proposal, it comes back.
7. **An open PR reopens a done task.** If an actor other than `me` links an `open` PR to a `done` task, the task moves to `in_review` in the same change. A PR linked already `merged` or `closed` reopens nothing. And when `set_pr_state` records a PR as `open` again, every `done` task linked to it moves to `in_review`, with a `status` event after the `link_state` event, by the same actor. Code can't stay done while its PR is unmerged.

### Who may do everything else (enforced by code)

8. **Permissions for every other call:**

   | Call | `me` | an agent | `plan` | `sync:gh` | `sync:t3` | `launch` |
   |---|---|---|---|---|---|---|
   | `add_repo` | yes | (no) | (no) | (no) | (no) | (no) |
   | `add_task` | yes, as `todo` | yes, always as `proposed` | (no) | (no) | (no) | (no) |
   | `add_tasks` (a plan) | yes | (no) | yes | (no) | (no) | (no) |
   | `edit_task` (title, notes) | yes | (no) | (no) | (no) | (no) | (no) |
   | `link_repo` / `unlink_repo` | yes | (no) | yes | (no) | (no) | (no) |
   | `set_branch` | yes | (no) | (no) | (no) | (no) | yes |
   | `add_note` | yes | yes | (no) | yes | yes | yes |
   | `move_task` | yes | (no) | (no) | (no) | (no) | (no) |
   | `delete_task` | yes | (no) | (no) | (no) | (no) | (no) |
   | `link` | yes | yes | yes | yes | yes | yes |
   | `unlink` | yes | only links it added | (no) | (no) | (no) | (no) |
   | `set_pr_state` | yes | (no) | (no) | yes | (no) | (no) |
   | `add_dependency` / `remove_dependency` | yes | (no) | yes | (no) | (no) | (no) |

   An agent that is stuck sets `blocked` with a reason. Dropping is a status change, so only `me` can drop (rule 6). Agents never change a task's repos or branch.
9. **Dropping drops the subtree.** Every open task under it moves to `dropped` in the same change, with one event each. Tasks already `done` stay `done`.
10. **A parent is never finished automatically.** When its last subtask becomes `done`, `attention` suggests finishing it, and I decide.

### Repos and branch (enforced by code)

24. **A task links to any number of repos.**
    - `add_task` and `add_tasks` link the repos they're given, in the same change, with no event beyond `created`.
    - `link_repo` writes a `repo_linked` event and `unlink_repo` a `repo_unlinked` event, each naming the repo. Linking a repo that's already linked changes nothing. Unlinking one that isn't is `Error::NotFound`.
    - `Task.repos` lists a task's own repos, sorted by repo name. `tree()` carries the same list. A parent shows only its own; the union over its subtree is the app's job.
    - A plan sent again leaves an existing step's repos as they are, like its title (rule 20). `plan` links more with `link_repo`.
25. **A task has at most one branch,** shared by every thread `launch` starts for it. Null means no branch yet. `set_branch` writes one `edited` event whose body names the branch, e.g. `branch: booking-flow`, or `branch cleared`. A blank name clears it. Setting the branch it already has writes nothing.
    - A name, once trimmed, uses only `A–Z`, `a–z`, `0–9`, `.`, `_`, `/` and `-`, and is at most 200 characters long.
    - It can't contain `..`, start with `-` or `/`, or end with `/` or `.lock`.
    - Anything else is `Error::InvalidTarget`, and nothing is written. `launch` passes the name to `git` and `t3`, so it must never read as an option or a path outside the repo.

### Links (enforced by code)

29. **A PR has one ref.** `link`, `set_pr_state` and `tasks_for_link` take a PR as `owner/repo#n`, in any case, or as its GitHub URL, `https://github.com/owner/repo/pull/n`, with or without a trailing path, query or fragment. They store and compare `owner/repo#n` in lowercase, with `n` a whole number from 1 and no leading zeros. Anything else is `Error::InvalidTarget`. So the same PR linked as a URL and as `Owner/Repo#12` is one link, and `sync:gh` finds it whichever form it was given.
    - `owner` and `repo` use only letters, digits, `.`, `_` and `-`, and neither is `.` or `..`.
    - Event bodies name the canonical ref.
    - Other link kinds keep their ref as given.

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
    - A subtask that also waits on an open task outside the parent goes in a separate `waiting_outside` group at the end, not in a wave. Each entry lists its blockers: the open tasks outside the parent that hold it, directly, through its subtree, or through a sibling it waits on. They're sorted by id.
    - Lifting can make subtasks wait on each other in a loop even when their steps don't, for example when Phase 1 and Phase 2 each have a step waiting on the other phase. Kahn's algorithm can't place those subtasks, or anything waiting on them; these go in a `tangled` group, so the rest of the order still shows. They keep their sibling order.
    - `done` and `dropped` subtasks are left out, and so are `proposed` ones until I accept them. A dependency on an unaccepted proposal doesn't hold anything back in run order.
    - Run order is only shown. Nothing starts by itself.

26. **A ready task knows since when.** `ready()` gives each task the time its last blocker became `done` or `dropped` (a `status` event to either, or a `rejected` event), or stopped being a blocker (a `waits_on_removed` event on the task). If it never waited, or every blocker finished before it was created, that's its creation time. "Last" goes by event id, never by time.
27. **`dependencies(id)` lists every dependency, both ways,** finished blockers included: what the task waits on, and what waits on it, each sorted by id. `waits_on(id)` still lists open blockers only.

### Plans (enforced by code)

19. **`add_tasks` takes a whole plan as a tree, all or nothing.**
    - Each new task has a `key` that's unique within the plan, optional children, and `depends_on` entries. Each entry is either another key in the same plan or an existing task id.
    - Every rule is checked before anything is written. If any check fails, nothing is written.
    - The keys are stored as `plan_key`.
20. **The same plan twice changes nothing.** A key that already exists under the same parent resolves to the existing task, for nesting and for dependencies alike. Its title isn't overwritten. Only new keys are added, so renaming a step in a revised plan doesn't create duplicates.

### History (enforced by code)

21. **Every call writes one event for each task it changes, in the same transaction**, except when a PR event reopens a task (rule 7), which records the link or PR state and the reopen as two events, so history shows both. If the call fails, nothing is written, not even events. One call can change several things about one task. For example, a planned task created with its dependencies gets one `created` event, with the dependencies listed in its body. A call that changes nothing, such as setting the status a task already has or moving it to where it already is, writes no event. Dropping an already dropped task still drops anything open under it (rule 9).
22. **Events are never edited.** They go only when their task is deleted. Rejected proposals are dropped, not deleted, so they stay in history (rule 6).
23. **Every committed change tells the app.** `Store::subscribe()` returns a channel that receives the new event ids after each commit. `core` sends them itself after committing, so no SQLite hook is needed. The app updates from this signal, never by polling.
    - `subscribe()` only hears this handle's own writes. A write from another process, such as `airboss mcp`, shows up in `data_version()`: SQLite's `PRAGMA data_version`, which changes when **another** connection commits to the file, and never for this handle's own commits. The app and sync watch the `-wal` file and check `data_version()` on focus and on wake.
28. **Activity is read in bulk.**
    - `history_with_subtasks(id, limit)` returns up to `limit` events from the task and everything under it, newest first.
    - Each `tree()` node carries its own newest event's time and actor (`last_event_at`, `last_actor`), and the newest event time anywhere in its subtree (`subtree_last_event_at`), for "quiet for 5+ days". Newest goes by event id.
    - `tree()` works these out with one query and one pass in Rust, never a query per node. A task with no events at all can only come from a hand-edited file, and fails the read.
30. **A snapshot is one consistent read.** `snapshot()` reads the tree, every dependency edge, the ready list and the newest event id inside one read transaction, so all four describe the same moment even while another process writes. It builds the tree once, and works the ready list out from that tree. `ready()` does the same on its own, so it never builds the tree twice either.
    - `edges` and `dependency_edges()` list every `(waiting, waits_on)` pair, finished tasks included, sorted by the waiting task and then the blocker.
    - `latest_event_id` is the highest event id, or `EventId(0)` when there are no events.
31. **Another module migrates only its own tables.** `migrate_module(module, migrations)` runs `module`'s pending migrations as `Store::open` runs `core`'s: each in its own IMMEDIATE transaction, recorded in `schema_migrations` under `module`, and never twice.
    - `core` is refused with `Error::InvalidTarget`, so nothing outside `core` can apply or skip a `core` migration. So is a blank module name, or versions that aren't whole numbers from 1 in strictly increasing order.
    - A failing migration leaves nothing of itself behind. Those before it in the list stay applied.
    - It writes no events and sends no notification.

## API shape

This is the public interface the other modules call. Exact signatures are settled during planning.

```rust
pub struct Store { /* one rusqlite::Connection */ }

impl Store {
    pub fn open(path: &Path) -> Result<Store>;           // applies settings, runs core's migrations
    pub fn open_in_memory() -> Result<Store>;
    pub fn path(&self) -> Option<PathBuf>;              // the file's absolute path; None in memory
    pub fn subscribe(&mut self) -> Receiver<Vec<EventId>>;
    pub fn data_version(&self) -> Result<i64>;          // changes when ANOTHER connection commits
    pub fn migrate_module(&mut self, module: &str, migrations: &[Migration]) -> Result<()>;  // "core": InvalidTarget

    // repos
    pub fn add_repo(&mut self, name: &str, path: &Path, remote: Option<&str>, actor: &Actor) -> Result<Repo>;
    pub fn repos(&self) -> Result<Vec<Repo>>;

    // tasks
    pub fn add_task(&mut self, new: NewTask, actor: &Actor) -> Result<Task>;
    pub fn add_tasks(&mut self, parent: Option<TaskId>, plan: &[PlanTask], actor: &Actor) -> Result<PlanResult>;
    pub fn edit_task(&mut self, id: TaskId, edit: TaskEdit, actor: &Actor) -> Result<Task>;  // title, notes
    pub fn link_repo(&mut self, id: TaskId, repo: RepoId, actor: &Actor) -> Result<()>;      // twice: no-op
    pub fn unlink_repo(&mut self, id: TaskId, repo: RepoId, actor: &Actor) -> Result<()>;    // not linked: NotFound
    pub fn set_branch(&mut self, id: TaskId, branch: Option<&str>, actor: &Actor) -> Result<Task>;
    pub fn add_note(&mut self, id: TaskId, text: &str, actor: &Actor) -> Result<()>;
    pub fn move_task(&mut self, id: TaskId, parent: Option<TaskId>, position: u32, actor: &Actor) -> Result<()>;
    pub fn set_status(&mut self, id: TaskId, to: Status, note: Option<&str>, actor: &Actor) -> Result<Task>;
    pub fn delete_task(&mut self, id: TaskId, actor: &Actor) -> Result<()>;

    pub fn task(&self, id: TaskId) -> Result<Task>;      // Error::NotFound if missing
    pub fn tree(&self) -> Result<Vec<TaskNode>>;         // in order, with waiting flags and activity; hides rejected proposals
    pub fn proposed(&self) -> Result<Vec<Task>>;
    pub fn history(&self, id: TaskId) -> Result<Vec<Event>>;                            // oldest first
    pub fn history_with_subtasks(&self, id: TaskId, limit: u32) -> Result<Vec<Event>>;  // newest first
    pub fn events_after(&self, cursor: EventId) -> Result<Vec<Event>>;
    pub fn latest_event_id(&self) -> Result<EventId>;   // EventId(0) when empty
    pub fn snapshot(&self) -> Result<CoreSnapshot>;     // one read transaction; tree built once

    // links
    pub fn link(&mut self, id: TaskId, link: NewLink, actor: &Actor) -> Result<Link>;   // linking twice returns the existing link
    pub fn unlink(&mut self, link: LinkId, actor: &Actor) -> Result<()>;
    pub fn set_pr_state(&mut self, pr_ref: &str, state: PrState, actor: &Actor) -> Result<Vec<TaskId>>;
    pub fn tasks_for_link(&self, kind: LinkKind, r#ref: &str) -> Result<Vec<TaskId>>;
    pub fn links(&self, id: TaskId) -> Result<Vec<Link>>;
    pub fn links_of_kind(&self, kind: LinkKind) -> Result<Vec<Link>>;   // every task's, by link id

    // dependencies
    pub fn add_dependency(&mut self, id: TaskId, waits_on: TaskId, actor: &Actor) -> Result<()>;
    pub fn remove_dependency(&mut self, id: TaskId, waits_on: TaskId, actor: &Actor) -> Result<()>;
    pub fn waits_on(&self, id: TaskId) -> Result<Vec<Task>>;   // its open blockers
    pub fn dependencies(&self, id: TaskId) -> Result<Dependencies>;  // both ways, finished ones too
    pub fn dependency_edges(&self) -> Result<Vec<(TaskId, TaskId)>>; // every (waiting, waits_on), finished ones too
    pub fn ready(&self) -> Result<Vec<ReadyTask>>;             // `todo`, no open blockers, with since
    pub fn run_order(&self, parent: TaskId) -> Result<RunOrder>;  // { waves, waiting_outside, tangled }
}

pub struct Task { id, parent, repos: Vec<RepoId>, title, body, status, blocked_reason, position,
                  origin, plan_key, branch: Option<String>, created_at }
pub struct NewTask { title, parent, repos: Vec<RepoId>, body }       // .in_repo(r) adds one; call it again for more
pub struct TaskNode { task: Task, depth: u32, waiting: bool,
                      last_event_at: String, last_actor: Actor, subtree_last_event_at: String }
pub struct NewLink { kind, r#ref, url, pr_state, title: Option<String> }  // .with_title(t)
pub struct Link { id, task, kind, r#ref, url, pr_state, title: Option<String>, linked_by, linked_at }
pub struct Dependencies { waits_on: Vec<Task>, blocks: Vec<Task> }
pub struct ReadyTask { task: Task, since: String }
pub struct RunOrder { waves: Vec<Vec<Task>>, waiting_outside: Vec<OutsideWait>, tangled: Vec<Task> }
pub struct OutsideWait { task: Task, blockers: Vec<TaskId> }
pub struct CoreSnapshot { tree: Vec<TaskNode>, edges: Vec<(TaskId, TaskId)>, ready: Vec<ReadyTask>,
                          latest_event_id: EventId }
pub struct Migration { version: i64, sql: &'static str }

pub struct PlanTask { key: String, title: String, body: Option<String>, repos: Vec<RepoId>,
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
- **v2 tests:**
  - several repos on one task, sorted by name, from `add_task`, from a plan and from `link_repo`; linking twice and unlinking, with their events
  - `link_repo`, `unlink_repo` and `set_branch` refused for every actor the rule 8 table says no to
  - a branch set, changed and cleared, with one `edited` event each, and none for a repeat
  - a link title stored and read back
  - `history_with_subtasks` newest first, across three levels, cut at `limit`
  - `dependencies` both ways, with a finished blocker included
  - `waiting_outside` blockers: direct, through a step, and through a sibling
  - tree activity: own newest event and actor, and a subtree's newest event coming up from a grandchild
  - `ready` since: creation time with no blockers, and the last blocker's finish time otherwise
- **v3 tests:**
  - `snapshot()` matches `tree()`, `dependency_edges()`, `ready()` and `latest_event_id()` read one by one, and `EventId(0)` on an empty store
  - `dependency_edges` with a finished blocker included
  - `data_version`: two `Store` handles on one file; it changes after the other handle commits, and not after its own
  - `links_of_kind` across several tasks; `path()` for a file and for memory
  - `migrate_module`: applied once, refused for `core`, a blank name and out-of-order versions, and a failing migration leaving nothing of itself
  - PR refs: mixed case, the URL form with a trailing path, query and fragment, the same PR in two forms making one link, `set_pr_state` and `tasks_for_link` by URL, malformed refs refused with nothing written, and `002_pr_refs.sql` rewriting refs stored before it
  - branch names: each rejected shape, and names at and just over 200 characters
- **Failure tests:** an error forced partway through a call leaves no rows and no events behind, and no notification goes out. They live in `src/` unit tests, because the fault-injection points exist only in test builds of the crate.
- **Concurrency:** several `Store` handles on one file must write at once without "database is locked". Every write takes the lock up front (an IMMEDIATE transaction), and opening sets `busy_timeout` before anything else.
- **Change notification:** one commit sends exactly the ids of the events it wrote.
- **Performance checks** run in release mode, `#[ignore]`d by default, on a file-backed database with `synchronous = NORMAL`. The numbers are the 95th percentile over 1,000 runs. Each run prints the machine (model, chip and macOS version) next to the numbers.

  | What | Target |
  |---|---|
  | `tree()`: 2,000 tasks with waiting flags, repos, activity and 1,000 dependencies | under 10 ms |
  | `snapshot()`: tree, edges, ready and latest event id, on that 2,000-task tree | under 8 ms |
  | `ready()`, on that 2,000-task tree | under 8 ms |
  | `add_task` | under 1 ms |
  | `set_status`, event included | under 1 ms |
  | `add_dependency`, loop check included, on that 2,000-task tree | under 2 ms |
  | `run_order`: a parent with 200 subtasks and 400 dependencies | under 2 ms |
- **Coverage:** no percentage is required. Every public function and every rule needs at least one test. Every write path needs an atomicity test.

## Boundaries

- **Always:**
  - Run `fmt`, `clippy` and the tests before every commit.
  - Write events in the same transaction as the change.
  - Take the actor as an argument and never guess it.
  - Keep `core` free of network calls and of Tauri.
- **Ask first:**
  - Adding any dependency.
  - Changing a rule or one of the permission tables.
  - Changing the schema once `mcp` ships. That needs a new migration file, never an edited one.
- **Never:**
  - Read or write T3's or Claude Code's files from `core`.
  - Expose raw SQL or the connection to other modules.
  - Edit or delete an event, except through deleting its task.

## Success criteria

- [x] `cargo test -p airboss-core` passes, with tests for every rule, every row of the rule 6 table and every "no" in the rule 8 table.
- [x] An agent can't accept its own proposal, reopen a `done` or `dropped` task, drop a task, or finish a task that has a PR.
- [x] `sync:gh` finishes a task only when none of its PRs is open and at least one has merged.
- [x] Linking an open PR to a `done` task, or reopening its PR, moves it back to `in_review`.
- [x] Rejected proposals are `dropped`, are hidden from `tree()`, and are still in history.
- [x] Dependency loops of any length are rejected, including across parents, and nothing is written.
- [x] `run_order` returns the expected waves for the diamond, the cross-phase case and the outside-blocker case.
- [x] Running a plan twice changes nothing, and a revised plan adds only its new steps.
- [x] A call that fails partway leaves no rows, no events and no notification.
- [x] Each performance check meets its target, with the machine recorded next to the numbers.
- [x] `clippy -D warnings` and `fmt --check` are clean.
- [x] Opening an existing database runs only the pending `core` migrations, once, and keeps the data.

v2:

- [x] A task links to several repos; `Task.repos` and `tree()` list them by name; only `me` and `plan` change them, each change with its own event.
- [x] `set_branch` stores, changes and clears a branch for `me` and `launch` only, with one `edited` event per change.
- [x] Links keep an optional title.
- [x] `history_with_subtasks` returns a subtree's events newest first, up to the limit.
- [x] `dependencies` lists both directions, finished blockers included.
- [x] `waiting_outside` names each subtask's outside blockers.
- [x] Every `tree()` node carries its own last event time and actor, and its subtree's last event time, and `tree()` on 2,000 tasks still meets 10 ms: 4.3 ms at the 95th percentile on a Mac16,1 (Apple M4), macOS 27.2.1 (25 Nov 2026).
- [x] `ready()` says since when each task has been ready.
- [x] Every new write path has an atomicity test.

v3:

- [ ] `snapshot()` returns the tree, edges, ready list and newest event id from one read transaction, building the tree once, in under 8 ms on the 2,000-task tree.
- [ ] `ready()` builds the tree once.
- [ ] `dependency_edges`, `latest_event_id`, `data_version`, `links_of_kind` and `path` work as rules 23, 30 and 31 say.
- [ ] `migrate_module` runs another module's migrations once, and refuses `core`.
- [ ] Every PR ref is stored as lowercase `owner/repo#n`, URLs are accepted, and malformed refs are refused.
- [ ] `set_branch` refuses every branch name rule 25 rules out.
- [ ] `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` are clean, `mcp` included.

## Decisions (settled 25 Nov 2026)

- **Sibling order:** whole numbers, renumbered on every add, move and delete (rule 13).
- **Parent done:** never automatic. `attention` suggests it (rule 10).
- **`sync:t3`:** never changes status, to keep v1 simple (rule 6).
- **Dropping a parent** drops its open subtree (rule 9).
- **Agents finish only PR-less work,** with a note. Code finishes when its PRs merge (rules 6 and 7).
- **Dependencies:** any task can wait on any other except its own ancestor or descendant (rules 14–17). Outside blockers, such as a PR in another repo or a Linear ticket, aren't tracked in v1: set `blocked` with a reason.
- **Run order:** shown, never started automatically (rule 18).
- **Rejecting a proposal** drops it rather than deleting it, so history keeps it (rule 6).

## v2 (25 Nov 2026)

The app design ([DESIGN.md](DESIGN.md), "Needs from the core data model") asked for eight things `core` didn't have. The EM decided them, and they're folded into the schema, rules, tables and API above.

1. **Several repos per task.** `task_repos` replaces `tasks.repo_id`. `Task.repos` replaces `Task.repo`, and `edit_task` no longer takes a repo. New calls `link_repo` and `unlink_repo`, with `repo_linked` and `repo_unlinked` events (rules 3, 5, 8 and 24).
2. **Branch.** `tasks.branch` and `set_branch`, for `me` and `launch` (rules 8 and 25).
3. **Link titles.** `links.title` and `NewLink::with_title`.
4. **History for a subtree.** `history_with_subtasks(id, limit)` (rule 28).
5. **Every dependency in both directions.** `dependencies(id)` (rule 27).
6. **Outside blockers in run order.** `waiting_outside` entries name their blockers (rule 18).
7. **Activity on tree nodes.** `last_event_at`, `last_actor` and `subtree_last_event_at`, with `tree()` still under 10 ms (rule 28).
8. **When a task became ready.** `ready()` returns `ReadyTask { task, since }` (rule 26).

**Decisions made while building v2:**

- `link_repo` and `unlink_repo` return `()`, like the dependency calls. `set_branch` returns the task, like `edit_task`.
- Unlinking a repo that isn't linked is `NotFound`, like removing a dependency that isn't there.
- A plan sent again doesn't touch an existing step's repos (rule 24).
- A blank branch name clears the branch, since no git branch can be blank (rule 25).
- A blank link title is stored as no title. Linking the same thing again returns the link as it was, title included.
- An outside blocker reaches a subtask through a sibling too, so every `waiting_outside` entry names at least one blocker (rule 18).
- "Newest" and "last" go by event id, as everywhere else; times are only shown (rules 26 and 28).
- A task freed by removing or deleting its blocker is ready since that removal, not since its creation (rule 26).

## v3 (26 Nov 2026)

The app and service specs asked for a few more things. The EM decided them in [tasks/program.md](tasks/program.md) ("EM decisions on the spec reviews"), and they're folded into the rules, API and tests above.

1. **One consistent read.** `snapshot() -> CoreSnapshot { tree, edges, ready, latest_event_id }`, under 8 ms on the 2,000-task tree, so services and the app never build the tree several times per change. `ready()` builds the tree once too (rule 30).
2. **Smaller reads.** `dependency_edges()`, `latest_event_id()`, `links_of_kind(kind)` and `path()` (rule 30).
3. **Writes from other processes.** `data_version()` (rule 23).
4. **Other modules' migrations.** A public `Migration` and `migrate_module(module, migrations)`, which refuses `core` (rule 31).
5. **Canonical PR refs.** Lowercase `owner/repo#n`, with GitHub URLs accepted, and `002_pr_refs.sql` for refs stored before (rule 29).
6. **Branch names** limited to `[A-Za-z0-9._/-]`, with git's worst shapes refused (rule 25).

**Decisions made while building v3:**

- `path()` is SQLite's absolute path for the file, not the path as given to `open`, so a service opening its own `Store` on it doesn't depend on the working directory.
- `002_pr_refs.sql` lowercases PR refs stored in `owner/repo#n` form. Where a task already has the lowercase twin, the old row goes and the twin's state stays. Rows in URL form were never valid, so it leaves them alone.
- `migrate_module` also refuses a blank module name and versions out of order, since either would make `schema_migrations` lie.
- Branch names follow only the rules the EM listed. Git has more (no `//`, no `@{`), and `git` itself refuses those when `launch` runs it.
- A PR URL must start `https://github.com/` (any case). Other hosts and `http://` are `InvalidTarget`.

## Open questions

- **Event ids can be reused.** `task_events.id` is a plain `INTEGER PRIMARY KEY`, so when the newest events go with a deleted task, SQLite hands their ids out again. A reader holding one of those ids as a cursor, or as `latest_event_id`, then misses the new events. Rule 23's "event ids only grow" needs `AUTOINCREMENT`, which takes a migration that rebuilds `task_events`. That's a schema change, so it waits for the EM. Until then, `events_after` and `latest_event_id` can't be trusted across a delete. The app uses its own patch sequence number, so it isn't affected.

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
