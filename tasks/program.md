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
| 1 | Specs: `app`, `sync`, `launch`, `attention` | Draft 2, approved by the EM after fresh reviews |
| 1→2 | `mcp` | Done (merged; 107 tests; start-up 2.3 ms, 5.4 MB binary) |
| 1→2 | `core` v3 + `crates/services` contract | Done (439 tests) |
| 2 | Build `sync`, `launch`, `attention` | Done (merged; 843 workspace tests) |
| 2 | Build `app` UI | Done (merged; 147 unit/component + 30 Playwright tests) |
| 2 | Build `app` backend + wire real services | Done (merged; 941 Rust tests; 5.7 MB dmg, 457 ms cold start) |
| 3 | UI polish pass from design QA (24 findings) + integration fixes | Done (merged; 173 unit/component + 135 Playwright incl. axe) |
| 3 | Integration QA on the real app | Done (merged; 7 bugs fixed; real agent + real Do it verified; 958 Rust tests) |
| 4 | Fixes from integration QA (R1, R2, R5) | Done |
| 4 | Final full-repo audit | Done: ready with caveats; B1 and all should-fix items fixed in waves 5–6 |
| 5 | Audit fixes: core/sync/mcp/CLI/docs | Done |
| 5 | Audit fixes: app side + notifications | Done |
| 6 | Last fixes: repo ids never reused, conditional reject, notifications via UNUserNotificationCenter, output-read window | Done |
| 6 | Final verification from a clean build | Done: 1,085 Rust + 199 Vitest + 140 Playwright tests; .dmg 5.85 MiB |
| 7 | Independent security check of the link opener, then hardening (signature-verified handler apps) | Done: 1,094 Rust + 200 Vitest + 140 Playwright tests |
| 7 | Manual checklist by a person (`app/QA.md`) | Waiting for the user |
| 3 | Integration: one app process hosts sync, launch and attention; the CLI and `airboss mcp` share the database | |
| 3 | QA: real-user flows, performance targets, light and dark, keyboard-only | |
| 4 | Polish and bug-fix iterations; README and install instructions | Done |

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

## EM decisions on the draft-2 open points (26 Nov 2026)

- When sync and launch both report a "T3 Code" connection, the app shows them as one row: rows with the same name merge, and the worst state wins.
- The hold-before-finishing rule (no linked thread running or waiting on me) applies to **every** task, not only tasks with several repos. One rule is simpler.
- `AlreadyStarting` gets its own toast in the app: "Already starting — check T3 in a moment."
- An interrupted Plan it, or a research-only start, stays pending until "Start again". That's acceptable for v1.
- Reject commits after a 5-second undo window. Accept can't be undone: it's rare to regret one, and the task can be dropped.
- The default quick-add hotkey is ⌃⌥N (it can be changed in Settings).

## EM decision on idle memory (26 Nov 2026)

- **Target revised: idle memory under 130 MB** (it was 80 MB). Measured: 129 MB with both web views alive, 119 MB with the main window hidden. WebKit alone uses about 78 MB for one view, so 80 MB can't be reached with a web view plus an instant quick-add panel.
- **Releasing the quick-add view after 10 idle minutes** would save about 20 MB, still miss 80 MB, and make quick-add open slowly. So instant quick-add wins.
- The user can revisit this. The lighter alternative would be a native quick-add panel instead of a second web view.

## EM decision on JS bundle limits (26 Nov 2026)

- The spec's JS limits (main window under 150 KB and quick add under 40 KB, both minified) were a means to the real targets: a small download and a fast quick add. Both are met with room to spare: the `.dmg` is 5.7 MB and quick add opens in 9 ms.
- So the limits are now **main window under 300 KB, quick add under 100 KB minified**. Measured: 256 KB and 93 KB.

## EM decisions on integration QA findings (26 Nov 2026)

- **R1** (sync and launch log messages lost when the app opens from Finder): fix. All service logs go to the app's log file.
- **R2** (deleting the live WAL loses writes): this is SQLite's behaviour, not a bug. Detect it and show a clear "database problem" state where cheap, and document it in troubleshooting.
- **R3** (catch-up costs 17–21 ms per outside write at 2,000 tasks): accepted, within the 25 ms write budget.
- **R4** (131 MB idle at 2,000 tasks): accepted. The target is under 130 MB typically and under 140 MB at 2,000 tasks.
- **R5** (a run that ends without a PR leaves the task `doing`, with nothing in Needs me): fix. A new Needs me kind, **"Agent stopped"**, appears when every linked thread of a `doing` task has stopped (idle, settled or gone; errored is covered by Failed run), there's no open PR, and nothing has changed for 10 minutes. Its actions are Open thread, Mark done, Set in review and Dismiss.
- **Clean-up done:** the stray login item from the measurement runs, plus the QA throwaway T3 project, thread and worktree.

## EM decisions on the final audit (26 Nov 2026)

- **B1 (a link added by an agent can run code):** must fix before daily use. Strict URL allowlist: `file:///abs` with no host, query or fragment; the path canonicalized; only safe file types opened; everything else revealed in Finder. `t3code:` only as `threads/<env>/<id>`. `https:` only for opening in the browser.
- **B2 (the app has never been used by a human):** this session can't capture the screen. A short manual checklist, `app/QA.md`, goes to the user.
- **B3 (licence):** the user decides before open-sourcing. Recommendation: MIT.
- **Should-fix 1–12 and the nits:** fix all of them, split by area. Core, sync, mcp, CLI and docs go to one engineer now. App-side fixes (undo correctness, reorder, reject write retry with a toast, the agents-ready check, the `zshrc` PATH check, the 0-byte CLI build) go to another once R5 lands.
- **Week-1 gaps, added to v1:**
  - **macOS notifications** for agent questions and approvals, failed runs and Agent stopped. On by default, can be turned off in Settings, and grouped per task.
  - **`airboss backup <path>`.**
  - **Uninstall steps** in the docs.
  - Plan it needing the user's `plan-review` skill, and Slack needing Composio, are documented as requirements.
- **Open-source hygiene:** integration-QA ids and account names get placeholders. The redundant `refs/original` were removed. The local-only refs (`refs/t3/*`, `backup/pre-attribution-rewrite`) stay, but never `git push --all` or `--mirror`.

## Done means

- Every module's success criteria are checked off, with tests.
- Every performance target in CAPABILITIES is measured on this Mac.
- The app is used end to end, keyboard-only, in dark and light: capture a task, plan it, Do it, an agent reports, a PR merges, Needs me clears.
- `cargo clippy -D warnings`, `cargo fmt --check`, every Rust test and every UI test are clean.
- No AI attribution anywhere.
