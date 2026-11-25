# Tasks: sync

The plan is in [sync-plan.md](sync-plan.md), and the spec is [SPEC-sync.md](../SPEC-sync.md).

Every task meets the Definition of Done:

- tests are written first and pass
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean
- no `unwrap` or `expect` outside tests
- no test runs the real `gh`, `t3` or `ps`, or writes to anything under `~/.t3` or `~/.claude`
- one commit per task, with no AI attribution

The focused test command is `cargo test -p airboss-sync`.

Sizes: XS = 1 file, S = 1–2 files, M = 3–5 files.

## Phase 1: Foundation

- [x] **T1. Crate, migration, own tables, `Secret`** (M)
  - **Description:** Add `crates/sync` (`airboss-sync`) to the workspace with exactly the spec's dependencies at their latest stable versions (`notify` 9.0.0 with only `macos_kqueue`). Migration `sync` v1 as in the spec, run through `migrate_module`; `SyncDb` opens its own connection on `Store::path()`. `Secret` with no `Display` and a redacting `Debug`.
  - **Acceptance:** each CHECK rejects its bad row; opening twice applies the migration once; an in-memory store is refused; `format!("{:?}", secret)` hides the token.
  - **Verify:** `tests/migrations.rs`, unit tests in `src/secret.rs`.
  - **Files:** `Cargo.toml`, `crates/sync/Cargo.toml`, `src/lib.rs`, `src/db.rs`, `src/migrations/001_sync.sql`, `src/secret.rs`, `tests/migrations.rs`
  - **Depends on:** none

## Phase 2: Sources

- [x] **T2. T3's schema as a fixture, and `T3Db`** (M)
  - **Description:** `tests/fixtures/t3_schema.sql` (the five tables' DDL and indexes, read from T3's database read-only on 26 Nov 2026) and `t3_rows.sql`. `T3Db` implements `T3Source`: read-only open, column checks, Q0 value checks, required-key checks, Q1–Q8 in one read transaction, Q7 `find_thread`, `environment_id`.
  - **Acceptance:** every query runs on the fixture; `Broken` names a missing column, an unknown run status, an unknown request kind and a missing `$.settledAt`; JSON `null` is accepted; Q7 and Q8 answer.
  - **Verify:** `tests/t3_db.rs`.
  - **Files:** `src/t3/mod.rs`, `src/t3/db.rs`, `tests/fixtures/*.sql`, `tests/t3_db.rs`
  - **Depends on:** T1

- [x] **T3. Thread state rules** (S)
  - **Description:** `threads.rs` turns a `T3Read` into `ThreadView`s: the state table (first match wins), question and approval text, the root-thread rule, child counts, last activity, untracked filtering, the failed-run note text, `thread_url` encoding.
  - **Acceptance:** each state row on the fixture; an approval without `prompt`; a child's request marks its root; untracked rows exclude linked, child and side-chat threads; `thread_url` percent-encodes.
  - **Verify:** `tests/t3_db.rs` (state section), unit tests in `src/threads.rs`.
  - **Files:** `src/threads.rs`, `tests/t3_db.rs`
  - **Depends on:** T2

- [x] **T4. Claude Code sessions** (S)
  - **Description:** read `<pid>.json` files (never `.key`), parse `ps -o pid=,lstart=` output against `procStart`, map statuses, `FakeClaude` over real temp files with faked liveness.
  - **Acceptance:** states; a reused pid is `gone`; an unknown status is `Broken`; `t3-code` sessions are never untracked.
  - **Verify:** `tests/claude.rs`.
  - **Files:** `src/claude.rs`, `src/fake.rs`, `tests/claude.rs`
  - **Depends on:** T1

### Checkpoint 1

- [x] All tests pass; clippy and fmt are clean; no test opens a real T3 or Claude file.

