# Tasks: launch

The plan is in [launch-plan.md](launch-plan.md), and the spec is [SPEC-launch.md](../SPEC-launch.md).

Every task meets the Definition of Done:

- tests are written first and pass
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean
- no `unwrap` or `expect` outside tests
- no test runs the real `t3` or touches `~/.t3`: tests use `FakeT3Cli`, `FakeGit`, `FakeSync`, `FakeClock`, temp dirs, or the `fake-t3` fixture

The focused test command is `cargo test -p airboss-launch`.

Sizes: XS = 1 file, S = 1–2 files, M = 3–5 files.

## Phase 1: Foundation

- [x] **T1. Crate and launch's tables** (S)
  - **Description:** `crates/launch` (`airboss-launch`) in the workspace with the spec's dependencies; `db.rs` opens launch's own connection and runs migration `launch` v1 through `migrate_module`.
  - **Acceptance:** each CHECK rejects its bad row; the one-pending index refuses a second pending row for the same task, repo and mode (null repo included) and allows one after it settles; opening twice applies once.
  - **Verify:** `tests/migrations.rs`.
  - **Files:** `Cargo.toml`, `crates/launch/Cargo.toml`, `src/lib.rs`, `src/db.rs`, `src/migrations/001_launch.sql`, `tests/migrations.rs`
  - **Depends on:** none

- [x] **T2. The process runner** (S)
  - **Description:** `process.rs`: argv-only `Command`, stdin written then closed, working directory, child environment minus `T3CODE_THREAD_ID`, `CLAUDE_CODE_SESSION_ID` and `AIRBOSS_THREAD`, and a timeout that kills the child.
  - **Acceptance:** against `tests/fixtures/fake-t3`: argv and stdin arrive byte for byte, shell metacharacters included, and nothing they name runs; exit codes; the removed variables; a missing binary is `NotFound`; the timeout kills the child and returns quickly.
  - **Verify:** `tests/process.rs`.
  - **Files:** `src/process.rs`, `tests/process.rs`, `tests/fixtures/fake-t3`
  - **Depends on:** T1

- [x] **T3. Branch names** (S)
  - **Description:** `branch.rs`: `core`'s rule, the slug, `task-<id>-<slug>`.
  - **Acceptance:** the spec's slug cases; 40-character cut at the last `-`; no-ASCII titles; names at the rule's edges (200 characters, `.lock`, leading `-` or `/`, `..`); every generated name passes the rule.
  - **Verify:** `tests/branch.rs`.
  - **Files:** `src/branch.rs`, `tests/branch.rs`
  - **Depends on:** T1

- [x] **T4. Briefs** (M)
  - **Description:** `brief.rs`: the Do it, research-only and Plan it templates and the reference block, filled by plain replacement, with the cuts and the nonce.
  - **Acceptance:** golden files for Do it with one and two repos, research only, Plan it with and without subtasks, and with a Slack link; notes with "----- END REFERENCE", the nonce itself, "ignore your instructions", newlines and shell metacharacters stay inside the block; long notes and lists are cut.
  - **Verify:** unit tests in `src/brief.rs`, `tests/briefs.rs` with `tests/golden/`.
  - **Files:** `src/brief.rs`, `tests/briefs.rs`, `tests/golden/*`
  - **Depends on:** T1

- [x] **T5. `t3` argv and answers** (S)
  - **Description:** `t3cli.rs` and `errors.rs`: argv for Do it, Plan it, research only and `project add`; titles; reading the JSON answer; mapping stderr to `ErrorCode`s and whether a row stays pending.
  - **Acceptance:** every stderr row of the spec's table; exit 127 and a missing binary; a timeout; non-JSON stdout; a kill by signal.
  - **Verify:** unit tests in `src/t3cli.rs`.
  - **Files:** `src/t3cli.rs`, `src/errors.rs`
  - **Depends on:** T2

### Checkpoint 1

- [x] The pure pieces are tested; clippy and fmt are clean.

## Phase 2: Starting threads

- [x] **T6. CoreAccess and the fakes** (M)
  - **Description:** `core_access.rs` with `fail_next`; `git.rs` (real `git` calls and the cached default branch); `fake.rs` with `FakeT3Cli` and `FakeGit`; `LaunchConfig`, `LaunchTools`, `open`.
  - **Acceptance:** `fail_next` fails exactly the next such write, once, with the chosen error; the fakes record calls and answer from scripts.
  - **Verify:** unit tests in `src/core_access.rs`; `tests/common/mod.rs` builds a `Launch` on an in-memory store.
  - **Files:** `src/core_access.rs`, `src/git.rs`, `src/fake.rs`, `src/lib.rs`, `tests/common/mod.rs`
  - **Depends on:** T5

