# Implementation plan: sync

Module `sync` from [CAPABILITIES.md](../CAPABILITIES.md). It builds what's in [SPEC-sync.md](../SPEC-sync.md) (draft 2, approved 26 Nov 2026) on `core` v3 and the shared contract in `crates/services`. The tasks and their checkpoints are in [sync-todo.md](sync-todo.md).

State: **planned 26 Nov 2026.**

## Overview

`sync` is one library crate, `airboss-sync`, that runs inside the app process as two worker threads (T3 and Claude; GitHub) plus `notify`'s kqueue thread. It reads GitHub PRs through `gh api graphql`, T3 threads from T3's SQLite database (read-only, on every WAL write) and Claude Code sessions from their files. It keeps what it sees in its own tables, publishes one `Arc<SyncSnapshot>`, and changes tasks in `core` only as `sync:gh` (PR state, finishing) and `sync:t3` (PRs found on threads, failed-run notes).

Each worker is a plain state machine, `handle(msg, now) -> Step`, so every timing rule is tested with chosen `SystemTime`s and no sleeping. Every outside thing sits behind a trait (`GhRunner`, `T3Source`, `ClaudeSource`, `WalWatch`, `Clock`), and tests use fakes. The T3 fake is T3's real schema in a temp file, so the real SQL runs in every test.

## Architecture decisions

