# Implementation plan: core

Module `core` from [CAPABILITIES.md](../CAPABILITIES.md). It builds what's in [SPEC-core.md](../SPEC-core.md), approved 25 Nov 2026. The tasks and their checkpoints are in [todo.md](todo.md).

State: **approved 25 Nov 2026**. Building starts when I say "build it".

## Overview

`core` is a Rust library crate, `airboss-core`. It holds the task tree, history, links and dependencies in SQLite, and it enforces 23 rules about who may change what. Nothing else in Airboss exists yet, so `core` is built and tested on its own, against an in-memory database.

The work is cut into slices that each add one behaviour, end to end: schema, Rust API, rule check and tests. That way every task leaves a crate that builds and passes its tests, and each task's tests are written before its code.

## Architecture decisions

- **One crate, one `Store`.** All writes go through `Store` methods, which take the actor as an argument. Each call is one SQLite transaction. Its events are written inside that transaction, and the change notification is sent only after the commit (rules 21 and 23).
- **Permission checks are pure functions.** The checks for rule 6 (status changes) and rule 8 (every other call) are plain functions over a small `TaskFacts` struct, loaded inside the transaction. Every row of the two tables can then be tested without setting up a full scenario.
- **The loop check is a recursive SQL query.** It follows "waits on" across the whole tree in one query instead of loading the graph into Rust, which keeps it under the 2 ms target on 2,000 tasks.
- **Run order is computed in Rust.** It loads one parent's subtree and its dependencies in two queries, then applies Kahn's algorithm in memory. Dependencies between deeper tasks count as dependencies between the parent's direct subtasks (rule 18).
- **Migrations are SQL files built into the crate**, recorded in `schema_migrations` under the module name `core`. The runner is generic, so `sync` and `launch` can reuse it later.
- **No new dependencies** beyond `rusqlite` (with the `bundled` feature), `serde` and `thiserror`.

## Dependency graph

```
T1 workspace
 └─ T2 open + migrations
     └─ T3 add, read, history, repos
         ├─ T4 edit + notes
         ├─ T5 links + PR state
         │   └─ T6 status changes (rules 1, 4, 6, 7)
         │       ├─ T7 proposals
         │       └─ T9 drop subtree + delete
         ├─ T8 move + positions
         │   └─ T9
         └─ T10 dependencies + loop check
             ├─ T11 run order
             └─ T12 plans (add_tasks)
T13 change notification   (after T3; best after T12, so every write path exists)
T14 performance checks    (after T12)
```

## Phases

1. **Foundation (T1–T3):** the workspace, opening the database, and the first slice: add a task, read it, see its history.
2. **Status that stays honest (T4–T7):** edits, links with PR state, the status table, and proposals.
3. **Tree shape (T8–T9):** moving and renumbering, dropping a subtree, deleting.
4. **Dependencies and plans (T10–T12):** waiting, loops, run order, and plans arriving as trees.
5. **Finish (T13–T14):** change notification and the performance checks.

There's a checkpoint after each phase, where I review with you before going on.

## Before task 1

- Rust 1.100 has to be installed and on your PATH. The Mac has 1.98.1 through rustup, but `~/.cargo/bin` is missing. Run `rustup update` and add rustup's toolchain directory to your PATH. Task 1 then checks for 1.100 through `rust-toolchain.toml`.

## Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Renumbering siblings breaks a partial order during a move | Medium | No unique index on position. Renumber with one `UPDATE … ROW_NUMBER()` query per sibling group inside the transaction, and test it with property-style random moves (T8). |
| The loop check is slow on deep or wide graphs | Medium | Recursive CTE with `UNION`, so each task is visited once, using the index on `task_deps(waits_on_id)`. A perf check is added in T10, not only in T14. |
| Lifting deep dependencies to their top-level subtask in run order hides a loop between siblings | Medium | Lifting can create a loop between siblings even when the tree has none, for example when steps inside Phase 1 and Phase 2 each wait on the other phase. Done: run order no longer fails. The subtasks Kahn's algorithm can't place go in their own `tangled` group after the waves, in sibling order, so the rest of the order still shows and no task is dropped. A test covers it. |
| One `Store` behind a mutex blocks MCP behind a slow UI read | Low | Every call is a millisecond or less (T14 measures this). If that changes, open a second, read-only connection for reads, which WAL mode allows. |
| `rusqlite` 0.43 behaves differently from what's assumed | Low | T2 pins it and tests the settings and migrations first, so any surprise shows up in the second task. |

## Parallel work

- **Can run in parallel:**
  - After T3: T4, T5, T8 and T10.
  - After T10: T11 and T12.
  - The design thread, which is already running.
- **Must run in order:** T1 → T2 → T3, and T5 → T6 → T7.
- **Recommendation:** build in order, one task at a time, in a single thread. The crate is small, and parallel agents would keep changing the same `lib.rs` and migration file.

## Open questions

None. The one thing outside `core` is who proves an actor's identity, and that belongs to the `mcp` spec.
