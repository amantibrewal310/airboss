# Program: Airboss v1

Who drives it: Claude, acting as engineering manager, with a team of up to five agents. The user delegated the whole build on 25 Nov 2026: "drive it end to end, testing and bug fixes included; high quality, polished, great UX". The user comes back in only for decisions that can't be made without them.

Inputs: [docs/idea.md](../docs/idea.md), [CAPABILITIES.md](../CAPABILITIES.md), [PRODUCT.md](../PRODUCT.md), [DESIGN.md](../DESIGN.md) (with the prototype in `docs/design/`), and one spec per module.

## Process (agent-skills lifecycle, per module)

1. Spec, saved as `SPEC-<module>.md`.
2. Fresh-eyes review of the spec.
3. Task plan.
4. Build, tests first.
5. Code review, then simplification.
6. Merge.

Then, for the whole product:

7. End-to-end QA as a real user.
8. Polish passes until the bar in "Done means" is met.

## Waves

| Wave | Work | State |
|---|---|---|
| 0 | `core` v1 | Done (PR #1) |
| 0 | App design | Done (merged e63a45b) |
| 1 | `core` v2: what the design needs (see SPEC-core "v2") | Done (262 tests) |
| 1 | Specs: `app`, `sync`, `launch`, `attention` | Drafted; under fresh review |
| 1→2 | `mcp` | Done (merged; 107 tests; start-up 2.3 ms, 5.4 MB binary) |
| 2 | Build `app`, `sync`, `launch`, `attention` in parallel | |
| 3 | Integration: one app process hosts sync, launch and attention; the CLI and `airboss mcp` share the database | |
| 3 | QA: real-user flows, performance targets, light and dark, keyboard-only | |
| 4 | Polish and bug-fix iterations; README and install instructions | |

## Decisions made as EM (the user may revisit them)

- **Several repos per task**, per the design (agreed with the user in the design thread). This replaces the single `tasks.repo_id`. Do it starts one thread per linked repo, on one branch name shared by all of them.
- **Migrations:** `001_core.sql` may still be edited, because no real database exists yet. Once `mcp` ships, migrations are append-only.
- **`mcp` connects over stdio, one process per thread** (SPEC-mcp). The app watches the database file to pick up writes from other processes.
- **Sync, launch and attention run inside the app process.** The CLI gets `--once` commands where that's useful.

## EM answers to the spec leads' open questions (25 Nov 2026)

- **Launch at login:** offered on the first-run screen, ticked by default. It's also in Settings.
- **Appearance:** follows the system by default. Settings has Dark / Light / System. Dark is the design's first-class mode.
- **`gh` accounts:** sync reads PRs with every account logged in to `gh` (`gh auth token --user <login>`). Tokens are held in memory only and never written to disk.
- **PRs that T3 lists on a linked thread** are linked to the task as `sync:t3`, so a PR the agent forgot to link still counts.
- **History notes from sync:** only a failed run writes a history note. Questions and approvals are live state shown in Needs me, not history.
- **"Add to T3"** is offered when a linked repo isn't a T3 project yet.
- **Thresholds:** "became ready" shows for 7 days. A PR isn't listed as needing review while an agent on its task is still running. Quiet means 5 days.
- **Small core additions** for the services: `Store::migrate_module(module, &[Migration])` (making the runner public), and `links_of_kind(kind)`.

## EM decisions on the spec reviews (26 Nov 2026)

- **Service contract:** one std-only `crates/services` crate holds the shared types and traits, using the reconciled sketch from the services review: `Service`, `SyncService`, `LaunchService`, `AttentionService`, `StoreChange`, `LaunchReport`, a serializable `ServiceError` and a `Changes` signal. No tokio in services. Every spec builds to it.
- **Seeing writes from other processes:** FSEvents doesn't report writes to a WAL file that stays open. So the app and sync watch the `-wal` file with kqueue (re-armed when it's recreated), and check core's new `data_version()` on focus and on wake as a safety net.
- **Core v3 additions** (small, built first):
  - `snapshot() -> CoreSnapshot { tree, edges, ready, latest_event_id }` in one consistent read, so services and the app never rebuild the tree several times per change
  - `dependency_edges()`, `latest_event_id()`, `data_version()`, `links_of_kind(kind)`
  - a public `Migration` and `migrate_module(module, &[Migration])`, plus `path()`. Service crates open their own connection for **their own** tables only
  - PR refs stored in one canonical form, `owner/repo#n` in lowercase, with URLs accepted
  - branch names limited to `[A-Za-z0-9._/-]`
- **App updates** use a patch sequence number of their own, separate from core event ids, and the snapshot is rebuilt on any service change too.
- **Open thread** opens the T3 deep link `t3code://threads/<environment>/<thread>`. Sync supplies the environment id.
- **PRs found on T3 threads** are linked only if T3 linked them at or after the moment the thread was linked to the task, and with GitHub's real state. The finish check uses only state read from GitHub.
- **A task with several repos finishes** only when its PRs have merged **and** none of its linked threads is running or waiting on me.
- **Launch writes a pending record before running `t3`**, so a retry or restart can't start a duplicate thread. `InvalidTransition` on the final move to `doing` counts as success.
- **`gh`:** health, backoff and rate limits are tracked per account. Tokens are wrapped in a `Secret` type that is never printed.
- **T3 database:** an unknown value or a missing JSON path marks T3 sync `Broken`. It never falls back silently. An `#[ignore]` test runs the queries against the real database, read-only.
- **Time:** deadlines come from the wall clock, no single wait is longer than 15 minutes, and every service has `woke()`.
- **App UX additions:**
  - focus rules for every command
  - quick-add resets and focuses when it's hidden, so it's ready the moment it opens again
  - plugin defaults fixed: `window-state` restores size and position only; autostart passes `--hidden`; the hotkey acts on Pressed only
  - a "Starting…" state and result toasts for Do it
  - Undo (⌘Z and on toasts) for dismiss, snooze, reject, status changes and moves
  - reordering with ⌥⌘↑/↓
  - a first-run "Add repos" step that lists T3's projects
  - ↓ moves from the quick-add title to its notes
  - screenshot baselines for all 13 screens in both themes
  - ids in `ts-rs` types as `number`
  - services are notified after the store lock is released, and service threads catch panics instead of aborting

## Done means

- Every module's success criteria are checked off, with tests.
- Every performance target in CAPABILITIES is measured on this Mac.
- The app is used end to end, keyboard-only, in dark and light: capture a task, plan it, Do it, an agent reports, a PR merges, Needs me clears.
- `cargo clippy -D warnings`, `cargo fmt --check`, every Rust test and every UI test are clean.
- No AI attribution anywhere.