- **Layers, bottom up.** `db.rs` (own tables) → `t3/` (the only code that knows T3's layout) and `claude.rs` and `gh.rs` (outside sources, each returning typed rows) → `threads.rs` and `prs.rs` / `finish.rs` (pure rules over those rows) → `worker.rs` (the two state machines) → `lib.rs` (`Sync`, threads, `SyncService`).
- **A `Hub` holds what the workers publish.** The T3 worker owns the thread half (threads, `thread_tasks`, projects, T3 and Claude health, environment id); the GitHub worker owns the PR half (PRs, `pr_tasks`, per-account health). Publishing a half rebuilds the `Arc<SyncSnapshot>` (stale flags, `checked_at`) and bumps `Changes`. `snapshot()` and `connections()` read the hub only.
- **Two connections to our tables,** one per worker, on `Store::path()`, after `migrate_module("sync", …)` on the shared store. `core` is only touched through the shared `Store`, one short lock per call, never held across a `gh` run or a T3 read.
- **Write order makes every pass repeatable.** Core writes (`set_pr_state`, `link`, `set_status`, holds) happen before `observed_prs` is updated, so if the app stops in between, the next pass still sees the old state and fires the same trigger.
- **The `gh` client (runner, accounts, tokens) sits behind one mutex** shared by the GitHub worker and `lookup_pr`, so only one `gh` process runs at a time.
- **Fakes live in `src/fake.rs` behind a `fakes` feature** that the crate's own tests turn on through a dev-dependency on itself, as `airboss-services` does.

## Decisions (where the spec left a gap)

1. **Watch messages.** `WalWatch::watch` takes the T3 worker's own `Sender<T3Msg>`, so there's no separate `WatchEvent` type to forward, and `T3Msg` gains `T3Dir` for a change in T3's folder (which re-arms the WAL watch and checks `data_version`). Without this the worker would need a second channel and a forwarding thread, since `std::sync::mpsc` can't wait on two.
2. **The GitHub timer drives the T3 worker's periodic work.** On each timer pass the GitHub worker sends `T3Msg::Tick`, and the T3 worker then checks Claude liveness, retries T3 if it's off, and runs the hourly checks when due ("checked again each GitHub tick" in the spec). The T3 worker's own deadlines are only the end of the 2 s gap, the 10 s grace for a request with no item, and the 15-minute cap, which the forwarded ticks keep resetting.
3. **Link events name their kind.** `core` writes `linked` and `unlinked` bodies as `<kind> <ref>` and `link_state` for PRs only. A worker re-reads `links_of_kind` only for its own kinds that an event names: T3 for `t3_thread` and `claude_session`, GitHub for `pr`. Any change in the open-task set re-filters the cached links without a read.
4. **`pr_waiting` holds objects,** `{"ref","url","linked_at"}`, so the GitHub worker can link a PR with its URL and apply the `linkedAt` rule without reading T3. `pr_seen` is a plain array of refs. Both are only changed by single atomic `UPDATE`s, so the two workers never overwrite each other's edits.
5. **PRs found on threads are linked to open tasks only.** `core` rule 7 would otherwise let `sync:t3` reopen a done task through a PR that only T3 knew about.
6. **A failed `gh` call keeps what's known.** A PR with an earlier GitHub read keeps its state and shows `stale`; one never read gets an `error = 'failed'` row (state null), so its problem shows.
7. **`lookup_pr` writes nothing.** It tries the active account, then the others, stopping at the first that can see the repo (it doesn't read our tables for the account that last read the PR, so it needs no connection of its own).
8. **Untracked Claude sessions show their live state** (`running` or `idle`). T3's untracked threads are `settled` or `idle`, as the spec says, because their runs aren't read.
9. **`last_activity_at` falls back to the thread's `updated_at`** when it has no runs, and for untracked T3 threads.
10. **Claude files.** A file that isn't valid JSON (caught mid-write) is skipped for that pass. A missing required field (`pid`, `sessionId`, `status`, `procStart`) or an unknown `status` marks Claude sync `Broken`.
11. **Approval text** without a `prompt` is the kind's phrase, capitalised: "Run a command", "Read a file", "Change files", "Grant a permission", "Use a tool", "Answer an MCP server", "Sign in again".
12. **No environment id:** the "T3 Code" row is not ok, with state "Can't open threads" and a fix saying why, and `thread_url` is `None`.
13. **A linked thread with no observed row** (T3 off since before it was linked) doesn't hold a finish.
14. **The 10 s grace for a request with no item** applies to requests on linked threads. Kind and status values are still checked on every pending request.
15. **What was seen before shows at once.** `start` publishes the snapshot loaded from our tables before the threads start, with every source marked stale until its first pass.
16. **The store must be a file.** `start` refuses an in-memory `Store` with `ErrorCode::Storage`, since `sync` opens its own connection on `path()`.
17. **Wake-ups are counted per thread.** Each worker loop counts the times it wakes (`Sync::wakeups()`); the idle measurement reports those and the process's CPU time.
18. **A request on a child thread** marks its root thread `needs_me` (the spec's root rule), using the child's `$.lineage.rootThreadId`. A root thread's `rootThreadId` is its own id, as on this Mac.
19. **`pr_tasks` also lists tasks finished in the last 7 days** (attention's request on 26 Nov 2026, so a Ready item can say "143 merged"). "Finished" is read from `core`'s tree as a `done` task whose newest event is at most 7 days old, the same window as "became ready". Everything `sync` decides (what to read, the finish check) still uses open tasks only.
20. **The finish check uses this pass's GitHub states** before `observed_prs` is written (it's written last, decision above), so it doesn't wait a pass for its own row.
21. **Times in connection rows say "UTC"** ("Live state broken since 14:02 UTC"): `sync` has no time-zone data and no dependency for it.
22. **After its own `core` writes, the GitHub worker reads the PR links again** at once, instead of waiting for the app to hand the change back, so the next comparison isn't against a stale `pr_state`.

## Dependency graph

```
T1 crate, migration, own tables, Secret
 ├─ T2 T3 fixture + T3Db (checks, Q0–Q8)
 │   └─ T3 thread state rules
 │       └─ T5 T3Worker ──────────────┐
 ├─ T4 Claude sessions ───────────────┤
 └─ T6 gh: accounts, query, parsing   │
     └─ T7 GhWorker, PR mapping, finish│
         └─ T8 Sync service ◄─────────┘
             ├─ T9 real kqueue watcher (two-process test)
             └─ T10 real tools, real-database test, t3_check, perf
                 └─ T11 idle CPU and wake measurement
                     └─ T12 self-review fixes
```

## Phases

1. **Foundation (T1):** the crate, the migration and its CHECKs, `Secret`.
2. **Sources (T2–T4, T6):** T3's database, the thread rules, Claude sessions, `gh`.
3. **Workers (T5, T7):** both state machines, every timing rule, every PR rule.
4. **Service (T8–T10):** `Sync`, the real watcher, the real tools.
5. **Finish (T11–T12):** measurements, review.

## Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| A read-only connection to T3's live 3 GB WAL database holds back checkpoints or writes | High | One short read transaction per pass; `query_only`; `mode=ro`; never `immutable`. The real-database test runs every query this way. |
| `notify` 9.0.0's kqueue backend is new | Medium | Its source was read (single-path watches for non-recursive folders, `poll(None)` so no idle wakes, watches dropped on delete/rename). The two-process test exercises it for real. |
| The two workers race on `observed_threads.pr_waiting/pr_seen` | Medium | Only atomic, conditional `UPDATE`s touch those columns. |
| A crash between core writes leaves a task unfinished forever | High | `observed_prs` is written last, so the trigger fires again (tested). |
| A token leaks into argv, logs, tables or `Debug` | High | `Secret` has no `Display`; only `gh.rs` calls `expose()`, to set a child's env. A test checks argv, tables and `Debug` for the token text. |
| Tests touching the real T3 or Claude files, or running `gh`/`ps` | High | Tests build `Tools` from fakes only; the T3 fake is a temp file; Claude files are temp files; liveness is faked. Only the `#[ignore]` real-database test opens T3's file, with `mode=ro`. |
| Idle cost above target | Medium | No polling anywhere; the only timer is GitHub's 5 minutes. Measured in T11. |

## Parallel work

Built in order in one thread. Other engineers own `launch`, `attention`, the app backend and the app UI; `sync` touches only `crates/sync/**`, these two files and one line of the root `Cargo.toml`.
