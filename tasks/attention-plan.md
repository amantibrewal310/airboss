# Implementation plan: attention

Module `attention` from [CAPABILITIES.md](../CAPABILITIES.md). It builds what's in [SPEC-attention.md](../SPEC-attention.md) (draft 2, approved 26 Nov 2026), on `core` v3 and the `airboss-services` contract. The tasks and their checkpoints are in [attention-todo.md](attention-todo.md).

State: **planned 26 Nov 2026**.

## Overview

`attention` is one library crate, `airboss-attention`, that the app hosts. It turns the latest `CoreSnapshot` (from each `StoreChange`) and `SyncSnapshot` into the Needs me list. Everything it shows comes from one pure function, `compute(inputs, hidden, now, cfg)`. Around it sit a small SQLite table for dismiss and snooze, a worker state machine (`AttentionWorker::handle(msg, now)`) that tests drive with chosen times, and a thin service shell with two threads.

The work is cut into thin slices, each with its tests written first: the crate and an empty `compute`, then the kinds one family at a time, then order, hiding and deadlines, then storage, then the worker, then the service shell, then performance and review.

## Architecture decisions

- **`compute` is pure and the only place items are decided.** It takes no clock, no store and no I/O. The kinds live in `kinds.rs` (one function per kind), last activity in `quiet.rs`, order, hiding and `next_wake` in `compute.rs`. Internally it builds one `Entry { item, tasks }` per occurrence, where `tasks` lists every open task the item is about, so `for_task` can show a shared thread or PR on each of them while the list shows it once.
- **Timestamps are compared as text.** `core`'s format sorts as time. Every `since` is normalised to it (GitHub's `…:08Z` goes through `from_ts`/`to_ts`), and thresholds become cut-off strings (`to_ts(now − quiet_after)`) worked out once per call. Only the handful of deadline candidates are parsed. This keeps 2,000 tasks well inside 5 ms.
- **One bottom-up and one top-down pass over `tree` for Quiet.** `tree` is depth-first, so walking it backwards reaches children before parents (as `core` itself does for `subtree_last_event_at`).
- **Hidden state is the worker's own SQLite connection.** `start` opens a second `Store` at `Store::path()` only to run `migrate_module("attention", …)`, then opens a plain `rusqlite` connection for `attention_hidden`. The shared store is touched once, at start.
- **The worker owns everything mutable;** the service publishes an `Arc<NeedsList>` behind a mutex plus a `Changes`. `needs_me()` is an `Arc` clone.
- **The thread loop is dumb.** It waits with `recv_timeout(min(wake_at − now, 15 min))` and turns a timeout into `Tick`. All decisions (settling, deadlines, the cap) are in `handle`, so tests cover them without threads.
- **Errors never take Needs me down.** A panic in `handle` is caught and the previous list stays. A broken hidden table hides nothing.

## Decisions (where the spec leaves a choice)

