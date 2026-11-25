# Implementation plan: launch

Module `launch` from [CAPABILITIES.md](../CAPABILITIES.md). It builds what's in [SPEC-launch.md](../SPEC-launch.md) (draft 2, approved 26 Nov 2026) on `core` v3 and the `crates/services` contract. The tasks and their checkpoints are in [launch-todo.md](launch-todo.md).

State: **planned 26 Nov 2026**.

## Overview

`launch` is one library crate, `airboss-launch`, behind Do it and Plan it. It is the only module that runs `t3`. The signature action has to be reliable, so the plan is built around three guarantees:

1. **No duplicate threads.** A `pending` row is committed to `launch_threads` before `t3` runs. A partial unique index allows one pending row per task, repo and mode. A pending row is settled by `t3`'s answer, or later by `sync.find_thread(project, branch)`, and never by starting again unless I press "Start again".
2. **A clear partial-failure report.** Each place (repo, or the one folder) succeeds or fails on its own, with one `ServiceError` sentence. Errors that mean T3 can't start anything stop the loop and fail the rest with the same error.
3. **Safe task text.** Task text only ever reaches `t3` as argv entries (no shell anywhere) or on stdin. Everything people wrote sits in one nonce-fenced reference block that the brief says is reference material, not instructions.

The work is cut into thin slices, each test-first: the table, the process runner, names, briefs, the `t3` command and answer mapping, core access and the fakes, `prepare`, the start loop, failures, settling and the worker, then the service wiring and a self-review.

## Architecture decisions

- **`Launch` wraps an `Arc<Inner>`** holding the config, `CoreAccess`, launch's own SQLite connection (behind a mutex), the tools (each behind its own mutex), `sync`, the cached availability and `Changes`. `LaunchWorker` holds the same `Arc<Inner>`, so the worker and `start` share one state with no copies.
- **Locks are taken one at a time and never held across a process.** The launch-table lock, the core store lock and the git lock are each held for one short call. The `t3` runner's own mutex is the only lock held while `t3` runs, so two `start`s run `t3` one after another.
- **`open(…)` and `start(…)`.** `open` builds `Launch` and its `LaunchWorker` without a thread, so tests drive the worker with chosen times. `start` is `open` plus a worker thread; it's what the app calls.
- **One process runner** (`src/process.rs`) for `t3` and `git`: `std::process::Command` with argv only, stdin written then closed, stdout and stderr read on their own threads, a deadline enforced by polling `try_wait` and killing the child, and a child environment built by a pure function (so the removed variables are testable without `set_var`).
- **Pure pieces are plain functions:** branch slug and rules (`branch.rs`), briefs (`brief.rs`, with the nonce injectable inside the crate), argv and answer mapping (`t3cli.rs`), and settle-time maths (`worker.rs`).
- **Fakes in `src/fake.rs`** behind the `fakes` feature: `FakeT3Cli` (a recorded-behaviour runner: scripted replies, every call recorded with argv and stdin, and a hook that runs at call time) and `FakeGit`. `FakeSync` and `FakeClock` come from `airboss-services`. The crate's own tests depend on it with the feature on.

## Decisions

Where the spec is silent or ambiguous, the reading most consistent with `core`'s rules and the EM decisions:

