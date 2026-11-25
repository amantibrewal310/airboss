# Spec: core

Module `core` from [CAPABILITIES.md](CAPABILITIES.md). State: **draft, waiting for review**.

## Objective

`core` is the single source of truth for tasks. It stores the task tree, every change made to a task, and the outside things linked to each task. It enforces the rules that keep status honest.

Every other module reads and writes tasks only through `core`: `mcp` for agents, `app` for me, `sync` for GitHub and T3, `launch` for new threads. None of them touches the tables directly.

**User stories**

- As me, I add a task of any size and nest it anywhere: a bug fix stays one task, a product gets phases and steps under it.
- As me, I can see the history of every task: who changed what, when, and why.
- As an agent, I can move my task to `doing`, `in_review` or `blocked`, add notes and links, and propose subtasks. I can mark a task `done` only if it has no linked PR, and I must say what I did. I can't create subtasks directly.
- As `sync`, I can mark a task done when its linked PR merges.
- As a settled plan (`add_tasks` with actor `Plan`), my phases and steps are created straight away as subtasks.

## Tech stack

- **Rust edition 2024**, the latest edition, on stable Rust 1.100 (pinned in `rust-toolchain.toml`). `core` is a library crate, `airboss-core`, with no Tauri dependency, so it builds and tests fast on its own.
- **Dependencies:** `rusqlite` 0.43 (the `bundled` feature, so it carries its own SQLite), `serde` 1 (for types that `mcp` and `app` send as JSON) and `thiserror` 2. Nothing else without asking.
- **SQLite settings,** applied on open: WAL mode, `foreign_keys = ON`, `busy_timeout = 2000`. Schema versions are tracked with `PRAGMA user_version` and SQL files embedded in the crate.
- **Time:** UTC text written by SQLite, `strftime('%Y-%m-%dT%H:%M:%fZ','now')`.

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
crates/core/
  Cargo.toml               airboss-core
  src/lib.rs               Public API: Store and the types it uses
  src/db.rs                Opening the database, settings, migrations
  src/migrations/001_core.sql
  src/tasks.rs             Add, edit, move, delete, read the tree
  src/status.rs            Status changes and the rules
  src/links.rs             Links
  src/events.rs            History
  tests/rules.rs           One test per rule (see Testing)
  tests/tree.rs            Nesting, moving, ordering, cycles
  tests/perf.rs            #[ignore] performance checks