- [x] **T7. `prepare`** (S)
  - **Description:** refusals, repos (own or open subtasks'), `needs_choice`, `choices`, branch preview, `waiting_on`, `started`.
  - **Acceptance:** each field for a task with repos, one with only subtasks' repos, one with none; proposed, done and dropped refused; no process runs.
  - **Verify:** `tests/prepare.rs`.
  - **Files:** `src/prepare.rs`, `tests/prepare.rs`
  - **Depends on:** T6

- [x] **T8. The start loop** (M)
  - **Description:** `start.rs`: checks, branch saved once, one pending row per place committed before `t3`, the answer settles it, link, move to `doing`, the report; `briefs()`.
  - **Acceptance:** exact argv for one repo, two repos, Plan it, research only; base from `origin/HEAD`, the current branch and the override; `set_branch` once, before the first start; the pending row is in the table when the fake runs; `Waiting`, `NeedsChoice`; a started repo is skipped; `-2`, `-3` on again; task text with metacharacters reaches `t3` unchanged as one argv entry or on stdin.
  - **Verify:** `tests/commands.rs`, `tests/branch.rs`, `tests/idempotency.rs`.
  - **Files:** `src/start.rs`, `src/lib.rs`, `tests/commands.rs`, `tests/idempotency.rs`
  - **Depends on:** T7

- [x] **T9. Failures and partial failures** (S)
  - **Description:** each error's row state and loop behaviour; core write failures through `fail_next`.
  - **Acceptance:** repo 2 of 3 failing still links 1 and 3 and moves the task to `doing`, and a retry starts only repo 2; a global error fails the rest; `fail_next(Link)` reports `linked: false`; `fail_next(SetStatus, InvalidTransition)` succeeds; `fail_next(SetBranch)` starts nothing; `RepoMissing`, `NoBaseBranch`, `BadBranch` write no row; T3 missing.
  - **Verify:** `tests/failures.rs`.
  - **Files:** `src/start.rs`, `tests/failures.rs`
  - **Depends on:** T8

### Checkpoint 2

- [x] Do it and Plan it work end to end against the fakes, with every failure reported.

## Phase 3: Settling and the service

- [x] **T10. Settling and the worker** (M)
  - **Description:** `settle.rs` and `worker.rs`: `find_thread` for Do it rows, `lost` after `settle_after`, linking unlinked rows, `LaunchWorker::handle` and its `Step`.
  - **Acceptance:** a timeout leaves the row pending and a later `find_thread` hit links it with no second `t3` call; a restart settles a pending row on start; `AlreadyStarting` until `again`; a pending Plan it stays pending; `lost` at 3 minutes; settle at 10 s, 30 s, 60 s and 3 min; the 15-minute cap; `Woke`; `next_wake > now` after every step; a failed link is retried; two starts at once make one pending row and one `t3` call.
  - **Verify:** `tests/worker.rs`, `tests/idempotency.rs`.
  - **Files:** `src/settle.rs`, `src/worker.rs`, `tests/worker.rs`, `tests/idempotency.rs`
  - **Depends on:** T9

- [x] **T11. The service** (S)
  - **Description:** `start(…)` with the worker thread; `Service` and `LaunchService` for `Launch`: availability, connections, recheck, woke, shutdown within 1 s, `add_to_t3`.
  - **Acceptance:** availability `Missing` without T3 (no file, exit 127) and `Ready` with its version; the connection rows; `changes()` bumps; `add_to_t3` runs `project add`; `shutdown` returns within 1 s and `start` then says `ShuttingDown`; a panic in `handle` leaves the worker running.
  - **Verify:** `tests/service.rs`.
  - **Files:** `src/lib.rs`, `src/worker.rs`, `tests/service.rs`
  - **Depends on:** T10

- [x] **T12. Self-review** (S)
  - **Description:** re-read the crate against the spec with code-review-and-quality; fix what's found; record which of the spec's success criteria tests cover (the spec itself isn't changed).
  - **Acceptance:** every finding fixed or written down with a reason.
  - **Verify:** the full check suite.
  - **Files:** any in `crates/launch`, `tasks/launch-*.md`
  - **Depends on:** T11

### Checkpoint 3

- [x] Everything but the one real Do it by hand is covered by tests.

## The spec's success criteria, and what covers them

| Criterion | Covered by |
|---|---|
| Do it on a two-repo task: two threads, exact commands, one branch, both linked as `launch`, branch saved, task to `doing` | `tests/commands.rs` `do_it_in_two_repos_on_one_branch`, `do_it_in_one_repo` |
| Quitting while `t3` runs, then starting again, links the thread and starts no second one | `tests/idempotency.rs` `quitting_while_t3_runs_then_restarting_…` |
| Plan it starts one thread whose brief asks for `import_plan` into this task | `tests/commands.rs` `plan_it_…`, `tests/briefs.rs`; the settled plan arriving as subtasks needs a real thread (by hand) |
| A waiting task gets `Waiting`; no repo gets `NeedsChoice`; research only starts in `research_dir` | `tests/failures.rs`, `tests/commands.rs` `research_only_…` |
| Without T3, `availability()` is `Missing` with the design's sentence | `tests/worker.rs`, `tests/service.rs` |
| One failing repo doesn't stop the others; retrying starts only the failed one | `tests/failures.rs` `repo_2_of_3_failing_…` |
| Every brief matches its golden file, and no note or title can end the reference block | `tests/briefs.rs`, unit tests in `src/brief.rs` |
| One real Do it on this Mac, checked by hand | Not done: it starts a real thread. Left for the integration wave. |
