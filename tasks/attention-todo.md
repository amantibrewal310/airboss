# Tasks: attention

The plan is in [attention-plan.md](attention-plan.md), and the spec is [SPEC-attention.md](../SPEC-attention.md).

Every task meets the Definition of Done:

- tests are written first and pass
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean
- no `unwrap` or `expect` outside tests
- no test touches `gh`, `t3`, T3's database or the real Airboss database

The focused test command is `cargo test -p airboss-attention`.

Sizes: XS = 1 file, S = 1–2 files, M = 3–5 files.

## Phase 1: Foundation

- [ ] **T1. Crate, config and an empty `compute`** (S)
  - **Description:** Add `crates/attention` (`airboss-attention`) to the workspace's `members`, with `airboss-core`, `airboss-services` and `rusqlite` 0.43 `bundled`. `AttentionConfig` with its defaults, `Inputs`, `Hidden`, `Computed`, and `compute` returning an empty list.
  - **Acceptance:** the defaults are 5 days, 3 days, 7 days, 6 and 150 ms; empty inputs give no items and no `next_wake`.
  - **Verify:** `tests/kinds.rs` (first cases), `cargo test -p airboss-attention`.
  - **Files:** `Cargo.toml`, `crates/attention/Cargo.toml`, `src/lib.rs`, `src/compute.rs`, `tests/common/mod.rs`, `tests/kinds.rs`
  - **Depends on:** none

## Phase 2: Compute

- [ ] **T2. Questions, approvals and failed runs** (S)
  - **Description:** items from linked threads: `NeedsMe` with a question or an approval request, and `Errored`. One item per request or run, at the lowest open task id; `tasks` lists every open one.
  - **Acceptance:** each shows with its key, `since`, headline, context and target; a done task hides it; an untracked thread never shows; the lowest-task-id rule; `stale` follows the thread; fallbacks for a missing title, error or run id.
  - **Verify:** `tests/kinds.rs`.
  - **Files:** `src/kinds.rs`, `src/compute.rs`, `tests/kinds.rs`, `tests/common/mod.rs`
  - **Depends on:** T1

- [ ] **T3. Reviews** (XS)
  - **Description:** open, non-draft PRs without a problem, on an open task, with no `Running` or `NeedsMe` thread on any of its open tasks.
  - **Acceptance:** shows with "PR #n · title" and the diff, checks and review context; a draft, a merged PR, a PR with a problem, a done task and a running or waiting agent each hide it; `since` is `ready_since`.
  - **Verify:** `tests/kinds.rs`.
  - **Files:** `src/kinds.rs`, `tests/kinds.rs`
  - **Depends on:** T2

- [ ] **T4. Proposed, Ready and Finish it?** (S)
  - **Description:** proposals grouped per parent; ready tasks with at least one edge, inside the window, naming blockers; open parents whose non-dropped direct subtasks are all done.
  - **Acceptance:** one item per parent with the proposals in id order, a top-level proposal on its own; Ready with one, two and three blockers, a merged PR, no edges (hidden), an old `since` (hidden); Finish it? hidden by a proposed child, by only dropped children, by an open child, and for a closed parent.
  - **Verify:** `tests/kinds.rs`.
  - **Files:** `src/kinds.rs`, `tests/kinds.rs`
  - **Depends on:** T3

- [ ] **T5. Quiet and last activity** (S)
  - **Description:** last activity per subtree from `subtree_last_event_at`, linked threads (running or waiting counts as now) and linked PRs' `updated_at`; Quiet top-down so a quiet parent hides its quiet subtasks.
  - **Acceptance:** `doing`, `in_review` and `blocked` show after 5 days, `todo` never; a running thread or a recent PR on a subtask keeps the parent from Quiet; "N open PRs."; a quiet child under a quiet parent is hidden, under an active parent shown.
  - **Verify:** `tests/kinds.rs`.
  - **Files:** `src/quiet.rs`, `src/kinds.rs`, `tests/kinds.rs`
  - **Depends on:** T4