```

Later modules get their own crates next to it (`crates/mcp`, `crates/sync`, `crates/launch`), plus `app/` for the Tauri shell and Svelte UI. Each module that needs tables owns its own migrations. For example, `sync`'s tables for observed threads and PRs live in `crates/sync`, not in `core`.

## Data model

Five tables, starting from the draft-2 schema tested on 25 Nov 2026 in a throwaway in-memory SQLite database. The changes since then:

- `proposed` added as a status.
- `planning` dropped as a status: `launch` records that a planning thread is running as a link and an event.
- `observed_*` tables moved out to `sync`.
- `task_deps` added for "this task waits on that one". Tested the same way on 25 Nov.

| Table | One row is | Notes |
|---|---|---|
| `repos` | A git repo tasks can point at | `name` and `path` are unique; `t3_project_id` is null until `launch` first uses the repo |
| `tasks` | One thing to get done, at any depth | `parent_id` is null for a top-level task; `repo_id` is null when the task isn't in one repo |
| `task_events` | One change to a task | Written in the same transaction as the change it records |
| `task_deps` | One task waiting on another task | Any two tasks, anywhere in the tree; a task can't wait on itself, and the same pair is stored once. Deleting either task removes the row |
| `links` | One outside thing attached to a task | Kinds: `t3_thread`, `claude_session`, `pr`, `slack`, `doc`, `github_issue`; the same `(task, kind, ref)` can only be linked once |

**Statuses:** `proposed`, `todo`, `doing`, `in_review`, `blocked`, `done`, `dropped`.

**Actors:** `me`; `agent:<thread ref>`; `sync:gh`; `sync:t3`; `plan`; `launch`.

## Rules

**The database enforces these:**

1. A blocked task has a reason, and a task that isn't blocked has none.
2. A task can't be its own parent, and a parent must exist.
3. A link can't be added twice.
4. A status event always names the new status.
5. Deleting a task deletes its subtree, its events and its links.

**`core`'s code enforces these, each with its own test:**

6. **Who can set `done`:**
   - `me`, always.
   - `sync:gh`, when the task has a linked PR that merged.
   - An agent, only when the task has **no** linked PR, and only with a note saying what was done (e.g. "Wrote docs/booking-api.md"). A task with a PR finishes only when the PR merges, so unmerged code is never `done`.
   - Anyone else gets `Error::NotAllowed`. A missing note gets `Error::NoteRequired`.
7. **Agents can set only `doing`, `in_review`, `blocked`, and `done` under rule 6.** They can't set `todo`, `proposed` or `dropped`.
8. **An agent's new subtasks start as `proposed`.** Only `me` can accept them (moving them to `todo`) or reject them (deleting them).
9. **`plan` creates subtasks as `todo` directly,** because I settled the plan.
10. **No cycles.** A task can't be moved under itself or under one of its own subtasks.
11. **Every change writes one event per task it changes, in the same transaction.** This covers creating, editing, moving, status changes, links and dependencies (recorded on the waiting task). If the change fails, no event is written. A move writes one event, for the moved task only: renumbering its siblings writes none.
12. **Adding the same plan twice adds nothing new.** `add_tasks` skips a subtask whose title already exists under the same parent.
13. **Deleting is only for `me`.** Agents and sync can drop a task, but never delete one.
14. **`sync:t3` never changes status.** It only adds events and links. Status comes from agents, me and merged PRs.
15. **Dropping a task drops its whole subtree** as one change, with an event for each task dropped. Subtasks already `done` stay `done`.
16. **A parent is never finished automatically.** When its last subtask becomes `done`, the parent stays as it is. `attention` suggests finishing it, and I decide.
17. **Siblings are ordered by whole-number positions (1, 2, 3, …).** Positions are renumbered on every move, so they stay gap-free.
18. **No dependency loops.** If A waits on B, B can't wait on A, directly or through other tasks. The database rejects a task waiting on itself, and the code rejects longer loops.
19. **"Waiting" is worked out, never stored.** A task is waiting while any task it waits on is neither `done` nor `dropped`. Waiting doesn't change a task's status. When its last blocker finishes, `attention` shows it as "ready to start".
20. **`me` and `plan` add or remove dependencies.** Agents don't, in v1. An agent that's stuck sets `blocked` with a reason.
21. **A plan brings its dependencies with it.** `add_tasks` takes each new task's "Depends on" as other tasks in the same batch, or as existing task ids, and checks rule 18 before writing anything.
22. **Deleting a task that others wait on** removes those dependencies and writes an event on each waiting task.

## API shape

This is the public interface the other modules call. Exact signatures are settled during planning.

```rust
pub struct Store { /* one rusqlite::Connection */ }

impl Store {
    pub fn open(path: &Path) -> Result<Store>;
    pub fn open_in_memory() -> Result<Store>;

    pub fn add_task(&mut self, new: NewTask, actor: &Actor) -> Result<Task>;
    pub fn add_tasks(&mut self, parent: TaskId, titles: &[NewTask], actor: &Actor) -> Result<Vec<Task>>;
    pub fn edit_task(&mut self, id: TaskId, edit: TaskEdit, actor: &Actor) -> Result<Task>;
    pub fn move_task(&mut self, id: TaskId, parent: Option<TaskId>, position: u32, actor: &Actor) -> Result<()>;
    pub fn set_status(&mut self, id: TaskId, to: Status, note: Option<&str>, actor: &Actor) -> Result<Task>;
    pub fn delete_task(&mut self, id: TaskId, actor: &Actor) -> Result<()>;

    pub fn task(&self, id: TaskId) -> Result<Option<Task>>;
    pub fn tree(&self) -> Result<Vec<TaskNode>>;            // whole tree, in order
    pub fn proposed(&self) -> Result<Vec<Task>>;
    pub fn history(&self, id: TaskId) -> Result<Vec<Event>>;
    pub fn changed_since(&self, at: &str) -> Result<Vec<Event>>;

    pub fn link(&mut self, id: TaskId, link: NewLink, actor: &Actor) -> Result<Link>;  // linking twice returns the existing link
    pub fn links(&self, id: TaskId) -> Result<Vec<Link>>;

