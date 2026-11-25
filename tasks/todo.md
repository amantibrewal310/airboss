# Tasks: core

The plan is in [plan.md](plan.md), and the rules are in [SPEC-core.md](../SPEC-core.md). "Rule N" means rule N of that spec.

Every task meets the Definition of Done:

- tests are written first and pass
- `cargo clippy -p airboss-core -- -D warnings` and `cargo fmt --check` are clean
- no `unwrap` or `expect` outside tests

The focused test command is `cargo test -p airboss-core`.

Sizes: XS = 1 file, S = 1–2 files, M = 3–5 files.

## Phase 1: Foundation

- [x] **T1. Workspace and empty crate** (S)
  - **Description:** Set up a Cargo workspace with `rust-toolchain.toml` pinning Rust 1.100 and the `crates/core` library crate (`airboss-core`, edition 2024). Add the `core::Error` enum (`NotFound`, `NotAllowed`, `InvalidTransition`, `NoteRequired`, `InvalidTarget`, `Loop`, `DuplicateKey`, `Db`) and wrapper types for ids (`TaskId`, `EventId`, `LinkId`, `RepoId`).
  - **Acceptance:**
    - `cargo build` uses Rust 1.100.
    - The crate compiles with `rusqlite` 0.43 (`bundled`), `serde` 1 and `thiserror` 2, and nothing else.
  - **Verify:** `cargo test -p airboss-core` (no tests yet, passes); clippy and fmt clean; `rustc --version` inside the repo shows 1.100.
  - **Files:** `Cargo.toml`, `rust-toolchain.toml`, `crates/core/Cargo.toml`, `crates/core/src/lib.rs`, `crates/core/src/error.rs`
  - **Depends on:** none

- [x] **T2. Open the database and run migrations** (M)
  - **Description:**
    - `Store::open(path)` and `Store::open_in_memory()` apply the settings outside any transaction: WAL, `synchronous = NORMAL`, foreign keys on, `busy_timeout = 2000`.
    - They run any pending `core` migrations from `001_core.sql`, which holds the schema exactly as in the spec.
    - A generic runner records each migration in `schema_migrations` under its module name, inside the migration's own transaction.
  - **Acceptance:**
    - Opening a new file creates all tables and indexes.
    - Opening it again runs nothing and keeps the data.
    - A second module's migration recorded in the same table doesn't confuse `core`'s runner.
  - **Verify:**
    - Tests: settings applied (read back with `PRAGMA`), migrate-once, data kept across reopen, a foreign module's rows ignored.
    - The 10 constraint rejections from the spec's schema test, rewritten as Rust tests.
  - **Files:** `src/db.rs`, `src/migrations/001_core.sql`, `src/lib.rs`, `tests/db.rs`
  - **Depends on:** T1

- [x] **T3. Add a task, read it back, see its history** (M)
  - **Description:** The first vertical slice:
    - `add_task` as `me`: the task lands at the end of its siblings and gets a `created` event.
    - `task`, `tree` (in order) and `history`.
    - `add_repo` and `repos`.
    - The `Actor` enum and how it's stored as text.
  - **Acceptance:**
    - Rule 2: a blank title, or a parent that doesn't exist, is rejected.
    - Rule 21: one event per call, in the same transaction.
    - A failure forced after the insert leaves no task and no event.
    - `task` on a missing id returns `NotFound`.
  - **Verify:** tests in `tests/rules.rs` and `tests/tree.rs` (adding 5 levels deep, reading the tree back in order).
  - **Files:** `src/tasks.rs`, `src/events.rs`, `src/lib.rs`, `tests/rules.rs`, `tests/tree.rs`
  - **Depends on:** T2

### Checkpoint 1: Foundation

- [ ] All tests pass; clippy and fmt are clean.
- [ ] Review the API's look and feel with you before the rules pile on.

## Phase 2: Status that stays honest

- [x] **T4. Edit tasks and add notes** (S)
  - **Description:** `edit_task` (title, notes, repo) for `me` only. `add_note` for every actor, writing a `note` event.
  - **Acceptance:**
    - Rule 8, the `edit_task` and `add_note` rows: an agent editing gets `NotAllowed`.
    - Rule 4: a note event always has text.
    - Editing writes one `edited` event.
  - **Verify:** one test per "no" in those rows.
  - **Files:** `src/tasks.rs`, `src/permissions.rs`, `tests/rules.rs`
  - **Depends on:** T3