- [x] **T5. The T3 worker** (M)
  - **Description:** `T3Worker::handle(msg, now)`: passes on WAL and folder events with the 2 s gap, `data_version` short-circuit, `Broken`/off/busy handling, link following from `StoreSummary`, the failed-run note, T3-listed PRs into `pr_waiting` and a `GhMsg::Read`, Claude passes, watch targets and re-arming, `find_thread`, holds re-checked after thread changes, rows written only on change, `next_wake > now` always.
  - **Acceptance:** the 2 s gap, a 15-minute cap, a 6-hour sleep then `Woke`; a question reaches the snapshot; a note is written once per failed run; a T3 PR linked before our thread link is ignored.
  - **Verify:** `tests/t3_worker.rs`.
  - **Files:** `src/worker.rs`, `src/hub.rs`, `src/fake.rs`, `tests/t3_worker.rs`
  - **Depends on:** T3, T4

- [x] **T6. `gh`: finding it, accounts, the query, parsing** (S)
  - **Description:** `gh.rs`: binary search order, `auth status --json hosts` parsing (active first), the batched GraphQL document with variables, response parsing (`NOT_FOUND` at `pN` versus `pN.pullRequest`, exit 4, secondary limits, budget), the child environment. `FakeGh` answers from a small GitHub "world" by account and refs, and records calls.
  - **Acceptance:** each mapping row; owner, repo and number only in variables; the token only in the child env; `FakeGh` sees which account each call used.
  - **Verify:** `tests/gh.rs`.
  - **Files:** `src/gh.rs`, `src/fake.rs`, `tests/gh.rs`
  - **Depends on:** T1

### Checkpoint 2

- [x] All tests pass; clippy and fmt are clean.

## Phase 3: GitHub worker

- [x] **T7. The GitHub worker, PR mapping and finishing** (M)
  - **Description:** `GhWorker::handle(msg, now)`: the 3 s first pass and 5-minute timer, on-demand limits, per-account health, backoff and rate limits, account fallback on `NOT_FOUND`, viewer PRs, `set_pr_state` on differences, the finish check and its three triggers, holds, reopening, T3-listed PRs linked with GitHub's state, rows written last.
  - **Acceptance:** every `tests/prs.rs` case in the spec.
  - **Verify:** `tests/prs.rs`.
  - **Files:** `src/worker.rs`, `src/prs.rs`, `src/finish.rs`, `tests/prs.rs`
  - **Depends on:** T5, T6

## Phase 4: Service

- [x] **T8. `Sync`: start, threads, `SyncService`** (M)
  - **Description:** `start(cfg, store, tools)`, `run_once`, the thread loops (`recv_timeout` capped at 15 minutes, panics caught and turned into `Broken`), `connections()`, `store_changed`, `refresh`, `lookup_pr`, `find_thread` (2 s), `thread_url`, `woke`, `recheck`, `shutdown` (at most 2 s), the snapshot loaded from our tables at start and marked stale.
  - **Acceptance:** every trait method's row in the spec; `lookup_pr`'s error codes; a panic in `handle` marks the source broken and the thread keeps running.
  - **Verify:** `tests/service.rs`.
  - **Files:** `src/lib.rs`, `src/hub.rs`, `tests/service.rs`
  - **Depends on:** T7

- [x] **T9. The real kqueue watcher** (S)
  - **Description:** `KqueueWatch` implements `WalWatch` with `notify::KqueueWatcher`: T3's folder (non-recursive), the `-wal` file, the Claude folder and its `<pid>.json` files; re-arms a dropped watch when called again.
  - **Acceptance:** a child process holding a WAL database open writes 5 times, and each write is reported within 1 s; it closes (the WAL is deleted), reopens and writes, and the watcher re-arms and reports that too.
  - **Verify:** `tests/watch.rs`.
  - **Files:** `src/watch.rs`, `tests/watch.rs`
  - **Depends on:** T8