1. **Config additions.** `LaunchConfig` also has `home` (the working directory for `t3` and `git`, and what `LaunchTools::real` needs) and `db_path: Option<PathBuf>` (launch's own tables; `None` means `core`'s file from `Store::path()`). With `core` in memory and no `db_path`, `start` fails with `Storage`. Tests use a temp file, as the spec's Testing section says.
2. **"Those rows become lost" means pending rows.** With `again`, only `pending` rows for that task, repo and mode become `lost`. `started` rows stay `started`: the schema's CHECKs tie `started` to a thread id, and the thread does exist.
3. **The again-branch** is used only when that repo already has a `started`, `pending` or `lost` Do it row for the task, so a thread may hold the task's branch. Otherwise "Start again" uses the task's branch, as a first start would.
4. **Plan it with no repo and `TaskRepos`** starts in `research_dir`, and its brief says "No repo is linked yet". `NeedsChoice` is for Do it only, as `Prepared.needs_choice` says.
5. **A task branch git rejects** is `Err(BadBranch)` for the whole call, checked before `set_branch`, because nothing can begin. An again-branch that fails is `BadBranch` for its place only.
6. **Paths.** A repo path or a `NoRepo` folder must be absolute and an existing directory, otherwise `RepoMissing`. An absolute path can't be read as a `t3` flag.
7. **The base** (override or from `git`) must pass `core`'s branch character rule, otherwise `NoBaseBranch`; it's passed to `t3` and shown in the brief.
8. **Titles.** Control characters (newlines included) become spaces. With several repos the cut to 80 characters shortens the task title and keeps ` · <repo>`, because the brief tells siblings they're titled by repo.
9. **The reference block** replaces any copy of the brief's nonce in the content with `[nonce removed]` and drops NUL characters. Repo names and paths in the instruction part have control characters replaced by spaces.
10. **`import_plan` in the Plan it brief uses `repos`** (a list with one repo name), the field `SPEC-mcp` and `crates/mcp` define. The spec's template says `repo`, which `import_plan` would refuse (`deny_unknown_fields`). This is the only change to what the briefs tell agents; flagged in the report.
11. **The Parent line** of the reference block appears only when the parent has notes; "Under" already names it.
12. **Settling a row to `started`** outside `start` (restart, timeout) also links it and moves the task to `doing` as `launch`, as `start` would have. `InvalidTransition` there is fine too.
13. **A `t3` killed by a signal** that wasn't our timeout is `BadAnswer`: the row stays pending, because a thread may exist.
14. **A no-repo place that already started** is skipped without an entry in `skipped` (which holds `RepoId`s); `prepare` lists it in `started`.
15. **`briefs()`** leaves out places that would be skipped and returns the first place error, so a preview never shows a brief that wouldn't be sent.
16. **`add_to_t3`** runs `<t3> project add --title <name> <path>`: flags before the path, as `t3 project add --help` shows its usage. The repo path must be absolute.
17. **Availability:** `Ready { version }` holds the version without the `v` (`"0.1.12"`). A `T3Missing` during `start` queues an availability check.
18. **Unlinked started rows** are retried by the worker 60 s later, and on every settle pass.
19. **A deleted task** (`NotFound` from `core` on link or status) has its rows removed.
20. **Logging** is one `eprintln!` line prefixed `airboss-launch:`. There is no logging crate, and the spec allows no new crate.
21. **After `shutdown`,** `start` returns `ShuttingDown`.
22. **`src/process.rs`** holds the runner shared by `t3` and `git`; the spec's tree puts running `t3` in `t3cli.rs`, which keeps the argv and answer mapping.

## Dependency graph

```
T1 crate + table
 ├─ T2 process runner (fake-t3 fixture)
 ├─ T3 branch names
 ├─ T4 briefs
 └─ T5 t3 argv + answer/error mapping
     └─ T6 CoreAccess + FakeT3Cli + FakeGit
         └─ T7 prepare
             └─ T8 start loop
                 ├─ T9 failures and partial failures
                 └─ T10 settling + worker
                     └─ T11 service wiring (start(), availability, add_to_t3, shutdown)
                         └─ T12 self-review
```

## Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| A test runs the real `t3 thread start` and makes a real thread | High | Tests only use `FakeT3Cli` or the `fake-t3` fixture; `LaunchTools::real` is never called from a test; the config in tests points `t3_path` at a temp file. |
| A timeout or restart starts a second thread | High | Pending row committed first; one-pending unique index; settled only by `t3`'s answer, `find_thread`, or `again`. Tests count `t3` calls. |
| Task text read as shell or as instructions | High | No shell; argv and stdin only; nonce-fenced reference block; tests with metacharacters, newlines and injection text. |
| A grandchild holds `t3`'s pipes open after a kill | Medium | The runner doesn't wait for the readers after a timeout, and waits a bounded time otherwise. |
| `core` and launch's own connection deadlock | Medium | Locks taken one at a time, never across a process; `busy_timeout` on launch's connection. |
| T3's real output differs from its source | Medium | Recorded under "Not verified yet"; the one manual Do it in the spec's success criteria checks it. |

## Parallel work

Built in order in one thread: T7–T11 share `Inner`.

## Open questions

None. Everything ambiguous is under Decisions.