- [x] **T5. Links and PR state** (M)
  - **Description:**
    - `link`: linking the same thing twice returns the existing link.
    - `unlink`: `me` can remove any link; an agent only links it added.
    - `links`, `tasks_for_link` and `set_pr_state` (`me` or `sync:gh`; it returns the tasks affected).
    - Each change writes a `linked`, `unlinked` or `link_state` event.
  - **Acceptance:**
    - Rule 3: a PR link always has a state, and no other link has one.
    - Rule 8: the `link`, `unlink` and `set_pr_state` rows.
    - `tasks_for_link` finds a task by its PR ref.
  - **Verify:** tests in `tests/rules.rs` and a new `tests/links.rs`.
  - **Files:** `src/links.rs`, `src/permissions.rs`, `tests/links.rs`, `tests/rules.rs`
  - **Depends on:** T3

- [x] **T6. Status changes** (M)
  - **Description:** `set_status` with the rule 6 table as a pure function over `TaskFacts` (status, has a PR, all PRs merged or closed, any PR merged). Linking a PR to a `done` task as anyone but `me` moves it back to `in_review` (rule 7).
  - **Acceptance:**
    - Every row of the rule 6 table passes, and every case it rejects fails with the right error (`NotAllowed`, `InvalidTransition`, `NoteRequired`).
    - Rule 1: blocked needs a reason, and leaving blocked clears it.
    - Rule 7 holds.
  - **Verify:** table-driven tests, one case per row, plus each rejected case.
  - **Files:** `src/status.rs`, `src/links.rs`, `tests/status.rs`
  - **Depends on:** T5

- [x] **T7. Proposals** (S)
  - **Description:**
    - When an agent calls `add_task`, the task is always created as `proposed`.
    - `proposed()` lists them.
    - `me` accepts a proposal by moving it to `todo`, or rejects it by moving it to `dropped` with a reason.
    - `tree()` hides rejected proposals, but history keeps them.
  - **Acceptance:**
    - An agent can't move its own proposal anywhere.
    - A rejected proposal is gone from `tree()` but still in `history()`.
  - **Verify:** tests in `tests/rules.rs`.
  - **Files:** `src/tasks.rs`, `src/status.rs`, `tests/rules.rs`
  - **Depends on:** T6

### Checkpoint 2: Status

- [ ] All tests pass.
- [ ] Walk through the rule 6 and rule 8 tables with you against the tests. Every row has a test.

## Phase 3: Tree shape

- [x] **T8. Move tasks and keep positions gap-free** (M)
  - **Description:** `move_task` (`me` only).
    - Rule 11: no cycles.
    - Rule 12: no moving under a `proposed`, `done` or `dropped` parent. The same check applies when adding a task.
    - Rule 13: positions start at 1 and are clamped; the old and new sibling groups are renumbered; only the moved task gets a `moved` event.
  - **Acceptance:**
    - Positions stay 1..n for every group after random moves.
    - A move into a task's own subtree is rejected.
    - A move under a `done` parent is rejected.
  - **Verify:** tests in `tests/tree.rs`, including 500 random moves that check the 1..n invariant after each one.
  - **Files:** `src/tasks.rs`, `src/positions.rs`, `tests/tree.rs`
  - **Depends on:** T3 (and T6 for the parent-status checks)

- [x] **T9. Drop a subtree, delete a task** (S)
  - **Description:**
    - Dropping a task (`me` only) also drops its open subtasks, with one event each; `done` subtasks stay `done` (rule 9).
    - `delete_task` is `me` only: it cascades (rule 5) and renumbers the siblings.
  - **Acceptance:**
    - Dropping a 3-level subtree drops every open task in it, and `done` ones stay `done`.
    - After a delete, the subtree, its events and its links are gone, and sibling positions are gap-free.
  - **Verify:** tests in `tests/rules.rs` and `tests/tree.rs`.
  - **Files:** `src/status.rs`, `src/tasks.rs`, `tests/rules.rs`, `tests/tree.rs`
  - **Depends on:** T6, T8

### Checkpoint 3: Tree

- [ ] All tests pass.
- [ ] Rules 1–13 and 21–22 each have a named test.

## Phase 4: Dependencies and plans