1. **Review while an agent runs:** a PR isn't a review if *any* open task it's linked to has a `Running` or `NeedsMe` thread (EM: "while an agent on its task is still running"). Checking only the lowest task would list a PR an agent is still pushing to.
2. **Quiet's ancestor rule uses eligibility, not visibility.** A quiet parent hides its quiet subtasks even when the parent is dismissed or snoozed: snoozing a parent means "leave this subtree alone", and keeps `compute`'s kinds independent of the hidden rows.
3. **A `Running` or `NeedsMe` thread makes its task and every ancestor "active now"**: never quiet, and no Quiet deadline from it (the thread stopping is a sync change, which recomputes).
4. **Quiet's day count is a deadline too.** "No activity for {d} days" changes at each whole day, which is a visible change, so a visible Quiet item adds `activity + (d+1) days` to `next_wake`. Otherwise the text could lag up to 15 minutes.
5. **`next_wake` only counts what could change the visible list:** a task turning quiet (tasks under a quiet ancestor are skipped), a *visible* Ready item leaving its window, a snooze on a live key ending, and decision 4. Each is strictly later than `now`.
6. **Thresholds at the edge:** quiet when `now − activity ≥ quiet_after` (exactly 5 days is quiet); Ready shown while `now − since < ready_window` (exactly 7 days is gone); a snooze hides while `until > now`.
7. **Finish it? looks at direct subtasks.** Deeper levels have their own parents, which get their own item.
8. **Wording follows the prototype**, with singulars ("1 subtask", "1 open PR", "1 day"). A top-level proposal reads "The agent proposed a task" · "Proposed task". Missing fields fall back instead of inventing: no thread title → the task's title; no error → "Thread stopped"; no PR title → "PR #{n}"; unknown diff, checks or review → that part is left out.
9. **Ready and merged PRs:** each named blocker reads "merged" if a PR linked to it in `pr_tasks` is `merged`, else "done": "143 merged · ready to start", "143 merged and 145 done · ready to start". The headline keeps "is/are done". `pr_tasks` is documented as open tasks only, so if sync never lists a done blocker there, the plain wording shows; nothing breaks.
10. **Fallback keys and times:** a failed run without a `run_id` is keyed `failed:<thread ref>@<last activity>`; a Review's `since` falls back from `ready_since` to `updated_at`, then to the task's `last_event_at`; a Question's `since` is the request time. Never `now`, which would change the item on every recompute.
11. **`stale` for Quiet** is set when any thread or PR feeding that subtree's last activity is stale.
12. **Hiding needs a live item.** `dismiss` and `snooze` of a key not in the current candidates (visible or hidden) is `NotFound`; the table needs its kind and task. Hiding a hidden key replaces its row (a dismiss after a snooze becomes a dismiss). `restore` deletes the row; `NotFound` only when there's no row (an expired snooze restores fine). A snooze `until` that can't be read is `Internal`.
13. **Settling is a throttle, not a debounce:** the first `Store` or `Sync` message opens a 150 ms window and the recompute runs at its end, however many messages arrive. A steady stream can't postpone it, so a change reaches `changes()` within 200 ms. `Hide`, `Restore`, `Recheck` and `Woke` recompute at once (folding in anything pending). `Tick` always recomputes: it's at most 4 an hour when idle and catches a sleep.
14. **`version` moves when any item field changes** (a superset of the spec's list: a renamed task or a changed proposal title is visible too), never for `checked_at` alone. The published list still carries the newest `checked_at`.
15. **Pruning's 30-day clock is kept in memory,** seeded with the load time for every row and refreshed while a key is live. The schema has no "last live" column and adding one would change the spec's SQL. A restart can only delay pruning, never prune early (a Review hidden while its agent runs keeps its row).
16. **An in-memory `core` store** (tests only) gets an in-memory hidden table, so dismissals don't survive; `Store::path()` is `None` and there's no file to share.
17. **Shutdown wakes the `Changes` waiter by bumping `sync`'s counter.** The waiter blocks in `wait_past(seen, 15 min)`; without a bump it would take up to 15 minutes to see the stop flag. A spurious bump only makes listeners re-read an unchanged snapshot. Shutdown waits at most 1 s for both threads.
18. **Logging** is one `eprintln!` line per event, prefixed `airboss-attention:` (no log crate; "no new crates"). Each failure mode logs once until it recovers.

## Dependency graph

```
T1 crate, config, empty compute
 └─ T2 thread kinds: Question, Approval, Failed run
     └─ T3 Review
         └─ T4 Proposed, Ready, Finish it?
             └─ T5 Quiet and last activity
                 └─ T6 order, hiding, next_wake
T7 hidden table + migration          (after T1)
T8 worker state machine              (after T6, T7)
 └─ T9 service shell, threads, sleep scenario
     └─ T10 performance
         └─ T11 self-review
```

## Phases

1. **Foundation (T1):** the crate in the workspace, `AttentionConfig`, the types, `compute` returning nothing.
2. **Compute (T2–T6):** every kind, every hiding rule, the order and the deadlines, all through `compute` alone.
3. **Storage (T7):** `attention_hidden`, its migration and `HiddenStore`.
4. **Service (T8–T9):** `AttentionWorker::handle`, then `start` and `Attention` with their two threads.
5. **Finish (T10–T11):** 2,000-task performance, self-review.

## Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| `compute` over 5 ms at 2,000 tasks | Medium | Text comparison against precomputed cut-offs; one `HashMap` per index; strings built only for items shown. Measured in T10 with the 95th percentile. |
| Two `rusqlite` versions linking two SQLites | Medium | Use `core`'s exact `0.43` with `bundled`, so Cargo unifies them. |
| A deadline missed after sleep | High | Wall-clock deadlines, a 15-minute cap on any wait, `woke()`; the sleep scenario is a test with `FakeClock`. |
| A worker panic leaves dismiss callers waiting | Medium | The reply sender is dropped on unwind, so the caller gets `Internal`; the loop keeps running. |
| The waiter thread outliving shutdown | Low | Decision 17; a test checks `shutdown()` returns within 1 s. |

## Parallel work

Built in order in one thread: T2–T6 all touch `kinds.rs` and `compute.rs`. T7 could go alongside but is small.

## Open questions

None blocking. Decision 9 (merged blockers in `pr_tasks`) and decision 15 (pruning clock) are worth a look from `sync` and the EM.