- [x] **T10. Real tools, the real-database test, `t3_check`, perf** (M)
  - **Description:** `Tools::real` (process `gh` runner with a 60 s timeout, `ps` liveness, `T3Db`, `KqueueWatch`, `SystemClock`). `tests/real_t3.rs` (`#[ignore]`): Q0–Q8 and every check against `~/.t3/userdata/statev2.sqlite` opened with `mode=ro`. `examples/t3_check.rs`. `#[ignore]` perf test: a pass on 50 linked of 2,000 threads.
  - **Acceptance:** the real-database test passes read-only; the perf pass is under 5 ms at the 95th percentile.
  - **Verify:** `cargo test -p airboss-sync --release -- --ignored real_t3 t3_pass_perf`.
  - **Files:** `src/gh.rs`, `src/claude.rs`, `src/lib.rs`, `tests/real_t3.rs`, `tests/perf.rs`, `examples/t3_check.rs`
  - **Depends on:** T9

### Checkpoint 3

- [x] All tests pass; the real-database test passes; clippy and fmt are clean.

## Phase 5: Finish

- [x] **T11. Idle CPU and wake frequency** (S)
  - **Description:** `examples/idle.rs` (an example, not a test, so its CPU comes from `time -l` and its wake-ups from `top`, and no test runs `ps`) runs `Sync` with the real watcher on an idle fixture T3 database and an idle `FakeGh`, at the real 5-minute timer, and reports wake-ups per hour and CPU averaged over the run. Results recorded below.
  - **Acceptance:** 12 timer wake-ups an hour; CPU under 0.05%.
  - **Verify:** `cargo build --release -p airboss-sync --example idle --features fakes && /usr/bin/time -l target/release/examples/idle 3600`.
  - **Files:** `examples/idle.rs`, `tasks/sync-todo.md`
  - **Depends on:** T10

- [x] **T12. Self-review and fixes** (S)
  - **Description:** review the crate with agent-skills code-review-and-quality (correctness, readability, architecture, security, performance) and fix what it finds.
  - **Acceptance:** every finding fixed or written down with why it stays.
  - **Verify:** the full check suite.
  - **Depends on:** T11

### Checkpoint 4

- [x] Every spec success criterion that tests can check is checked; measurements recorded.

## Results

Measured 26 Nov 2026 on this Mac (macOS 27.2, Rust 1.100.0, release builds).

| What | Target | Measured |
|---|---|---|
| One T3 pass, 50 linked of 2,020 threads, 200 untracked (fixture) | < 5 ms p95 | 0.49 ms median, 0.53 ms p95 |
| The same queries on the real 3 GB database, 50 recent threads, Q6 and Q8 included | | 2.3 ms median, 10.4 ms max (20 passes) |
| Open and checks (Q0) on the real database | | 36 ms, once an hour |
| One GitHub pass, 50 PRs, one account (FakeGh's JSON included) | 1 call, < 150 ms | 1 call, 0.84 ms median, 7.6 ms max |
| Idle CPU (examples/idle, 3,600 s, real kqueue and T3 queries, FakeGh) | < 0.05% | 0.04 s of CPU (0.01 user + 0.03 sys) in 3,600 s = 0.0011% |
| Idle wake-ups | 12 an hour (the GitHub timer) | 12 GitHub passes, 24 worker-thread wake-ups in the hour (each timer pass also wakes the T3 worker with the forwarded tick); `top` IDLEW 2 over the hour |
| Memory | < 10 MB added | 7.8 MB maximum RSS for the whole idle process, fixture included |

The real-database test (`tests/real_t3.rs`, `mode=ro`) passed every column, value and key check and Q0-Q8: 28 projects, 50 threads (43 settled, 7 idle), 1 failed run with its message, Q7 found its thread. A read-only WAL connection still updates its read mark in `statev2.sqlite-shm`; that's SQLite's own mechanism and the reason `immutable=1` (which would skip the WAL) isn't used.

The `gh` process itself (one every 5 minutes) isn't in the idle CPU figure: no test may run the real `gh`.

Review (agent-skills code-review-and-quality, plus a fresh reviewer): fixed a lost finish when both workers touched a hold, the GitHub row while `gh` itself fails, no signed-in account counted as signed in, missing sources leaving threads running, the "Run failed" fallback cached, a starved T3 tick, links failing to load at start, health rows not saving backoff, and an untested first-read trigger (found by a mutation that stayed green).