    pub fn add_dependency(&mut self, task: TaskId, waits_on: TaskId, actor: &Actor) -> Result<()>;
    pub fn remove_dependency(&mut self, task: TaskId, waits_on: TaskId, actor: &Actor) -> Result<()>;
    pub fn waits_on(&self, id: TaskId) -> Result<Vec<Task>>;   // its open blockers
    pub fn ready(&self) -> Result<Vec<Task>>;                  // `todo` with no open blockers
}
```

`Store` isn't shared between threads by itself. The app holds one `Store` behind a mutex. Every call is short, so `mcp`, `app` and `sync` waiting on one lock stays well under the 20 ms MCP target.

## Code style

```rust
/// Agents can move work forward and finish PR-less work; code finishes only when its PR merges.
fn check_allowed(actor: &Actor, task: &TaskFacts, to: Status) -> Result<()> {
    match (actor, to) {
        (Actor::Me, _) => Ok(()),
        (Actor::SyncGh, Status::Done) if task.pr_merged => Ok(()),
        (Actor::Agent(_), Status::Done) if !task.has_pr => Ok(()), // note checked by caller
        (Actor::Agent(_), Status::Doing | Status::InReview | Status::Blocked) => Ok(()),
        _ => Err(Error::NotAllowed { actor: actor.clone(), to }),
    }
}
```

- `rustfmt` defaults, and clippy clean with `-D warnings`.
- No `unwrap` or `expect` outside tests. Errors use one `thiserror` enum, `core::Error`.
- SQL lives in `.sql` files or `const` strings next to the function that runs it. No ORM.
- Names: `TaskId(i64)` rather than a bare `i64`; statuses and actors are enums, never strings, in Rust code.
- A short doc comment on every public item, saying why, not what.

## Testing strategy

- Tests come first: one failing test per rule (1–22) before its code. Rule tests live in `tests/rules.rs`, named after the rule, e.g. `agent_cannot_finish_task_with_pr`.
- Every test uses `Store::open_in_memory()`, so no test touches disk or another test's data.
- Tree tests cover nesting 5 levels deep, moving a subtree, reordering, and the cycle check.
- Dependency tests cover a chain (A waits on B waits on C), a loop rejected at every length, a plan batch with dependencies inside it, and a waiting task becoming ready when its blocker finishes.
- **Performance checks** run in release mode, `#[ignore]`d by default:
  - Load the whole tree of **2,000 tasks**, with each task's waiting state and 1,000 dependencies, in **under 10 ms**.
  - Add a task in **under 1 ms**.
  - Change a status, event included, in **under 1 ms**.
  - These numbers are the 95th percentile over 1,000 runs on a file-backed database, measured on my Mac.
- No coverage percentage is required. Every public function and every rule needs at least one test.

## Boundaries

- **Always:**
  - Run `fmt`, `clippy` and the tests before every commit.
  - Write an event with every change.
  - Take the actor as an argument and never guess it.
  - Keep `core` free of network calls and of Tauri.
- **Ask first:**
  - Adding any dependency.
  - Changing a rule (6–22).
  - Changing the schema once the app is in daily use; that needs a new migration file, never an edited one.
- **Never:**
  - Read or write T3's or Claude Code's files from `core`.
  - Expose raw SQL or the connection to other modules.
  - Delete events. History is append-only.

## Success criteria

- [ ] `cargo test -p airboss-core` passes, with a test for every rule 1–22.
- [ ] Agents get `NotAllowed` for `done` on a task with a linked PR, for `todo`, and for deleting a task.
- [ ] An agent can set `done` on a task with no PR only with a note; without one it gets `NoteRequired`.
- [ ] `sync:gh` can set `done` only on a task with a merged linked PR.
- [ ] An agent's subtasks come back from `proposed()` and don't appear as `todo` until I accept them.
- [ ] Running `add_tasks` with the same plan twice leaves the tree unchanged the second time.
- [ ] A failed change leaves no event and no partial write. Tested by forcing an error in the middle of a change.
- [ ] The performance checks meet their numbers in release mode.
- [ ] `clippy -D warnings` and `fmt --check` are clean.
- [ ] A dependency loop of any length is rejected, and nothing is written.
- [ ] A task that waits on an open task shows as waiting; once that task is `done` or `dropped`, it appears in `ready()`.
- [ ] Opening an existing database file runs any pending migrations once and keeps the data.

## Decisions (settled 25 Nov 2026)

- **Sibling order:** whole numbers, renumbered on a move (rule 17). Text keys only pay off for sync between devices or people, which Airboss doesn't do.
- **Parent done:** never automatic. `attention` suggests it (rule 16).
- **`sync:t3`:** doesn't touch status, for now, to keep it simple (rule 14).
- **Dropping a parent:** drops its subtree (rule 15).
- **Dependencies:** a task can wait on any other task (rules 18–22). Outside blockers, such as a PR in another repo or a Linear ticket, aren't tracked in v1: set `blocked` with a reason.

## Open questions

None.