- [ ] **T6. Order, hiding and `next_wake`** (S)
  - **Description:** sort by group, then newest `since` (Quiet oldest), then task id, then key. Drop hidden keys (`until` null or later than now). `live_keys` is every candidate. `next_wake` per decision 5, always later than `now`.
  - **Acceptance:** `tests/order.rs` (group order, newest first, Quiet longest first, ties); `tests/time.rs` (exactly 5 days, `next_wake` equals activity plus 5 days, a Ready window ending, a snooze ending, the day count rolling over, nothing due gives `None`).
  - **Verify:** `tests/order.rs`, `tests/time.rs`.
  - **Files:** `src/compute.rs`, `tests/order.rs`, `tests/time.rs`
  - **Depends on:** T5

### Checkpoint 1

- [ ] Every kind and every hiding rule is tested through `compute` alone.

## Phase 3: Storage

- [ ] **T7. `attention_hidden` and its migration** (S)
  - **Description:** `src/migrations/001_attention.sql` exactly as the spec; `HiddenStore::open(path)` runs it through `migrate_module` and opens its own connection; load, hide (upsert), restore, prune. An unreadable table hides nothing; a write that fails is `Storage`.
  - **Acceptance:** each CHECK rejects its bad row; opening twice applies once; rows survive reopening.
  - **Verify:** `tests/migrations.rs`, `tests/hidden.rs` (storage part).
  - **Files:** `src/db.rs`, `src/hidden.rs`, `src/migrations/001_attention.sql`, `tests/migrations.rs`, `tests/hidden.rs`
  - **Depends on:** T1

## Phase 4: Service

- [ ] **T8. `AttentionWorker::handle`** (M)
  - **Description:** the state machine: inputs, hidden rows, the published list and `Changes`, settling, deadlines, pruning. `Hide` and `Restore` write, recompute and reply.
  - **Acceptance:** `tests/worker.rs`: a `Store` message reads nothing; 20 messages in a burst make one recompute; `version` and `changes()` move only on a visible change; `Woke` after a 6-day jump shows Quiet at once; the wait is capped at 15 minutes. `tests/hidden.rs`: dismiss hides until the key changes, snooze until `until`, restore, `NotFound`, pruning after 30 days.
  - **Verify:** `tests/worker.rs`, `tests/hidden.rs`.
  - **Files:** `src/worker.rs`, `src/hidden.rs`, `src/lib.rs`, `tests/worker.rs`, `tests/hidden.rs`
  - **Depends on:** T6, T7

- [ ] **T9. `start`, `Attention` and the threads** (M)
  - **Description:** `start(cfg, store, sync, clock)` reads one snapshot, opens the hidden table, spawns the worker and the `Changes` waiter; `Attention` implements `AttentionService`. Panics in `handle` are caught.
  - **Acceptance:** `tests/service.rs`: a sync change reaches `changes()` within 200 ms; a store change too, with the store's mutex poisoned after start; dismiss and snooze survive a restart; `for_task` shows a shared thread on each task; a FakeClock sleep, then `woke()`, shows Quiet; `shutdown()` returns within 1 s.
  - **Verify:** `tests/service.rs`.
  - **Files:** `src/lib.rs`, `src/worker.rs`, `tests/service.rs`
  - **Depends on:** T8

### Checkpoint 2

- [ ] The whole service runs against `FakeSync` and a real temp database.

## Phase 5: Finish

- [ ] **T10. Performance** (S)
  - **Description:** `#[ignore]` checks: `compute` on 2,000 tasks, 1,000 edges, 50 linked threads, 100 PRs and 200 hidden rows (95th percentile), and dismiss to updated list. Each prints the machine.
  - **Acceptance:** under 5 ms and under 20 ms; numbers recorded here.
  - **Verify:** `cargo test -p airboss-attention --release -- --ignored perf --nocapture`.
  - **Files:** `tests/perf.rs`
  - **Depends on:** T9

- [ ] **T11. Self-review** (S)
  - **Description:** re-read the crate against the spec with code-review-and-quality; fix what's found.
  - **Acceptance:** every finding fixed or written down with a reason.
  - **Verify:** the full check suite.
  - **Files:** any in `crates/attention`, these task files
  - **Depends on:** T10

### Checkpoint 3

- [ ] The spec's success criteria hold, except the 3-second end-to-end question, which needs real `sync`.