- [x] **T10. Dependencies, loops and waiting** (M)
  - **Description:**
    - `add_dependency` and `remove_dependency` (`me` and `plan` only), each writing an event on the waiting task.
    - Rule 14: a recursive query checks for loops across the whole tree.
    - Rule 15: no dependency between a task and its own ancestor or descendant.
    - `waits_on`, `ready`, and the waiting flag in `tree()` (rule 16).
    - Deleting a blocker writes an event on each task that was waiting (rule 17).
  - **Acceptance:**
    - Loops of length 2, 3 and 5 are rejected, including across different parents.
    - Parent-child dependencies are rejected.
    - A task shows as waiting until its blocker is `done` or `dropped`, then appears in `ready()`.
  - **Verify:** tests in `tests/deps.rs`, plus an early perf check: `add_dependency` on a 2,000-task tree in under 2 ms.
  - **Files:** `src/deps.rs`, `src/tasks.rs`, `tests/deps.rs`
  - **Depends on:** T3 (and T9 for the delete case)

- [x] **T11. Run order** (S)
  - **Description:** `run_order(parent)` returns `{ waves, waiting_outside }` (rule 18).
    - Dependencies between deeper tasks count as dependencies between the parent's direct subtasks.
    - Kahn's algorithm makes the waves, and tasks keep their sibling order within a wave.
    - `done` and `dropped` subtasks are left out.
    - If lifting dependencies creates a loop between siblings, it returns `Error::Loop`.
  - **Acceptance:** the expected result for each of these cases:
    - a diamond: B and C wait on A, and D waits on B and C, giving [A], [B, C], [D]
    - steps in Phase 2 waiting on steps in Phase 1, so Phase 2 comes after Phase 1
    - a subtask with an open blocker outside the parent, which lands in `waiting_outside`
    - a loop created only by lifting, which returns `Error::Loop`
  - **Verify:** tests in `tests/deps.rs`.
  - **Files:** `src/deps.rs`, `tests/deps.rs`
  - **Depends on:** T10

- [x] **T12. Plans arrive as trees** (M)
  - **Description:** `add_tasks(parent, plan, actor)` for `me` and `plan`.
    - It takes a tree of `PlanTask` items, each with a `key`, children and `depends_on` entries (another key in the same plan, or an existing task id).
    - Every check runs before anything is written (rule 19).
    - Keys that already exist resolve to the existing task (rule 20).
  - **Acceptance:**
    - A three-level plan with dependencies inside it is created.
    - The same plan sent twice changes nothing.
    - A revised plan with one renamed step and one new step adds only the new step.
    - A plan containing a loop writes nothing.
    - A key used twice in one plan returns `DuplicateKey`.
  - **Verify:** tests in a new `tests/plans.rs`.
  - **Files:** `src/plans.rs`, `src/lib.rs`, `tests/plans.rs`
  - **Depends on:** T10

### Checkpoint 4: Dependencies and plans

- [ ] All tests pass.
- [ ] You review the run-order results for a real plan, such as the Toffee migration phases, written as a test fixture.

## Phase 5: Finish

- [x] **T13. Change notification and the event cursor** (S)
  - **Description:**
    - `subscribe()` returns a channel that receives the new event ids after each commit (rule 23).
    - `events_after(cursor)` returns history in event-id order.
    - Nothing is sent when a call fails.
  - **Acceptance:**
    - One call sends exactly the ids of the events it wrote.
    - A failed call sends nothing.
    - Two subscribers both receive every notification.
  - **Verify:** tests in a new `tests/events.rs`.
  - **Files:** `src/events.rs`, `src/lib.rs`, `tests/events.rs`
  - **Depends on:** T12, so that every write path exists

- [x] **T14. Performance checks** (S)
  - **Description:** Add `tests/perf.rs` (`#[ignore]`, release mode, a file-backed database). It times 1,000 runs of each check in the spec and prints the 95th percentile next to the machine (model, chip and macOS version).
  - **Acceptance:** each target in the spec's table is met on your Mac. Any miss is fixed, or brought to you before the target is changed.
  - **Verify:** `cargo test -p airboss-core --release -- --ignored perf`, with the output pasted into the PR or commit.
  - **Files:** `tests/perf.rs`, `tests/common/mod.rs` (fixture builders)
  - **Depends on:** T12

### Checkpoint 5: core complete

- [ ] Every success criterion in `SPEC-core.md` is checked off, with the test that proves it.
- [ ] `CAPABILITIES.md` is updated to show `core` as built.
- [ ] Review with you, then `code-review-and-quality` and `code-simplification` before calling `core` done.
