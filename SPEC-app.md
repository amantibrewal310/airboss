# Spec: app

Module `app` from [CAPABILITIES.md](CAPABILITIES.md). State: **draft 2, approved by the EM on 26 Nov 2026**. Draft 2 folds in the staff review and the EM's decisions of 26 Nov 2026 ([tasks/program.md](tasks/program.md)); see [Changes in draft 2](#changes-in-draft-2).

It builds on [`core`](SPEC-core.md) v3 for every rule and on [DESIGN.md](DESIGN.md) for every pixel. `app` adds no rules about tasks. It shows them, and it hosts [`sync`](SPEC-sync.md), [`launch`](SPEC-launch.md) and [`attention`](SPEC-attention.md) in one process through the contract in `crates/services`.

## Objective

A native-feeling Mac app that answers two questions at a glance: what is waiting on me, and where does every task stand. It also puts the two main actions one keystroke away: capture a task, and hand it to an agent.

**User stories** (one per design screen)

- As me, I open the app and land on **Needs me**: questions, reviews, failed runs, agents that stopped without a PR, proposals, newly ready tasks, parents to finish and quiet work, each with its main action on ↩.
- As me, I scan **All tasks** as a tree of 2,000 tasks. Each row shows status, progress, live agent state, repos and when it last changed, and I can move through it without a mouse.
- As me, I select a task and see its **panel**: what needs me, its properties, notes, threads and activity. ⌘↓ opens it as a **full page**, with the run-order **map** for a parent.
- As me, I see every **plan** as a card with its progress, current wave and what's next.
- As me, I press ⌃⌥N in any app, type a title, and the task exists (**quick add**). ⌘↩ also starts it.
- As me, I press ⌘K to **jump** to any task or run any command.
- As me, I change status with S then 1–6, or Space for done, right-click for everything else, and ⌘Z undoes a slip.
- As me, I glance at the **menu bar** for the "waiting on you" count and open an item from there.
- As me, I press ? to see **every shortcut**.
- As me, on **first run** I add my repos from T3's projects and see what else is set up. Without T3 Code, Do it and Plan it switch off and everything else works. When **nothing needs me**, the app says so and offers the ready tasks.

## Decisions (settled in this spec)

1. **Rust owns the view model; the UI draws it.**
   - The Rust side keeps a snapshot of every row (progress, repo union, live state, needs-me flags) and sends **patches**. Svelte never works out tree facts.
   - Each patch has its own increasing `seq`, which is separate from core event ids.
2. **One rebuild path.** These all lead to `catch_up()`, which rebuilds from one `core.snapshot()` and emits a patch only if something changed:
   - core's `subscribe()` (writes in this process)
   - kqueue on `airboss.db-wal` (writes from other processes)
   - `data_version()` when the window gains focus and when the Mac wakes
   - any service's `Changes` bump (which reuses the last core snapshot)
3. **Locking:** the store lock is taken once per `core` call. It is never held while a service is called or while `t3` or `gh` runs. Services hear about a change only after the lock is released.
4. **The service contract is the std-only `crates/services`.** The app binary is the only place that builds the services and wires them together.
5. **Three surfaces, two web views.**
   - The main window and the quick-add panel are web views. The quick-add panel is a non-activating `NSPanel`, created hidden at start-up and reset whenever it hides.
   - The menu-bar dropdown is a native menu.
6. **The mock backend replays real Rust output.**
   - Read fixtures and every scripted write's request and patch are recorded from a seeded `Store` by a Rust test.
   - TypeScript types are generated from the Rust DTOs, with ids as `number`.
   - The mock fails on any call it doesn't know.
7. **Undo lives in Rust:** a stack of inverse calls, 20 entries deep.

## Tech stack

Versions are the latest stable on crates.io and npm on 25 Nov 2026, pinned exactly (`=` in Cargo, no `^` in `package.json`).

| Rust crate | Version | Why |
|---|---|---|
| `tauri` | 2.12.7, features `tray-icon`, `macos-private-api`, `image-png` | The shell. The private API is needed for the transparent quick-add panel. |
| `tauri-build` | 2.7.6 | Build script |
| `tauri-plugin-global-shortcut` | 2.4.1 | The quick-add hotkey from any app |
| `tauri-plugin-autostart` | 2.7.0 | Open at login (LaunchAgent, with `--hidden`) |
| `tauri-plugin-single-instance` | 2.5.5 | A second launch focuses the first |
| `tauri-plugin-window-state` | 2.5.0 | Main window size and position only |
| `tauri-plugin-opener` | 2.7.3 | PRs, docs, Slack and `t3code://` links |
| `tauri-plugin-clipboard-manager` | 2.5.0 | Copy task number, agent note, setup command |
| `tauri-plugin-dialog` | 2.8.4 | Folder picker for "Add another folder…" |
| `tauri-plugin-log` | 2.10.2 | `~/Library/Logs/Airboss/app.log`, rotated at 1 MB, keeping 3 files. The services' lines (`airboss_services::log::line`) and panics on any thread go there too (`logs::install`), since stderr is `/dev/null` when the app opens from Finder. Nothing in the app process writes to stderr directly (`tests/logs.rs` checks the sources) |
| `tauri-nspanel` | 2.1.1 | Quick add as a non-activating panel over full-screen apps |
| `objc2-security` 0.3.2, `objc2-core-foundation` 0.3.2 | the versions already in the lock | Checking the code signature of the app that opens a link (`SecStaticCodeCreateWithPath`, `SecRequirementCreateWithString`, `SecStaticCodeCheckValidity`), in `src/macos.rs` |
| `objc2-user-notifications` 0.3.2, `block2` 0.6.2 | the versions already in the lock | macOS notifications through `UNUserNotificationCenter`, in `src/macos.rs`: permission, posting, and the delegate that hears a click. (`tauri-plugin-notification` was dropped: on macOS it posts through the deprecated `NSUserNotificationCenter`.) |
| `notify` | 9.0.0, `default-features = false`, feature `macos_kqueue` | kqueue on the WAL file and its folder. FSEvents is not used. |
| `objc2` 0.6.5, `objc2-foundation` 0.3.2, `objc2-app-kit` 0.3.2 | the versions Tauri already links | System accent colour and the wake notification, in one module, `src/macos.rs` |
| `serde` 1.0.229, `serde_json` 1.0.154, `thiserror` 2.0.21 | | As in `core` |
| `ts-rs` | 12.0.1 | TypeScript types from the DTOs |
| `toml` | 1.2.3 (the version already in the lock), features `std`, `parse`, `serde` | Reading `~/.codex/config.toml`, read-only, to see whether Codex has the `airboss` MCP server |
| `airboss-core`, `airboss-services`, `airboss-sync`, `airboss-launch`, `airboss-attention` | path | |

| npm package | Version | Use |
|---|---|---|
| `svelte` | 5.59.2 | UI, runes mode only |
| `vite` | 8.3.8 | Build, two entry pages |
| `@sveltejs/vite-plugin-svelte` | 7.3.2 | |
| `@tauri-apps/api` | 2.12.5 | `invoke`, `listen`, window calls. The only runtime dependency besides Svelte. |
| `@tauri-apps/cli` | 2.12.6 | `tauri dev` and `tauri build` |
| `typescript` | 6.0.3 | 7.0 isn't supported by `svelte-check` yet |
| `svelte-check` | 4.7.8 | Type checks |
| `vitest`, `@vitest/browser-playwright` | 5.1.2 | Unit tests in Node; component tests in WebKit |
| `vitest-browser-svelte` | 3.1.0 | Rendering components in Vitest browser mode |
| `@playwright/test`, `playwright` | 1.65.1 | WebKit engine, the smoke test and screenshot baselines |

- **Node 24** is needed to build and test only. Nothing Node runs in the shipped app.
- **None of these:** a component library, a CSS framework, a state library, a virtual-list library, or tokio as a direct dependency. Commands use `tauri::async_runtime::spawn_blocking`, and the app's own workers are std threads.
- **Inter** ships as one local variable `woff2`, Latin subset (about 110 KB).
- **The `airboss` CLI** is a sidecar, not a Cargo dependency.
  - `beforeBuildCommand` builds `airboss-mcp` in release.
  - It copies the result to `src-tauri/binaries/airboss-aarch64-apple-darwin` for `bundle.externalBin`.
  - `bundle.externalBin` lives only in `tauri.bundle.conf.json`, which the release build passes (`pnpm bundle`, that is `tauri build --config src-tauri/tauri.bundle.conf.json`). tauri-build copies a configured sidecar next to every binary it builds, so in `tauri.conf.json` it put an empty stand-in over `target/debug/airboss` on any `cargo build` or `cargo test` (final audit). `build.rs` writes no stand-in, and `tests/build_config.rs` keeps it that way.
- **Release profile:** `lto = "fat"`, `codegen-units = 1`, `opt-level = "s"`, `strip = true`, `panic = "unwind"` (see [Locking and panics](#locking-and-panics)).
- **Unsafe code:** the app crate overrides the workspace's `forbid` with `deny`, and allows unsafe only in `src/macos.rs`, with a `SAFETY:` comment on each block.
- **Platform:** Apple silicon only (`aarch64-apple-darwin`), signed ad hoc.

## Commands

```
cd app && pnpm install --frozen-lockfile
pnpm tauri dev                          # the app; set AIRBOSS_DB for a throwaway database
pnpm dev:mock                           # the UI in a browser, mock backend, http://localhost:5173
pnpm bundle --bundles app,dmg           # release .app and .dmg, with the airboss sidecar (tauri.bundle.conf.json)
pnpm check                              # svelte-check and tsc
pnpm test                                  # Vitest: unit (Node) and component (WebKit)
pnpm test:e2e                           # Playwright smoke test and screenshot baselines on the mock build
cargo test -p airboss-app -p airboss-services
cargo test -p airboss-app --release -- --ignored perf             # rebuild, diff and write timings
cargo test -p airboss-app --test fixtures               # regenerates mock fixtures, scripted writes, TS types
cargo run -p airboss-app --example seed -- --tasks 2000 --db /tmp/ab.db
cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check
scripts/measure.sh                         # size, memory, CPU, start-up, hotkey; prints the machine
```

## Project structure

```
crates/services/                 airboss-services: the contract in "Interfaces"; core and serde only
app/
  package.json  vite.config.ts  vitest.config.ts  playwright.config.ts  svelte.config.js  tsconfig.json
  index.html  quickadd.html      the two entry pages; quick add has its own small bundle
  QA.md                          the manual checklist for what automated tests can't reach
  src/
    main.ts  quickadd.ts
    styles/tokens.css            tokens from docs/design/airboss.css, dark and light
    styles/base.css              reset, fonts, focus ring, reduced motion, increased contrast
    lib/ipc/                     transport.ts (interface), tauri.ts, mock.ts, api.ts (one function per command)
    lib/types/                   generated by ts-rs; never edited by hand
    lib/state/                   tasks, needs, connections, ui, detail, toasts, settings (*.svelte.ts)
    lib/logic/                   views.ts, selection.ts, search.ts, runMapLayout.ts, time.ts, links.ts,
                                 keymap.ts, focus.ts, commands.ts
    lib/components/              StatusIcon, ProgressRing, StackedBar, Pill, Tag, LiveChip, Avatar, Kbd,
                                 Button, Menu, Popover, Modal, Toast, VirtualList
    lib/mock/fixtures/           generated by the Rust fixtures test
    windows/main/                App, Sidebar, Header, TaskList, TaskRow, TaskPanel, TaskPage, RunMap,
                                 WavesList, NeedsInbox, NeedsCard, PlansGrid, PlanCard, Jump, NewTask,
                                 StatusMenu, ContextMenu, StartInMenu, ShortcutSheet, Settings, FirstRun,
                                 AddRepos, Connections
    windows/quickadd/QuickAdd.svelte
  tests/unit/  tests/component/  tests/e2e/  tests/script.json (the scripted writes)
  src-tauri/
    Cargo.toml                   airboss-app
    tauri.conf.json  capabilities/main.json  capabilities/quickadd.json
    icons/  tray/                app icon, menu-bar template icon, one red icon per Needs me kind (Agent stopped: a circle with a pause)
    binaries/                    the airboss sidecar, copied at build time (git-ignored)
    resources/agent-note.md      what "Copy agent note" copies
    src/main.rs                  builder, plugins, windows, run loop
    src/state.rs                 AppState: store, services, view model, undo, settings
    src/viewmodel.rs             rows, progress, repo union, plan cards, diff → Patch
    src/runmap.rs                display waves and lifted edges
    src/catchup.rs               triggers, debounce, catch_up(), service listeners
    src/watcher.rs               kqueue on the WAL and its folder, re-arming
    src/undo.rs                  the undo stack and deferred rejects
    src/commands/                tasks.rs, detail.rs, needs.rs, launch.rs, repos.rs, app.rs
    src/tray.rs  src/menu.rs  src/quickadd.rs  src/settings.rs  src/macos.rs
    src/dto.rs                   every type the UI sees, mirroring core and service types, with ts-rs
    src/errors.rs                AppError and the message table
    examples/seed.rs
    tests/commands.rs  tests/viewmodel.rs  tests/catchup.rs  tests/undo.rs  tests/fixtures.rs
    tests/ipc.rs  tests/perf.rs
scripts/measure.sh
```

Both new crates join the workspace `members`.

## Architecture

### Processes and windows

There is one process, `Airboss.app`, with a Dock icon (activation policy Regular).

| Part | What it is |
|---|---|
| `AppState` | `SharedStore` (one `core` connection), the three services as `Arc<dyn …Service>`, the view model (with the last `Arc<CoreSnapshot>`), `seq`, the undo stack, the in-flight launch set and the settings |
| App threads | the catch-up worker, the WAL watcher, and one listener per service waiting on `changes().wait_past(seen, 15 min)`. All are std threads, wrapped in `catch_unwind`. |
| Main window `main` | 1100 × 720 by default, minimum 900 × 560. Overlay title bar with the title hidden; traffic lights at x 18, y 20; the sidebar top and the 44px header are drag regions. The native `backgroundColor` is the theme's `--bg` (`#0f1012` dark, `#ffffff` light), set at creation and on every theme change, so no white shows before the first paint. Created hidden. |
| Quick-add panel `quickadd` | 680px wide, frameless, transparent window and web view (the card paints `--bg-raised`), always on top, on every Space and over full-screen apps. Created hidden at start-up; its height follows its content. A non-activating `NSPanel`, so showing it doesn't bring the main window forward. |
| Menu-bar item | a template icon, with the Needs me count as its title (nothing at 0), and a native menu |
| Bundled CLI | the `airboss` sidecar in `Contents/MacOS/`. "Install command-line tool" links it to `~/.local/bin/airboss`. |

- **Showing the main window:**
  - The UI calls `ui_ready` from `onMount`, once the bootstrap data has rendered, and Rust then shows the window.
  - If `ui_ready` hasn't arrived 2 s after the window was created, Rust shows it anyway and logs a warning, so a UI failure never leaves an invisible app.
  - Started by autostart with `--hidden`, Rust doesn't show it; only the menu-bar item appears.
- **Closing:** ⌘W or the red button hides the main window. ⌘Q quits (see Shutdown). The Dock icon, "Open Airboss" in the menu-bar menu, and a second launch show it again.
- **Open at login:** autostart uses the LaunchAgent launcher with args `["--hidden"]`. First run turns it on and shows the ticked box (see First run). Settings can turn it off.
- **window-state:** restores the main window's size and position only (`StateFlags::SIZE | StateFlags::POSITION`). `quickadd` is on its denylist.
- **No network:**
  - The CSP is `default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'`.
  - `withGlobalTauri` is off.
  - Each window's capability file allows only the commands that window uses.
- **The native menu bar:**
  - Airboss: About, Check for Updates… ([SPEC-updates](SPEC-updates.md)), Settings… ⌘,, Install Command-Line Tool, Quit ⌘Q.
  - File: New Task ⌘N, Quick Add, Close Window ⌘W.
  - Edit: the predefined Undo, Redo, Cut, Copy, Paste and Select All, which text fields need.
  - Task, View, Window, Help.
  - Task and View list every command. Items whose key is a single letter show it in their title, not as a key equivalent, because a bare-letter key equivalent would swallow typing.
- **Shutdown** (⌘Q or the menu-bar Quit):
  1. Pending deferred rejects are written.
  2. Each service's `shutdown()` is called on a thread. The app waits at most 1 s in total.
  3. The app exits.

### The command layer

- Every command is an `async` Tauri command that runs its body in `spawn_blocking`, so commands never run on the main thread. The body is a plain function, `fn(&AppState, Args) -> Result<T, AppError>`, in `src/commands/`, so tests call it without Tauri.
- **Write commands** do four things, in order:
  1. Make their `core` calls (one lock per call).
  2. Release the lock.
  3. Run `catch_up()`, which tells the services and rebuilds.
  4. Return `Patch` (or a struct holding it).
- All `core` calls act as `Actor::Me`.
- **Which needs fields a write's patch carries:** the patch returns as soon as `core` is written and the services have been *told*. The app doesn't wait for `attention` to recompute (it settles for 150 ms).
  - The needs-derived fields in that patch (the needs-me dot, Ready and Quiet pills, Needs me list) still hold `attention`'s previous values.
  - A follow-up patch and `needs:changed` arrive when `attention` bumps its `Changes`, usually within 250 ms.
  - A Needs me item the user acted on is hidden locally at once, with its 180 ms exit. It stays hidden until `attention` drops it, or for 2 s at most; if `attention` still lists it after that, it comes back.
- **In-flight guard:** `do_it` and `plan_it` hold a per-task entry in `AppState.launching` until `start` returns. A second request for the same task gets `AppError::AlreadyStarting` and the UI ignores it.
- The full command list, each mapped to its `core` and service calls, is in [Appendix A](#appendix-a-commands). Events are in [Appendix B](#appendix-b-events).
- **DTOs** (`dto.rs`) mirror the `core` and service types the UI sees (`TaskDto`, `RowDto`, `LinkDto`, `ThreadDto`, `NeedsItemDto` …), with `From` conversions. `core` and `services` don't depend on `ts-rs`.
  - Every id newtype and `seq` is annotated `#[ts(type = "number")]`. Ids stay well under 2⁵³.
  - `cargo test --test fixtures` writes `app/src/lib/types/*.ts`, and CI fails if they differ from the committed files.

### The view model and patches

`catch_up()` builds from one `core.snapshot()` (`CoreSnapshot { tree, edges, ready, latest_event_id }`) plus `repos()`, `sync.snapshot()` and `attention.needs_me()` (both `Arc` clones). It makes no per-row `core` calls:

| Field of `RowDto` | Source |
|---|---|
| `id`, `parent`, `depth`, `position`, `title`, `status`, `origin`, `blocked_reason` | `tree`; a parent's status and reason are what its steps add up to (core rule 10) |
| `waiting`, `waits_on` | `tree.waiting`, and `edges` whose blocker is open (only when waiting) |
| `has_steps` | `tree.has_steps`: it has subtasks that aren't proposals, so its status follows them and it takes only Drop (core rule 10) |
| `has_children`, `progress` | `{done, total, in_review, doing, blocked}` over leaf subtasks at any depth. Dropped subtasks are left out and proposed ones count as todo (DESIGN "Progress"). `None` for a leaf. |
| `repos`, `repos_from_subtasks` | `Task.repos`; for a parent with none, the union over its subtree, flagged |
| `branch` | `Task.branch`, or the nearest ancestor's (display only) |
| `ready` | in `ready` and a leaf; `ready_since` from it |
| `ready_pill`, `quiet_days`, `needs`, `needs_stale` | from `attention`: a Ready item, a Quiet item, any item, any stale item |
| `live` | the strongest `chip()` of the task's threads in `sync.snapshot().threads_for(id)`: Waiting on you, then Working, then Your turn, then Stopped. Only while the task is open. |
| `last_at`, `last_actor` | `subtree_last_event_at` for a parent, `last_event_at` for a leaf, and `last_actor` |
| `launched_in` | when `last_actor` is Launch: the `Source` (T3 or Herdr) of the task's most recently active linked T3 thread or Herdr terminal, so the avatar reads T (T3) or H (Herdr) in place of L. `null` otherwise |

- **Plan cards, display waves and the dependency lists** in the panel are built from the same `tree` and `edges`.
- **`diff(old, new)`** gives `Patch { seq, upserts, removed, order, repos, created }`:
  - `upserts` are whole `RowDto`s; `removed` are task ids.
  - `order` (ids in tree order) and `repos` are sent only when they changed.
  - `created` drives the new-row highlight.
- **The UI** applies a patch only if its `seq` is greater than the last one applied. A write's returned patch and the same patch arriving as an event therefore apply once.
- **Repo colours** are fixed by id: `repo-((id − 1) mod 4) + 1`. A repo keeps its colour for ever, as the Owned Colour Rule asks; a fifth repo shares repo-1's colour.

**Budget** at 2,000 tasks and 1,000 dependencies, on the Mac where `core` v2 measured `tree()` at 4.3 ms (95th percentile, Apple M4):

| Step | Budget |
|---|---|
| `core.snapshot()` (tree, edges, ready in one read) | 6 ms |
| Build rows and diff | 3 ms |
| `catch_up()` in total, including `events_after` and `repos()` | under 10 ms |
| A write command, returned to the UI | under 15 ms |
| A rebuild on a service bump (no `core` read) | under 4 ms |

### Seeing writes from other processes

- **Why kqueue:** FSEvents doesn't report writes to a WAL file that a connection keeps open (checked on this Mac by the reviewer). So `watcher.rs` uses `notify`'s kqueue watcher on two paths:
  - **`<db>-wal`** (from `Store::path()`): every write by any process fires a vnode write event.
  - **The database's folder** (non-recursive): an entry created or deleted there re-arms the WAL watch. That covers a WAL deleted when the last connection closes and created again, and a rename or revoke of the WAL.
- **Debounce:** a trigger waits 30 ms after the last event, and never more than 150 ms after the first. Then it runs `catch_up()`.
- **Every trigger goes to one std `mpsc` channel** read by the catch-up worker:
  - the kqueue callback
  - a small thread forwarding core's `subscribe()`
  - the main window gaining focus
  - the wake notification from `macos.rs`
  - the service listeners
- **On focus and on wake,** the worker compares `data_version()` with the last value seen. It skips the rebuild if nothing changed. On wake it also calls `woke()` on every service.
- **`catch_up()` for a `core` trigger:**
  1. `events_after(cursor)` and `repos()`, one lock each. The CLI's `repo add` writes no event, so `repos()` is compared too.
  2. If both are unchanged, stop. Our own writes, the services' writes to their own tables and WAL checkpoints cost two small reads.
  3. `snapshot()`, one lock.
  4. Release the lock. Call `store_changed(&StoreChange { events, snapshot })` on sync, launch and attention. Each must only queue the change and return.
  5. Rebuild, diff, increment `seq` and emit `tree:patch`.
- **For a service trigger,** `catch_up()` skips `core`: it rebuilds from the last snapshot and the services' new snapshots. A bump from `attention` also emits `needs:changed`; a change in any `connections()` or `availability()` emits `connections:changed`.
- **Inline and background runs share one lock.** A write's inline `catch_up()` and the worker's are serialised by a `catchup` mutex, which is separate from the store lock and owns the cursor, `seq` and the view model.
- **Cursor at start:** `latest_event_id` from the first snapshot.
- **The file disappears or is replaced:** the footer goes orange with "Database moved", and the popover says to quit and reopen.
- **Target:** an agent's `update_task` shows in an open window within 300 ms.

### Locking and panics

- **The store lock:**
  - `SharedStore` is `Arc<Mutex<Store>>`.
  - The app locks once per `core` call and drops the guard before the next call.
  - It never calls a service, emits an event or touches the UI while holding it.
  - `launch.start` and `sync.lookup_pr` (which run `t3` and `gh`) are always called with no lock held.
- **A poisoned store lock** is recovered with `into_inner()` (rusqlite rolls back the open transaction when it's dropped) and logged once.
- **Panics:**
  - The release build unwinds.
  - Each command body runs in `catch_unwind` and turns a panic into `AppError::Internal`.
  - Each app thread restarts its loop once after a panic. A second panic stops it, and the footer shows "Live updates stopped after an internal error. Restart Airboss."
  - Service threads catch their own panics (EM decision) and report through `connections()`.

### UI state (Svelte 5 runes)

The stores are classes in `lib/state/*.svelte.ts`, one exported instance each, with `$state` fields and `$derived` getters. Components read them; only store methods write them.

| Store | Holds |
|---|---|
| `tasks` | `rows: Map<number, RowDto>` and `order: number[]` as `$state.raw` (no deep proxies over 2,000 objects; a patch swaps in a new `Map` and replaces only changed row objects); `repos`, `seq`, `fresh` (ids to highlight); `applyPatch()` |
| `needs` | items, count, `checkedAt`, the selected key, `hiddenLocally` (key → expiry) |
| `connections` | the connection rows, and `launch` availability |
| `ui` | view (`needs`, `ready`, `plans`, `progress`, `all`, `repo:<id>`, `parent:<id>`), layout (`tree` or `status`), selection and its index, map selection, folded ids, panel open, page open, focus zone, open overlay, rename target, `starting: Set<number>` |
| `detail` | the `TaskDetail` for the selection, with the request number it answers |
| `toasts`, `settings` | |

- **Visible rows** come from pure functions in `logic/views.ts`, `(rows, order, view, folded, layout) → VisibleItem[]`, wrapped in one `$derived`.
- **Selection rules** (`logic/selection.ts`, pure and unit-tested):
  - When a patch, a view change or a fold removes the selected row from the visible list, the row now at the same index is selected, clamped to the last row. If the list is empty, nothing is selected and the panel closes.
  - Folding a parent that holds the selection selects that parent.
  - Revealing a task (Jump, menu bar, toasts) unfolds its ancestors, and switches to All tasks if the current view doesn't show it.
  - In Needs me, when the selected item leaves, the item at the same index is selected.
  - A map selection clears when its station disappears.
- **Stale responses:** every `task_detail`, `run_map` and `plans` request carries a request number. The store ignores a response that isn't the latest for the current selection or view.
- **Detail refetch:** `detail` refetches when a patch upserts the selected id. A parent's id is upserted whenever a subtask changes, because its `last_at` moves.
- **Folds** are saved in settings, debounced 1 s. Parents start folded, except the ancestors of the selection.
- **Times** ("6m", "2d") read a `now` value that ticks once a minute, only while the window is visible.

### Virtualised list

`VirtualList` is our own component, about 150 lines:

- **Heights:** task rows are 38px and group headers 34px (By status layout only). Offsets are a prefix-sum array rebuilt when the visible list changes, and the first visible item is a binary search.
- **Rendering:** rows sit in a `contain: strict` scroller, positioned with `transform: translateY` and keyed by task id, with 8 rows of overscan.
- **The sticky group header** is one extra element mirroring the current group, not CSS `sticky` inside the absolute layer.
- **Scrolling:** moving the selection scrolls just enough to keep it visible (`nearest`), with no smooth scrolling.
- **`TaskRow`** takes one `RowDto` plus selection and focus flags. It has no `$effect` and no per-row listeners; the list handles clicks by delegation.
- **Narrow widths** follow DESIGN's two steps, through a container query on the list.

### Keyboard, focus and menus

- **One registry:** `logic/commands.ts` lists every command with its id, label, keys, the focus zones it works in and `run()`. The keymap, the native Task and View menus, the context menu, ⌘K's command list and the shortcut sheet are all built from it.
- **Focus zones** (`logic/focus.ts`): `list`, `panel` (the panel or page, outside any field), `sidebar`, `field` (any input, textarea or rename), `overlay` (a menu, popover, Jump, New task, the sheet, Settings) and `quickadd`. The rules per command are in [Appendix C](#appendix-c-keyboard-map-and-focus-rules). In short:
  - In a `field`, only ⌘K, ⌘N, ⌘,, ⌘W, Esc and the Edit menu act. Every other key belongs to the field: ⌘↩ saves the field, ⌘Z undoes typing, ⌘⌫ and ⌘↑↓ edit text.
  - Single letters, Space and arrows act only in `list`, `panel` and `sidebar`.
  - An open `overlay` takes every key first, and Esc closes the top one.
- **The page handles keys first.** One `keydown` listener on `window` handles every shortcut it owns and calls `preventDefault()`. Native menu items with key equivalents fire only for keys the page left alone, and when clicked.
- **Menus follow focus.** The UI sends `set_focus_context { zone, selection, view }` whenever these change (once per animation frame at most). Rust then:
  - disables every Task-menu item while the zone is `field` or `overlay`, so their key equivalents can't fire through a text field
  - disables items that need a selection when there's none
  - relabels "Undo <action>"
- **While quick add is the key window,** the main window's menu commands don't act: Rust's menu handler checks `quickadd.is_focused()` first. Only the Edit items and ⌘W (which closes quick add) work.
- **A safety net:** if a key and its menu item both fire for one press, the second call within 50 ms is ignored.

### Undo

- **Stack:**
  - `undo.rs` keeps up to **20 entries**. The oldest drops off.
  - Each entry has a label ("Mark done 144"), the inverse calls, the affected task ids, and the newest event id the action wrote.
  - The stack is cleared on quit. An entry is dropped when one of its tasks is deleted.
- **What's undoable, and how:**

  | Action | Inverse |
  |---|---|
  | Dismiss, Keep open, Snooze | `attention.restore(key)` |
  | Status change (Space, S, ⌘B, Mark done, Set in review) | `set_status` back to the previous status, with the previous blocked reason as the note if it was blocked |
  | Drop | the same, for the task and every open subtask the drop took with it (previous statuses come from the view model before the change). A task with steps isn't put back itself: its status comes back through its steps (core rule 10). **Not for an agent's proposals:** rule 9 drops them with their parent, but `me` can't make a proposal again (core rule 6), and putting one back as todo would accept it without me. So they stay dropped, and the undo toast says "Undid “Drop 142”. Its 2 proposals stay dropped." Dropping a proposal on its own is a reject, with no undo once written. |
  | Move or reorder | `move_task` back to the previous parent and position. Reorder moves past siblings the tree hides (rejected proposals, dropped tasks) to the visible neighbour's position, and an end move changes nothing and pushes no entry |
  | Reject, Reject all | **deferred:** the reject is written 5 s later (or at quit). Until then the item and rows are hidden locally, and undo cancels it. `core` can't turn a task back into a proposal, so this is the only honest undo. |

- **Writing a deferred reject** (`undo.rs`, final audit should-fix 4): one call to core's `reject_proposals(ids, reason, me)` per Reject or Reject all. It checks that each task is still a proposal and writes in one IMMEDIATE transaction, so an `airboss accept` from another process can't land between the check and the write. Tasks accepted or dropped meanwhile come back as skipped, with their status, and are left as they are; an info toast names them: "153 was already accepted; left as is", "154 was already dropped; left as is", or both in one sentence. A busy database is retried after 250 ms and again after 1 s (each attempt waits out the 2 s busy timeout). A reject that still fails shows a warning toast, "Couldn't reject 153: the database stayed busy.", with **Retry**, which rejects again with a fresh 5 s undo window. The proposal shows again meanwhile, so nothing is lost silently.

  Accept, create, rename, notes, links, dependencies, repos, Do it and Plan it aren't undoable in v1. Text fields keep their own undo.
- **Refused undo:** if `events_after(entry.last_event)` shows that someone else changed one of the entry's tasks, the undo is refused with "144 changed since; nothing was undone", and the entry is dropped.
  - `entry.last_event` is the **catch-up cursor** the view model was built at, read with it under one lock (`AppState::vm_at`), not the newest event id. The inverse comes from the view model, so an agent's write the app hasn't caught up with yet (it sets in_review, I press Space within the 150 ms debounce) counts as "changed since", and ⌘Z is refused instead of overwriting it.
- **Where undo appears:**
  - ⌘Z, when the zone isn't `field`, undoes the top entry.
  - A toast for an undoable action shows "Undo"; that button undoes *its* entry, if it's still there.
  - Task › "Undo <label>" does the same as ⌘Z, without its own key equivalent; Edit's predefined Undo stays for text fields.
- **Toasts:** a toast with an action stays 5 s instead of DESIGN's 2.8 s, and pauses while hovered. F6 moves focus to the newest toast and back.

### Errors

`AppError { code, message, detail? }` is what commands return. `ServiceError { code, message }` from a service passes through with its own message, which the service specs write. The app branches on a few codes only: `NeedsChoice`, `NotAProject`, `TaskClosed`, `T3Missing` and `T3NotRunning`. Every other code shows its message. The app's own messages are in [Appendix D](#appendix-d-error-messages).

| Where it fails | How it shows |
|---|---|
| An action I took | A toast with an orange warning icon and the message. The row keeps its real state, because the UI only ever shows what Rust returned. |
| A form (New task, link, blocked reason) | Inline under the field. Create is disabled only for an empty title. |
| A service (`gh` signed out, T3 missing, watcher failed) | The connections footer turns orange and names it, and the popover gives the fix. Never a stream of toasts. |
| Do it, when the main window is hidden (from quick add) | On any failure or partial failure, Rust shows the main window, selects the task and shows the toast. A success shows nothing. |
| The database won't open at start | A full-window state: "Airboss can't open its database", the path, the reason, "Show in Finder" and "Try again". The path comes from core's `default_db_path()` (`AIRBOSS_DB`, or Application Support), so with neither `AIRBOSS_DB` nor `HOME` this state shows core's reason ("HOME isn't set … set AIRBOSS_DB") instead of a relative path, and settings aren't saved. |
| A newer Airboss migrated the database (core's `Error::is_newer_database`, checked before the generic database message) | At start, or on the first write it refuses while the app runs: a full-window "**Update Airboss**" state with "A newer Airboss has updated this database. Install it, then open Airboss again.", the path, and "Show in Finder" (no Try again). Like the database problem state, it's sent once as `db:unavailable` (code `db_too_new`), and every command but the window plumbing answers with it. |
| The database breaks while the app runs (integration QA R2) | The same state, headed "Airboss's database stopped working", with "Show in Finder" only: the fix is to quit and reopen. Two cheap signs (`dbhealth.rs`): a `core` call failing with an I/O or corruption error (`Error::is_storage_failure`: SQLite's `IOERR`, `CORRUPT`, `NOTADB` or `CANTOPEN`, not busy or locked), and one `stat` on each catch-up showing the `-wal` file gone or replaced (another device or inode than the one the app holds). The app sends `db:unavailable` once, and from then on every command except `open_url`, `ui_ready`, `show_main`, `set_focus_context` and the quick-add panel's answers fails with `db_unavailable`, so nothing is saved into a database that will lose it. SQLite isn't repaired. |

Red is never used for errors (The Red Is Mine Rule).

### Opening links

Agents can store any URL through MCP `link`, so every link the app opens (⌘O, a link tag, Open PR, Open thread) goes through one strict allowlist, `opener::decide` (final audit B1). Nothing else reaches macOS's `open`.

| Form | What happens |
|---|---|
| `https://host[:port]/…`, printable ASCII, no user info and no `\` | Opens in the browser, once the browser is verified (below) |
| `t3code://threads/<environment>/<thread>`: the environment is letters, digits and `-`; the thread id adds `_` and `%XX`; nothing after it. The thread id is judged again once decoded: UTF-8, no `/`, `?`, `#` or `\`, no control byte, and not `.` or `..` (so `%2E%2E%2F…`, `%3F` and `%00` are refused; `mcp%3Alaunch%3A…` passes) | Opens in T3 Code, once it's verified (below) |
| `file:///absolute/path`: no host (not even `localhost`), no query, no fragment | Percent-decoded (it must be UTF-8, with no control byte, escaped or not: `%0A` and `%1B` are refused), resolved (symlinks and `..`) and required to exist. A plain, non-executable file whose extension (any case) is on the list below opens in a **fixed system viewer**, `open -a /System/Applications/TextEdit.app` for text and code or `Preview.app` for PDFs and images, never in the app LaunchServices would pick. Everything else is **shown in Finder**: folders, bundles (`.app`, `.workflow`, `.rtfd`), `.command`, `.terminal`, `.sh`, `.py`, `.html`, `.svg`, executables, files without an extension, and lookalike extensions |
| anything else, `http` included | Refused: "Airboss only opens https links, files and T3 threads." |

- **Documents that open:** `md`, `markdown`, `txt`, `text`, `rtf`, `log`, `csv`, `tsv`, `json`, `jsonl`, `yaml`, `yml`, `toml`, `xml`, `ini`, `cfg`, `conf`, `diff`, `patch`, `pdf`, `png`, `jpg`, `jpeg`, `gif`, `webp`, `heic`, `heif`, `tif`, `tiff`, `bmp`, and the code files `rs`, `ts`, `tsx`, `js`, `jsx`, `mjs`, `cjs`, `svelte`, `vue`, `css`, `scss`, `go`, `java`, `kt`, `swift`, `c`, `h`, `cc`, `cpp`, `hpp`, `sql`, `graphql`, `proto`. Each one's default app shows it and runs nothing; HTML and SVG are left out because a browser would run their scripts, and `.py` because Python Launcher runs it.
- **What's opened** is the resolved path, never the URL text, so a symlink is judged by what it points at.
- **Why a fixed viewer** (found by a security review of this fix): a plain `open` lets LaunchServices choose the app, and an agent with a shell can steer that choice for a file that passes every check above: a Finder alias named `plan.md` (a regular file, so not resolved as a symlink) pointing at an app, a per-file `com.apple.LaunchServices.OpenWith` attribute, an app it registers as the default for a type, or a file swapped between the check and the open. With `open -a` and a system viewer named by path, each of those only ever reaches TextEdit or Preview, which show it and run nothing.
- **Which app opens a web or T3 link** (`handlers.rs`, the second security audit): a plain `open <url>` lets LaunchServices pick the app, and an agent can register one. An `Evil.app` it writes next to a file Airboss reveals, claiming `com.t3tools.t3code`, version 999 and the `t3code` scheme, is registered the moment Finder shows its folder, and could then take Open thread. So the handler LaunchServices would use is looked up (`NSWorkspace.URLForApplicationToOpenURL`), resolved, and its signature checked with the Security framework (strict validation, nested code included) against a requirement, and the link is opened with exactly that app (`open -a <app> -- <url>`):
  - **T3 links:** `identifier "com.t3tools.t3code" and anchor apple generic and certificate leaf[subject.OU] = "ARK85ZXQ4Z"` (T3 Tools, Inc.'s Developer ID). The app's name and folder don't matter: it's "T3 Code (Beta).app" today. Refused: "Couldn't verify the app that opens T3 links. Open the thread from T3 Code."
  - **Web links:** `anchor apple generic` (Apple, the App Store or a Developer ID), and the browser can't live in the home folder except `~/Applications`. Refused: "Couldn't verify your browser. Copy the link instead."
  - A refusal (`app_unverified`) carries the link as its detail, and its toast offers **Copy link** (`copy_text` with `link`).
  - The lookup and the check sit behind the `AppCheck` trait: `FakeAppCheck` in tests (genuine T3 Code and Safari unless a test says otherwise), `MacAppCheck` in the app. `tests/handlers_real.rs` (`#[ignore]`) checks this Mac: the installed T3 Code passes, and a freshly made lookalike bundle fails, unsigned or signed ad hoc; the default browser passes.
- **Left as it is:** a revealed item is selected in Finder, so pressing ⌘O in Finder next would open it. That's Finder's own behaviour, the same as for any file the user browses to.
- A missing file says "That file no longer exists."; one Airboss can't resolve says "Airboss can't reach that file."; a FIFO, socket or device (`/dev/stdin`) says "That isn't a file or folder Airboss can show."
- `opener.rs` has a table-driven test of every trick in the audit (`file:///tmp/x.command?a`, `file:/System/Applications/Calculator.app?://`), plus mixed case, `file://localhost`, `..` and `%2E%2E`, symlinks to executables, and Unicode lookalikes (a Cyrillic `і` in the scheme, a Cyrillic `ԁ` in an extension, a fullwidth dot, a right-to-left override). A second table runs the second security audit's list on files laid out as the auditor laid them out: `////`, `/./`, `%2F` in the path, escaped `?` and `#`, trailing dots and spaces, NFC and NFD names, `%0A` and `%1B`, hard links and symlinks to executables, a FIFO, `/dev/stdin`, `.webloc` and friends, hosts and `;`, plus its https and t3code strings.

## Screens

Every screen keeps DESIGN.md's layout, sizes and motion. This section adds data, behaviour and states. Keys per context are in [Appendix C](#appendix-c-keyboard-map-and-focus-rules).

### Sidebar and connections footer

- **Data:** `tasks` (counts), `needs.count` and `connections`.
- **Counts:** Needs me is the attention count, as a red badge hidden at 0. Ready to start counts ready leaves. Plans counts plan cards. In progress counts `doing` and `in_review`. All tasks counts open tasks.
- **Parents:** open top-level tasks with subtasks, in position order. Each shows its status icon, percentage and ring, and opens the per-parent view.
- **Repos:** each repo with its colour dot, opening the per-repo view. A "+" on the section header opens Add repos.
- **Update card:** directly above the footer, only while an update has something to say: available, downloading, installing, restarting or failed ([SPEC-updates](SPEC-updates.md), decision 5).
- **Footer:**
  - It reads "All connected" with a green dot, or names the first problem in orange, or says "N connections need setup".
  - Clicking it opens the 300px popover, with one line per row below and a "Check again" button (`recheck_connections`).

  | Row | From |
  |---|---|
  | T3 Code | `launch.availability()` and the T3 connection from `sync`. OK only if both are OK; otherwise the first one that's off. |
  | GitHub | `sync`'s GitHub connection |
  | Agents | the app: whether the CLI works, whether an agent tool has the `airboss` MCP server, and the time of the newest `agent:*` event. "Ready" needs `~/.local/bin/airboss` to lead, through the link, to a non-empty executable file; a dangling link (an Airboss.app that moved) is "Link broken". It also needs Claude Code or Codex to have the server (hands-on QA #9): with the CLI but neither, the row is "Not connected", "Agents can't report to Airboss yet." The app reads, read-only, `~/.claude.json` (a top-level `mcpServers` object with an `airboss` key; a project's own `mcpServers` doesn't count) and `~/.codex/config.toml` (an `airboss` table under `mcp_servers`, however it's written), both under the app's home folder. A missing, unreadable or malformed file is "not connected" for that tool, logged once per change of the file, never an error. Connections are recomputed on every catch-up, so each file is read again only when its modification time or size changes. Which tools have it is sent as `agents` with `connections:changed` and `bootstrap`. Ready's line reads "Last report 3m ago", or "No agent has reported yet." The time is tracked from catch-up's events, starting from the newest agent event found at start (the last 256 events, then 4,096, then all), and sent as `agent_report` with `connections:changed` |
  | Claude Code | `sync`'s Claude connection, shown only when it's broken |
  | Needs me | `attention`'s "Didn't start" row, only when its table couldn't open |

- The search and pencil icons at the top open Jump and New task.

### List views

| View | Rows | Layout |
|---|---|---|
| Ready to start | ready leaves in tree order; the time column shows when each became ready | flat |
| In progress | `doing` and `in_review`, tree order | flat |
| All tasks | the whole tree minus dropped; done rows dimmed | Tree or By status |
| Per repo | tasks whose own or inherited repos include it | flat, tree order |
| Per parent | the parent's subtree, its children at depth 0 | tree |

- **Header:** title, row count, the Tree / By status switch (All tasks only), Jump ⌘K and New task.
- **By status** groups, in order: In progress, In review, Blocked, Todo, Waiting, Proposed, Done.
- **Rows:** exactly DESIGN "Task rows".
  - Click selects and opens the panel; double-click opens the page; right-click opens the context menu.
  - The status icon opens the status menu, and the arrow folds.
  - A row with `needs_stale` dims its needs-me dot and gets the tooltip "May be out of date".
- **Reorder:** ⌥⌘↑ / ⌥⌘↓ move the selected task one place among its siblings (`reorder`), only in tree layouts. At either end nothing happens. In flat views it shows "Reorder in All tasks or a parent's view".
- **Empty view:** "Nothing here" and "No tasks match this view." Ready adds "Tasks show here when nothing they wait on is open."
- **Loading:** none. The data is in memory before the window shows.
- **Motion:** new rows fade the indigo wash out over 600 ms, and a status change pops the icon.

### Task panel (480px, or 380px when the window is 980px or less)

- **Data:**
  - `task_detail(id)`: the task, its links (with `PrView` and the threads' live state), `history_with_subtasks(id, 30)`, `waits_on` and `blocks` from `edges`, display waves if it has subtasks, `attention.for_task(id)`, and `launch.prepare(id, DoIt)` for the branch preview and repo choices (skipped for proposed, done and dropped tasks).
  - Opening a task with an open PR also calls `sync.refresh(Refresh::PrsOf(id))`, which `sync` rate-limits.
- **Header:**
  - The breadcrumb ("Booking flow › 144"); clicking the parent selects it.
  - Plan it (⇧⌘↩), Do it (primary, ⌘↩), open page (⌘↓) and close (Esc).
- **Do it states:**
  - While the task is in `ui.starting`, the button reads "Starting…" with a small spinner and is disabled. A second ⌘↩ does nothing.
  - On a task with no repo (`prepare` says `needs_choice`), Do it opens the **Start in menu** under the button. It lists "Start in <repo>" for each repo (this links the repo as me, then starts) and "No repo: research only". ↑↓ and ↩ pick, Esc closes.
  - On a done or dropped task (`TaskClosed`), the toast offers "Reopen and start". That sets the task to `todo`, then starts it.
- **T3 missing:**
  - An orange-iconed line sits under the header: "Starting agents needs T3 Code. Install it, then choose Check again."
  - Do it and Plan it stay focusable but read as disabled (`aria-disabled`). Pressing them repeats the line as a toast.
- **Title:** click it or press ↩ to rename in place. ↩ or blur saves; Esc cancels.
- **Needs me callouts:** one red-washed callout per item, with the item's main action and Dismiss.
- **Properties:**
  - **Status** opens the status menu.
  - **Repos** are tags, plus "Link repo", which opens the repo checklist; its last line is "Add repos…". The tags carry "· from subtasks" when the repos are inherited.
  - **Branch** (mono) shows the task's branch, or in text-3 the one Do it would use (`Prepared.branch`). Click it to edit (`set_branch`); `core` refuses characters outside `[A-Za-z0-9._/-]` and the message shows inline.
  - **Due** shows the day the task is due ("Fri 4 Dec", red-washed once it has come), or "None · set a date". Click to pick a day in a date field (`set_due`), × to clear it; a refusal shows inline.
  - **Parent** opens Jump in "Move under…" mode (`move_task`, last position).
  - **Waits on** lists every dependency, finished ones too, each with its status icon, or "Nothing · ⌘D to add". Hovering a tag shows ✕.
  - **Blocks:** the same, read-only.
  - **Links:** tags titled by the link title, or by kind and ref. Clicking opens it; ✕ unlinks. Otherwise "None · ⌘L to add".
- **Adding a link (⌘L):**
  - A popover with "Paste a link or path" and an optional title.
  - Below the fields it lists `sync.snapshot().suggested_prs(branch)`: my open PRs from this task's branch, one click each.
  - A PR link is checked with `sync.lookup_pr` in the background. Its title and state fill in when the lookup answers.
- **Notes:** the body in Notes type. Click to edit in an auto-growing textarea; ⌘↩ or blur saves, and Esc cancels.
- **Subtasks:** the Waves list, when the task has subtasks.
- **Threads:** one row per linked thread: live chip (Running, Waiting, Your turn, Stopped), title, request or error summary, and "Open ↗" (`open_thread`). Open shows only when `sync.thread_url` returns a URL. Claude Code sessions have none, so they show their folder instead.
- **Activity:**
  - One line per event: the actor's avatar and name (You, Agent or "Agent in <thread title>", GitHub, T3, Plan, and Launch named by where its thread runs: T3 or Herdr, "Launch" only when the feed has no thread link to tell), then a verb phrase per event kind.
  - A subtask's events start with its id: "143 moved to Done".
  - "Show more" refetches with a limit of 200.
- **Note input:** "Leave a note for the agent…". ↩ adds the note, ⇧↩ adds a line.
- **Loading:** the previous task stays until the new detail arrives. Only after 150 ms do the sections show grey skeleton lines.
- **Error:** a task deleted elsewhere shows "Task 144 no longer exists", and the selection rule picks the next row.

### Full task page

- ⌘↓ or double-click opens it; ⌘↑ or Esc goes back to the list with the same selection.
- The layout is a centred document column (max 860px) and a 280px properties rail, with the panel's data. The rail adds Progress ("13% · 1 of 8").
- **Subtasks:** "N waves", the 8px stacked bar with its legend, the Waves / Map switch (opening on Map), and "Nothing starts by itself. Select a ready task and press ⌘↩."
- A leaf task's page has no subtasks section.

### Run-order map

- **Data:** `run_map(id)`, built in `runmap.rs` from the view model's snapshot and one `run_order(id)` call:
  - **Nodes** are the parent's direct subtasks that aren't dropped or proposed. Done ones are included.
  - **Edges** are dependencies lifted to those subtasks: a descendant's edge counts for its top-level subtask under the parent, as in rule 18. An edge is marked done when its blocker is done.
  - **Display waves:** Kahn layering over all the nodes, done ones included, in sibling order.
  - **Outside blockers** come from `run_order(id).waiting_outside`. Such a task sits in the column after its latest inside blocker, or in wave 1.
  - **Tangled** tasks are listed under the map with "These wait on each other in a loop."
- **Drawing:** `runMapLayout.ts` (pure, unit-tested) turns the graph and the width into station positions and SVG paths, following DESIGN's curve and wave-skip rules. Columns are 150px or wider, and stations in a wave are 80px apart.
- **Controls:**
  - Hovering a station traces its lines.
  - Click selects it, and then ⌘↩ starts that subtask. Double-click opens it.
  - With the map focused, ←→ move between waves, ↑↓ within a wave, ↩ opens and Esc clears.
- **Fallback:** above 40 subtasks or 8 waves, the page opens on Waves, and the Map tab says "Map shows up to 40 subtasks".

### Needs me

- **Data:**
  - `needs`, from `attention.needs_me()` and pushed with `needs:changed`.
  - `task_detail` for the selected item's task.
- **Inbox:** the 420px list, grouped in DESIGN's fixed order. ↑↓ move, and the first item starts selected. A stale item is dimmed and says "May be out of date".
- **Detail:** the card (kind, time, task, headline at 17px, actions), then the task's properties, notes and activity.
- **Actions** (the main action is ↩ or ⌘↩; Dismiss is ⌘⌫; Mark done is Space where the card offers it):

  | Kind | Main action | Others |
  |---|---|---|
  | Question, Approval, Failed run | Open thread (`open_thread` on the item's `ThreadKey`) | Dismiss |
  | Agent stopped | Open thread (`open_thread` on the most recently active of the task's threads) | Mark done (`set_status` done, Space), Set in review (`set_status` in_review), Snooze 3 days, Dismiss. Mark done and Set in review toast with Undo |
  | Review | Open PR (`open_url`) | Dismiss |
  | Proposed | Accept all (`accept_proposals`) | Accept, Edit (rename in place, then accept), or Reject each; Reject all |
  | Ready | Do it (`do_it`) | Dismiss |
  | Quiet | Open (reveals the task in All tasks) | Snooze 3 days (`needs_snooze` with no date, so `attention` adds its default) |

- **Clearing:**
  - An acted-on item leaves at once: it slides 10px left and fades over 180 ms (see "Which needs fields a write's patch carries").
  - An item resolved elsewhere leaves when `needs:changed` drops it.
  - The selection rule picks the next item.
  - Toasts for Dismiss, Snooze and Reject offer Undo.
- **Header:** "Needs me N", and "checked 10:44" from `checked_at`.
- **Empty:** DESIGN "Nothing needs you": a check, "Checked 10:44 · 9 in progress · 5 ready to start", and "Show ready tasks".

### Plans

- **Data:** `plans()` returns one `PlanCard` per open top-level parent, built from the view model:
  - progress
  - display waves: the count, the current one (the lowest with an open task) and one dot per wave
  - Next up: up to two ready subtasks, or "Nothing ready". A plan whose steps are all done shows done (core rule 10), so it has no card.
  - working agents and blocked subtasks
  - the repo union and `last_at`

  It is refetched when a patch upserts a parent while Plans is open, debounced 100 ms.
- **Header:**
  - "Plans N", and "35% of all planned work done · 6 of 17".
  - "Plan something" opens New task in plan mode: ↩ creates the task, then runs Plan it.
- **Grid:** cards at least 340px wide, 12px apart. ←→↑↓ move by card and by row, and ↩ opens the page.
- **Empty:** "No plans yet", "Press Plan it on a task and an agent works out the plan with you. Its steps land here as subtasks.", and Plan something.

### New task (in the app) and quick add (from anywhere)

One `NewTask.svelte` component serves both: as a modal on the scrim in the app, and as the whole quick-add window.

- **Opening:**
  - **In the app:** C, ⌘N or ⌥N. A per-parent view presets the parent, and a per-repo view presets the repo.
  - **From anywhere:** the global hotkey, by default **⌃⌥N**.
    - The handler acts on `ShortcutState::Pressed` only, so holding or releasing the keys does nothing more.
    - Rust positions the panel 22% from the top of the screen with the mouse, then shows and focuses it.
    - Pressing the hotkey while the panel is the key window hides it.
- **Ready before it opens:**
  - Every time the panel hides, the UI resets the form, refetches `quickadd_data` (repos and open parents) and focuses the title. Opening therefore does no I/O and no rendering.
  - The one exception: a hide caused by clicking elsewhere keeps a non-empty title and notes, with the cursor at the end.
  - Esc and Create always reset.
- **Fields:**
  - The title (18px). ↓ in the title moves to notes, and ↑ on the first line of notes moves back.
  - Notes ("Add notes, or paste a Slack link…").
  - Property pills: status (Todo, fixed), "Repos · optional" (a checklist: ↩ or click toggles a repo; in the app its last line is "Add repos…"), and Parent (a filterable list of open tasks plus "Top level"). Tab opens repos and Shift-Tab opens parent. Inside a dropdown, ↑↓ move and Esc closes it.
- **Pasting** a link into the title or notes attaches it as a link pill (with ✕) and keeps the rest of the text. `links.ts` sorts each link by its form:

  | Form | Kind |
  |---|---|
  | `github.com/<o>/<r>/pull/<n>` | PR, ref `o/r#n` |
  | `github.com/<o>/<r>/issues/<n>` | GitHub issue |
  | `*.slack.com/archives/…` | Slack |
  | `t3code://threads/<environment>/<thread>` | T3 thread, ref `<thread>` |
  | an absolute or `~` path | doc |
  | any other `https` URL | doc, with its URL |

  GitHub and Slack links are kept as `https`, whatever form was pasted. A plain `http` link, or any URL the opener would refuse (below), is refused when pasted with the opener's message, so a link that's kept is a link that opens.

- **↩ Create task:** `create_task`. The repos go in `NewTask.repos`, so `add_task` links them in the same change, and then each link is added.
  - The panel hides at once. In the app, the new row is highlighted and selected.
  - If a link fails after the task exists, the task stays, and a toast says "Created 153, but couldn't link <title>". From quick add, that shows the main window.
- **⌘↩ Create and Do it:** the same, then the Do it flow, including the Start in menu if there's no repo. From quick add, the menu opens in the panel and the panel stays open until a choice is made.
- **Errors:** an empty title disables Create.
- **Header:** "⌃⌥N from any app", or the hotkey set in Settings.

### ⌘K Jump

- **Data:** in memory. `search.ts` fuzzy-matches id and title over `tasks.rows` (dropped left out), plus the command registry.
- **Results:** with no query, the 5 most recently changed open tasks and 4 commands. With a query, 8 tasks and 5 commands. A number puts that id first.
- **↩** on a task reveals it and opens the panel; ↩ on a command runs it. With no match, the only row is "Create task “…”".
- **Picker mode** serves Add waits-on (⌘D) and Move under. Invalid targets (itself, its ancestors and descendants) are greyed, with the reason.

### Status menu and context menu

- **Status menu** (S, or click a status icon): 1 Todo, 2 In progress, 3 In review, 4 Blocked, 5 Done, 6 Dropped. Number keys, ↑↓ and ↩ pick. On a task with steps only Dropped is enabled; the rest read "Follows its subtasks" (core rule 10). Mark done (Space), ⌘B and the context menu's Mark done say the same instead of calling `set_status`.
  - **Blocked** asks "Why is it blocked?" in a one-line popover. ↩ saves; an empty reason can't be saved.
  - **Dropped** confirms first: "Drop 142 and its 3 open subtasks?". Drop is the default button, and Esc cancels.
  - **On a proposed task,** the menu shows Accept and Reject instead.
  - Every status change toasts with Undo.
- **Context menu** (right-click a row or station): Do it, Plan it, Status… S, Mark done Space, Rename ↩, Link repos… R, Add waits-on… ⌘D, Move up ⌥⌘↑, Move down ⌥⌘↓, Open page ⌘↓, Copy task number ⇧⌘C, Drop ⌘⌫. Commands that can't run are greyed, with the reason as a tooltip.

### Do it and Plan it results

Toasts are built from `LaunchReport`. Each names the task and offers what can be done next:

| Report | Toast | Actions |
|---|---|---|
| one thread started | "Started a thread for 142 in booking-web" | Open thread |
| several, all started | "Started 2 threads for 142 on `task-142-booking-flow`" | |
| research only | "Started a research thread for 142" | Open thread |
| Plan it | "Started a planning thread for 142. Its steps become subtasks when you settle the plan." | Open thread |
| some started, some failed | "Started in booking-web. booking-api failed: <message>" | Retry, or "Add to T3 and retry" when the code is `NotAProject` (`add_to_t3`, then `start` again) |
| none started, all `skipped` | "142 already has threads in booking-web and booking-api" | Start again (`again: true`) |
| none started, failed | "Couldn't start 142: <message>" | Retry |
| `warnings` not empty | the first warning is added as a second line | |

- **Retry** sends the same request. `launch` skips the repos that already started.
- When nothing started because T3 is missing or not running, the connections footer updates as well.

### Menu bar

- **Title:** the count, or nothing at 0. The template icon follows the menu bar's appearance. Its accessibility label is "Airboss, 8 waiting on you".
- **Native menu,** rebuilt on `needs:changed` (debounced 250 ms):
  - "Needs me 8", disabled.
  - Up to six items, such as "144  Agent in slot-picker asks · 10:32", each with its kind's red icon.
    - Times are **clock times**, because a native menu isn't redrawn as time passes: "10:32" today, "Mon 10:32" this week, "25 Nov" before that.
    - Choosing an item opens the main window on Needs me with that item selected.
  - A separator, then "Open Airboss ⌘0" and "Quick Add… ⌃⌥N".
  - A separator, then "Quit Airboss".
- **Empty:** "Nothing needs you", disabled, with the same items below it.

### Notifications

macOS notifications for what blocks an agent or needs me now (EM decision on the final audit). `notify.rs` decides, as pure functions; delivery goes through the `Notifications` trait (`RecordingNotifications` in tests, `UNUserNotificationCenter` in the app).

- **When:** a new Needs me item of the kinds **Question, Approval, Failed run and Agent stopped**. Never for Review, Proposed, Ready or Quiet: quiet when nothing's happening.
- **Never twice:** each key notifies at most once in a run, even if it leaves and comes back (a dismiss undone, a snooze that ends).
- **Not what was waiting at launch:** until sync has read every source once (`checked_at`) or a minute has passed, an item notifies only if it began waiting after the start (`since`); everything else is remembered as seen.
- **Not while the main window is focused**, and not with the setting off. An item that arrives then is remembered too, so it doesn't notify later. A stale item ("May be out of date") is judged once it's fresh.
- **Content:** one notification per item. The title is the task ("144 Web: slot picker"), the body is the item's one line (its headline), and the group is the task (`task-144`), so several at once go out together by task. Clicking one shows the main window on Needs me with that item selected (`nav:open`).
- **Settings:** a Notifications switch, on by default: "A banner when an agent asks something or needs an approval, a run fails, or an agent stops. Never while Airboss is in front."
- **Permission:** asked for at first run, alongside "Open at login" (and when the switch is turned on while macOS hasn't been asked). First run shows a second ticked box, "Notify me when an agent asks something or stops". If macOS has notifications off for Airboss, Settings says "macOS has notifications turned off for Airboss. Turn on Allow notifications in System Settings › Notifications › Airboss." with **Open System Settings**, rereading the state when Airboss is focused again. Outside the app bundle (a dev build) the permission is "unknown" and nothing is asked.
- **Delivery** (`macos.rs`), one current API for all of it, `UNUserNotificationCenter`:
  - Permission: `requestAuthorizationWithOptions` (alert and sound) and `getNotificationSettings`.
  - Posting: a `UNNotificationRequest` whose identifier is the Needs me key, so posting a key again replaces its notification instead of adding one; the content's `threadIdentifier` is the task (`task-144`), so macOS groups a task's notifications; the default sound.
  - Clicks: a `UNUserNotificationCenterDelegate` set in `setup`, on the main thread, before anything is posted, so a click that launches the app is heard too. `didReceiveNotificationResponse` shows the main window and sends `nav:open` with the key; `willPresentNotification` shows the banner when Airboss is active with another of its windows in front (the main window's focus already kept it from being posted).
- **What UN needs:** a bundle identifier. The built app has one (`io.github.amantibrewal310.airboss`) and is signed ad hoc; checked on this Mac with an ad-hoc signed probe bundle, where the centre answers (status "not determined"), while the same binary outside a bundle aborts with an Objective-C exception. So every call checks `NSBundle.mainBundle.bundleIdentifier` first, and a dev build run from `target/` posts nothing and reports the permission as "unknown". `app/QA.md` checks the prompt, a banner and a click by hand.

### Shortcut sheet

- ? opens it, built from the command registry in DESIGN's two-column layout. It shows the hotkey from Settings, plus ⌥⌘↑↓, ⌘Z and F6.
- Esc or ? closes it.

### Settings (⌘,)

A modal in the main window, built from the design's components:

- **Quick-add hotkey:** a recorder.
  - Registration is tried at once. If it fails: "That key is in use by another app. Pick another."
  - Keys macOS uses by default are refused, with the reason: ⌃Space, ⌃⌥Space, ⌘Space, ⌥⌘Space and ⌃⌘Space.
- **Open at login:** a switch.
- **Notifications:** a switch, on by default; see "Notifications".
- **Appearance:** System (the default), Dark or Light.
- **Check for updates automatically:** on by default, with **Check now** ([SPEC-updates](SPEC-updates.md)).
- **Command-line tool:** Install, with its path, and whether `~/.local/bin` is on `PATH`.
  - "On `PATH`" means the user's own shell's `PATH`: `$SHELL -ilc` (interactive and login, so `~/.zprofile` **and** `~/.zshrc` count, as in Terminal), reading the `PATH` after a marker, with a 3 s deadline, after which the shell is killed; it's always reaped (`login_path.rs`). It runs in the background at start and after Install, and the answer reaches the UI as `cli` in `connections:changed`. The app's own `PATH` is launchd's and never has `~/.local/bin`.
- **Agent setup:** "Copy agent note", and "Copy setup command" (`claude mcp add --scope user airboss -- ~/.local/bin/airboss mcp`). Below them, one line per agent tool:
  - **Claude Code:** "Connected", or "Not connected" with **Connect** (`connect_claude`, as on first run).
  - **Codex:** "Connected", or **Copy Codex setup**, which copies the `[mcp_servers.airboss]` table for `~/.codex/config.toml` with the CLI's absolute path and `env_vars = ["T3CODE_THREAD_ID"]` (`codex mcp add` can't set `env_vars`, which Codex needs to pass T3's thread id; SPEC-mcp). Codex isn't connected in one click.
- **Repos** (final audit should-fix 8): each repo with its colour dot, name, folder (mono, `~` for home) and remote, plus "Add repos…".
  - **Edit** opens the row in place: Name, Folder (with "Choose…", the folder picker) and Remote (blank clears it). ↩ saves, Esc cancels and returns to the row's Edit button. A refusal (a blank name, a folder that isn't there, a name or folder already used) shows under the fields. Tasks keep a repo that's renamed or moved.
  - **Remove…** asks in place: "Remove booking-web? 12 tasks lose the link." (counting tasks linked to it themselves, not parents showing it from subtasks), with "Its folder and its threads stay as they are." Cancel has focus; Esc cancels before it closes Settings. The toast says how many tasks lost the link.
- **Database:** "Back up…" opens a save dialog offering "Airboss backup <date>.db" and writes a consistent copy through core's online backup, while everything keeps running.
- The modal scrolls when its sections are taller than 80% of the window.

### First run, Add repos, T3 missing, nothing needs you

- **First run** is any start with no tasks. All tasks shows DESIGN screen 10, extended:
  - "Nothing here yet", with New task ⌘N and "Change hotkey…".
  - **"Open Airboss at login"**, a checkbox ticked by default. First run turns autostart on; unticking turns it off at once.
  - **"Notify me when an agent asks something or stops"**, ticked by default. First run asks macOS for permission to notify.
  - **Repos:** the Add repos list (below), when the screen opens with no repo added. It then stays for as long as the screen does (hands-on QA #3): repos added from it, by "Add N repos" or "Add another folder…", show ticked and greyed as "Added" (a folder that isn't a T3 project is listed after them), the other ticks are kept, and focus goes back to the list, or to "Add another folder…" once the folder picker closes.
  - **Connections:**

    | Row | OK | Off | Button |
    |---|---|---|---|
    | Agents | "Agents · Ready": the CLI is installed, Claude Code or Codex has the `airboss` server, and the time of the last agent report if any | "Agents · Not set up" (no CLI), then "Agents · Not connected": "Agents can't report to Airboss yet." | Install command-line tool, then Connect Claude Code, then Copy agent note: one button whose label follows the step, so focus stays on it |

    **Connect Claude Code** runs `connect_claude` only when it's clicked: `claude mcp add --scope user airboss -- <home>/.local/bin/airboss mcp`, argv only (no shell, so the CLI's path is absolute), with a 20 s deadline. `claude` is found like `gh`: `~/.local/bin/claude`, `~/.claude/local/claude`, `/opt/homebrew/bin/claude`, `/usr/local/bin/claude`, then the app's `PATH`, then the login shell's (`login_path.rs`); the child gets the login shell's `PATH` with `claude`'s folder first, so a Node install finds `node`. A server already registered (in `~/.claude.json`, or `claude` saying it "already exists") is success. Success refreshes the connections and toasts "Connected Claude Code". A failure (no `claude`, a non-zero exit, the deadline) toasts the reason with the first line `claude` printed, and offers **Copy setup command**. Airboss changes an agent tool's config only on this click.
    | T3 Code | "T3 Code · Found" | "T3 Code · Not found": "Install T3 Code to start agents on your tasks. Everything else works." | Check again |
    | GitHub | "GitHub · Signed in" | "GitHub · Not signed in": PRs won't sync. Run `gh auth login`. | Check again |

- **Add repos** (from first run, the sidebar's "+", and "Add repos…"):
  - **The list:** `t3_projects()` lists T3's projects from `sync.snapshot().projects`; `sync` reads them from T3's database on each pass.
    - Each row shows its folder name and path, and a checkbox, unticked.
    - Rows whose folder isn't a git repo, or is already added, are shown greyed with the reason.
    - From 8 rows, a search field above the list filters by name or path (every word; name matches first, order kept), and keeps ticks. The modal opens on it; ↓ goes into the list and ↑ from the first row comes back; Esc clears a search before it closes; "No repo matches “…”" when nothing does. A path's tooltip shows only when it's cut off, and the modal's list scrolls inside it.
  - **"Add N repos"** runs `add_repos`. Each name is the folder's name, and each remote comes from the folder's `.git/config`.
  - **"Add another folder…"** opens the folder picker.
  - **Without T3,** only the folder picker shows, with "T3 Code's projects show here once it's installed."
  - The list is one Tab stop (roving tabindex; hands-on QA #2): ↑↓ move, Home and End jump, Space ticks and ⌘↩ adds. Each row is named by its project and described by its path (and the reason it's greyed).
- **T3 missing:** as in the panel above, with "T3 Code not found" in the footer.
- **Nothing needs you:** as in Needs me. The menu bar shows no number.

## Interfaces: the `crates/services` contract

The reconciled contract (EM decision, 26 Nov 2026), word for word. The services specs build to the same text. `airboss-services` depends on `airboss-core` (for ids, `Event` and `CoreSnapshot`) and `serde`. It has no tokio, no threads and no logic beyond `Changes`.

```rust
#[derive(Clone)] pub struct Changes(Arc<(Mutex<u64>, Condvar)>);  // bump / wait_past(seen, timeout)
pub struct StoreChange { pub events: Vec<Event>, pub snapshot: Arc<CoreSnapshot> }
pub trait Service: Send + Sync { fn connections(&self) -> Vec<Connection>; fn recheck(&self); fn changes(&self) -> Changes; fn store_changed(&self, c: &StoreChange); fn woke(&self); fn shutdown(&self); }
pub trait SyncService: Service { fn snapshot(&self) -> Arc<SyncSnapshot>; fn refresh(&self, what: Refresh); fn lookup_pr(&self, pr: &PrRef) -> Result<PrView, ServiceError>; fn find_thread(&self, project: &Path, branch: &str) -> Option<ThreadKey>; fn thread_url(&self, key: &ThreadKey) -> Option<String>; }
pub trait LaunchService: Service { fn availability(&self) -> Availability; fn prepare(&self, task: TaskId, mode: Mode) -> Result<Prepared, ServiceError>; fn start(&self, req: &StartRequest) -> Result<LaunchReport, ServiceError>; fn add_to_t3(&self, repo: RepoId) -> Result<(), ServiceError>; }
pub struct StartRequest { pub task: TaskId, pub mode: Mode, pub place: Place /* TaskRepos | NoRepo(PathBuf) */, pub again: bool }
pub struct LaunchReport { pub started: Vec<StartedThread>, pub skipped: Vec<RepoId>, pub failed: Vec<(Option<RepoId>, ServiceError)>, pub branch: Option<String>, pub moved_to_doing: bool }
pub trait AttentionService: Service { fn needs_me(&self) -> Arc<NeedsList>; fn for_task(&self, task: TaskId) -> Vec<NeedsItem>; fn dismiss(&self, k: &NeedsKey) -> Result<(), ServiceError>; fn snooze(&self, k: &NeedsKey, until: Option<Timestamp>) -> Result<(), ServiceError>; fn restore(&self, k: &NeedsKey) -> Result<(), ServiceError>; }
#[derive(Clone, Serialize)] pub struct ServiceError { pub code: ErrorCode, pub message: String }
```

**How the app uses it:**

- **Timing:**
  - `store_changed`, `recheck`, `refresh` and `woke` must return at once (they queue work).
  - `needs_me`, `snapshot`, `for_task`, `availability`, `connections` and `thread_url` read memory only.
  - `prepare`, `dismiss`, `snooze`, `restore` and `add_to_t3` may block briefly. `start` and `lookup_pr` may block for seconds.
  - The app calls every one of them from a background thread with no lock held.
- **Listening:** one app thread per service loops on `changes().wait_past(seen, 15 min)`, so no wait is longer than 15 minutes, and sends a service trigger to the catch-up worker.
- **"Start in <repo>"** is `link_repo` as me, then `Place::TaskRepos`. "No repo: research only" is `Place::NoRepo(prepared.research_dir)`.
- **Open thread** is `opener` on `sync.thread_url(key)`, which gives `t3code://threads/<environment>/<thread>`. The T3 bundle registers `t3code`.
- **On wake,** `woke()` is called on each service; on quit, `shutdown()`.

**Supporting types: the fields the app reads.** The services specs own the full definitions. The app reads only these fields, and each must exist under these names:

| Type | Fields the app reads |
|---|---|
| `Connection` | `name` ("GitHub", "T3 Code"…), `ok: bool`, `state` ("Signed in"), `detail` (what it does), `fix: Option<String>` |
| `ErrorCode` | variants `NeedsChoice`, `NotAProject`, `TaskClosed`, `T3Missing`, `T3NotRunning`; the rest only pass through |
| `SyncSnapshot` | `threads_for(task) -> Vec<&ThreadView>`, `prs_for(task) -> Vec<&PrView>`, `suggested_prs(branch)`, `projects: Vec<T3Project { path, name }>`, `checked_at: Option<Timestamp>` |
| `ThreadView` | `key: ThreadKey`, `state: ThreadState`, `title`, `request: Option<Request { kind, text, at }>`, `error`, `project_path`, `last_activity_at`, `stale` |
| `ThreadState` | `Running, NeedsMe, Errored, Idle, Settled, Gone`, with `chip() -> Option<Chip>`: Running is Working; NeedsMe is WaitingOnYou; Idle is YourTurn (the agent finished its turn, the next message is mine); Errored and Settled are Stopped; Gone has no chip |
| `PrRef` / `PrView` | `PrRef::parse(&str)` (`owner/repo#n` or a URL, canonical lowercase as in `core` v3); `ref`, `url`, `title`, `state`, `draft`, `review`, `checks`, `additions`, `deletions`, `stale` |
| `Refresh` | `PrsOf(TaskId)` |
| `Availability` | `Unknown`, `Ready { version }`, `Missing { reason }` |
| `Mode` / `Prepared` | `DoIt`, `PlanIt`; `repos`, `from_subtasks`, `needs_choice`, `choices: Vec<Repo>`, `branch: Option<String>`, `research_dir: PathBuf` |
| `StartedThread` | `key: ThreadKey`, `repo: Option<RepoId>`, `title` |
| `NeedsList` / `NeedsItem` | `items`, `count`, `checked_at`, `version: u64`; `key`, `kind`, `group`, `task`, `task_title`, `since`, `headline`, `context`, `target` (`Thread(ThreadKey)`, `Pr { ref, url }`, `Task(TaskId)`), `proposed: Vec<(TaskId, String)>`, `stale` |
| `Timestamp` | UTC text in `core`'s format |

## Performance

Targets from CAPABILITIES, measured by `scripts/measure.sh` on this Mac, in a release build. The seeded database (`examples/seed.rs`) has 2,000 tasks, 5 levels, 1,000 dependencies and 40 linked threads. The machine is printed next to the numbers.

| Target | How it's met | How it's measured |
|---|---|---|
| Download under 15 MB | release profile; one Inter subset; no JS libraries; main bundle under 300 KB and quick add under 100 KB minified (revised 26 Nov 2026; see tasks/program.md) | size of the `.dmg` from `pnpm bundle --bundles dmg`, sidecar included |
| Idle memory under 130 MB (revised 26 Nov 2026 from 80 MB; see tasks/program.md) | two web views, a native tray menu, `$state.raw` rows | see below |
| Idle CPU near 0% | no polling; app threads block on channels and condvars; a minute tick only while visible; the live pulse animates only `transform` and `opacity` | `top -l 31 -s 2` over 60 s: average under 0.5% with no working chip on screen, under 1% with one, 0% with the window hidden |
| Cold start under 1 s | window created hidden; one `bootstrap`; shown on `ui_ready` | Rust logs from process start (`sysctl kern.proc` start time) to `ui_ready`; median of 10 cold starts after `purge` |
| Hotkey to quick add under 100 ms | panel created and reset ahead of time; showing it does no I/O | Rust logs from the `Pressed` callback to `quickadd_shown`, which the UI sends on the window's `focus` event; 95th percentile of 20 presses |
| 2,000 rows at 60 fps | virtual list, fixed heights, flat rows | Playwright WebKit on the mock build with 2,000 fixture rows: scroll the whole list in 3 s; 95th-percentile frame time under 16.7 ms and no task over 50 ms. Checked once by hand with Web Inspector in the real app. |
| Agent write visible under 300 ms | kqueue, debounce, `catch_up()` | `tests/catchup.rs`: a second `Store` writes; time until the patch arrives |
| Rebuild under 10 ms, write under 15 ms | one `snapshot()`, no per-row calls | `tests/perf.rs` (`#[ignore]`), 95th percentile of 200 runs |

**Memory, with both web views alive.**

- **Setup:** start the app, open quick add once with the hotkey, then hide it. Show the main window on All tasks with 2,000 rows, and wait 60 s.
- **Sum:** the `footprint` (phys_footprint) of the app process and every `com.apple.WebKit.*` process whose responsible process is the app (`launchctl procinfo <pid>`), which covers both WebContent processes, Networking and GPU.
- **Also recorded:** with the main window hidden.
- **If the visible total is over 72 MB** (90% of the target):
  - Turn on `quickadd_release_after = 10 min`. A panel hidden that long is destroyed, and the next press recreates it.
  - Record that first press's time separately. It's expected to miss 100 ms, and the trade-off is noted next to the numbers.

## Accessibility

- **Keyboard only:** every action has a key or is reachable by Tab.
  - Tab order: sidebar, then the list (one stop with roving selection), then the panel's controls. Tab from the list goes to the panel, and Shift-Tab comes back. F6 reaches toasts.
  - Menus, popovers and modals trap focus and give it back on close.
- **Focus:** the focused row or inbox item gets a 1px inset accent outline. Other controls get a 2px accent ring on `:focus-visible`.
- **Reduce Motion:** `prefers-reduced-motion` turns off every animation and transition. The live pulse becomes a static ring.
- **Increase Contrast** (`prefers-contrast: more`): hairlines go to 20% and 30%, text-3 takes text-2's value, the washes double, and the keyboard outline becomes 2px.
- **System accent:** `macos.rs` reads `NSColor.controlAccentColor` at start and on `NSSystemColorsDidChangeNotification`.
  - It replaces the accent in focus rings, the keyboard outline and text selection only.
  - Done, progress and the primary button stay Signal Indigo.
  - The Multicolor setting keeps indigo everywhere.
- **Never colour alone:** status always has its shape; live chips have a label, or a tooltip and an accessible name at narrow widths; repo dots carry the repo name; the needs-me dot is named "Needs you".
- **VoiceOver:**
  - **The list:** tree layout is `role="tree"`, with rows as `treeitem` carrying `aria-level`, `aria-expanded`, `aria-setsize` and `aria-posinset`. Flat views are a `listbox`. The list uses `aria-activedescendant`, and the active row is always rendered.
  - **Labels:** a row reads "144, Web: slot picker, In progress, agent working, needs you, booking-web, 6 minutes ago". Map stations are buttons: "203 Payment webhook, blocked, wave 2, waits on 201".
  - **Live regions:** toasts sit in a `role="status"` region.
- **Text:** nothing below 11.5px except key caps and initials.

## Testing

Tests come first, as in `core`.

- **Rust command tests** (`tests/commands.rs`):
  - **Setup:** every command, called as a plain function against `AppState` built on `Store::open_in_memory()` and fake services. The fakes implement the traits with settable snapshots and record their calls.
  - **Locking:** a fake service that asserts the store lock is free whenever it's called.
  - **Writes:** each write checks its `core` result, its returned patch, and that `store_changed` was called after the lock was released.
  - **Do it:** the in-flight guard; `NeedsChoice`, then a choice; a partial `LaunchReport`; Add to T3 and retry.
  - **Errors:** each error message in Appendix D.
- **View model tests** (`tests/viewmodel.rs`):
  - progress: leaves only, dropped left out, proposed counted as todo
  - repo union, its flag, and branch inheritance
  - `chip()` precedence
  - waits-on from `edges`
  - `diff`: upserts, removals, and order only when it changed
  - an ancestor upserted when a subtask changes
  - `seq` always increasing
  - a service bump rebuilding without a `core` read
- **Run-map tests:** DESIGN's Refunds example (waves of 3, 2, 1 and 2 with one outside blocker), a lifted phase edge, done subtasks kept, a tangled pair, and the fallback threshold.
- **Catch-up tests** (`tests/catchup.rs`), on a temporary file database:
  - A second `Store` handle (standing in for `airboss mcp`) writes, and one patch arrives within 300 ms.
  - A burst of 50 writes gives at most 3 patches.
  - Our own write gives no second patch.
  - `repo add` from another handle updates `repos`.
  - The WAL is deleted and recreated, and the watch re-arms.
  - A focus trigger with an unchanged `data_version` reads nothing.
- **Undo tests** (`tests/undo.rs`):
  - each inverse in the table
  - drop restoring the subtree's previous statuses, and leaving its proposals dropped
  - a refused undo when an agent wrote before the app caught up
  - reorder past a rejected proposal and a dropped sibling, with no entry for an end move
  - a refused undo after an agent's change
  - a deferred reject cancelled, written after 5 s, and written at shutdown
  - a reject retried while another connection holds the write lock, and one that still fails toasting with Retry
  - a Reject all whose proposals were partly accepted or dropped elsewhere: the rest is written, and the toast names the skipped ones
  - depth 20
- **IPC test** (`tests/ipc.rs`): `tauri::test::mock_builder` with the real handlers. One read and one write go through `invoke`, proving serialisation and the capability files.
- **Fixtures** (`tests/fixtures.rs`):
  - It seeds DESIGN's sample data and a 2,000-task tree.
  - It writes every read command's output.
  - It runs each scripted write in `app/tests/script.json` in order and records `{command, args, response, emitted events}`.
  - It writes the TS types.
  - CI fails if any generated file changed.
- **The mock transport:**
  - Reads come from the read fixtures.
  - Writes are matched **in order** against the recorded script by exact `command` and `args`, then answered with the recorded response and events.
  - A call it doesn't know, or one out of order, throws `UnscriptedCall`, which fails the test.
- **UI unit tests** (Vitest 5.1.2, Node): `views.ts`, `selection.ts` (every selection rule above), `search.ts`, `runMapLayout.ts`, `time.ts` (relative and clock times), `links.ts` (every row of the paste table, `t3code://` included), `keymap.ts` and `focus.ts` (every row of Appendix C's focus table), and `applyPatch`.
- **Component tests** (Vitest browser mode, `@vitest/browser-playwright` 5.1.2 on WebKit, `vitest-browser-svelte` 3.1.0):
  - `TaskRow` at each status and width
  - `VirtualList`: 2,000 rows render under 60 DOM rows, and the selection stays visible
  - `StatusMenu`: keys 1–6, the blocked reason, the drop confirmation
  - `NewTask`: ↓ to notes, Tab and Shift-Tab dropdowns, pasting a Slack link and a `t3code://` link, ⌘↩
  - `StartInMenu`, `Jump`, `NeedsInbox` (each kind's actions, local hiding), `TaskPanel` (rename, notes, the T3 missing banner, "Starting…"), `AddRepos`
  - reduced motion
- **End-to-end smoke test:** Playwright 1.65.1 on WebKit against `vite build --mode mock`, keyboard only:
  1. Open on Needs me, accept a proposal with ↩, then dismiss an item and press ⌘Z.
  2. G A, then ↓↓, Space, and S 4 with a reason, then ⌥⌘↑.
  3. ⌘K, type an id, ↩, then ⌘↓ for the page and map.
  4. C, type a title, ↓ into notes, Tab to a repo, ↩.
  5. ? for the sheet.
- **Screenshot baselines:**
  - Playwright `toHaveScreenshot` at 1440 × 900 with animations off covers the twelve web screens in `docs/design/screens/` (01–07 and 09–13), each in dark and in light: 24 baselines. They were compared once by a person with the design PNGs, and guard against regressions after that.
  - Screen 08, the native menu-bar menu, is captured with `screencapture` in both menu-bar appearances, as part of `app/QA.md`.
- **What automated tests leave unchecked** (`app/QA.md`, run before each merge to main): the real WKWebView inside Tauri, native windows and chrome, the panel over a full-screen app, the hotkey and its timing, the native menus and their enabling, `t3code://` opening the right thread, open at login with `--hidden`, single instance, focus moving between windows, the real watcher with a real `airboss mcp`, VoiceOver and the system accent. Tauri's WebDriver doesn't support macOS, which is why these are manual.

## Code style

- **Rust:**
  - Formatting and lints as in `core`: rustfmt, clippy `-D warnings`, no `unwrap` or `expect` outside tests.
  - Wrapper id types, and a short "why" doc comment on every public item.
  - No SQL in the app. Unsafe code only in `src/macos.rs`.
- **TypeScript and Svelte:**
  - TypeScript in strict mode, and Svelte 5 runes only.
  - Components get props and callbacks and never call IPC; stores and `lib/ipc/api.ts` do.
  - Styles are scoped per component, with no bare global classes. Colours, sizes and motion come only from `tokens.css`.

```ts
/** A write's patch arrives twice (returned and as an event); apply each seq once. */
applyPatch(p: Patch) {
  if (p.seq <= this.seq) return;
  const rows = new Map(this.rows);
  for (const r of p.upserts) rows.set(r.id, r);
  for (const id of p.removed) rows.delete(id);
  this.rows = rows;
  if (p.order) this.order = p.order;
  if (p.repos) this.repos = p.repos;
  this.seq = p.seq;
}
```

## Boundaries

- **Always:**
  - Act as `me`, through `Store` or a service trait.
  - Lock once per `core` call, and never call a service while holding the lock.
  - Return the patch from every write.
  - Keep task rows at 38px, and take colours from tokens.
  - Add every shortcut to the command registry, with its focus rule.
  - Run fmt, clippy, `npm run check` and every test before each commit.
- **Ask first:**
  - Adding any crate or npm package.
  - Adding a window or web view.
  - Changing `crates/services`.
  - Anything that needs network access.
  - Changing a DESIGN.md rule.
- **Never:**
  - Poll the database or a service on a timer.
  - Use FSEvents for the database.
  - Open a port, or write SQL.
  - Call `t3` or `gh` (that's `launch` and `sync`).
  - Use red for anything but needs-me.
  - Load remote content.
  - Show ports, protocols or acronyms in the main UI.

## Success criteria

- [ ] Every web screen in `docs/design/screens/` has a reviewed baseline in dark and light, and screen 08 is checked in `QA.md` (manual).
- [ ] The QA flow in program.md works keyboard only: capture, plan, Do it, an agent reports, a PR merges, Needs me clears (manual).
- [x] A write from `airboss mcp` or the CLI shows in the open window within 300 ms, with no polling and no FSEvents (`another_process_write_shows_within_300_ms`).
- [x] No service is called while the store lock is held, which the fake service checks (`Checked` and `assert_lock_free` in `tests/common/mod.rs`, used across `tests/commands.rs`).
- [ ] ⌘Z undoes dismiss, snooze, reject, status changes and moves, and refuses after someone else's change.
- [x] Every command in Appendix A has a Rust test (`tests/commands.rs`, all 44). Every view, selection and focus rule, the layout and the keymap have unit tests. The listed components have component tests. The smoke test passes, and the mock reports no unscripted calls.
- [ ] Every performance target is measured with `scripts/measure.sh` and met, memory with both web views alive, with the machine recorded.
- [ ] Without T3 Code, Do it and Plan it are off with the DESIGN wording, and everything else works. A partial Do it failure offers Retry and Add to T3 (manual: the wording and toasts are tested, e.g. `the_toast_table_for_every_report`; the screens need looking at).
- [ ] VoiceOver reads a row, a needs-me item and a map station correctly. Reduce Motion and Increase Contrast visibly apply (manual).
- [x] No AI attribution anywhere. Clippy, fmt, `svelte-check` and every test are clean (on `main`, in the final audit's runs).

## Not verified yet

- **Key-equivalent order in WKWebView:** the design assumes the page sees ⌘-keys before the native menu, so `preventDefault()` keeps a menu item from firing. The first build task checks this.
  - If the menu goes first, the Task items lose their key equivalents and rely on the page.
  - ⌘Z then goes through a custom Edit › Undo that calls the page, which runs `document.execCommand('undo')` in a field, or the app's undo otherwise.
- **`tauri-nspanel` 2.1.1 with Tauri 2.12.7:** a web-view window as a non-activating panel that takes key focus. If it doesn't work, the fallback is an always-on-top normal window, which brings the main window forward when the app is in the background.
- **kqueue on the WAL through `notify` 9.0.0's `KqueueWatcher`** (released 21 Nov 2026), including the folder watch that re-arms it. If the crate misbehaves, a 60-line `kqueue` module replaces it; that needs the `libc` crate, which Tauri already links.
- **Memory with two web views:** whether WebKit shares one WebContent process between them, and whether the 10-minute release is needed.
- **⌃⌥N** isn't a macOS default shortcut (checked against the defaults in Keyboard Shortcuts settings). Whether another app on this Mac already uses it is found when it's registered; Settings handles a clash.
- **`t3code://threads/<environment>/<thread>`** opening the thread in T3 Code, and `sync` being able to give the environment id.
- **Supporting-type names:** the table in "Interfaces" (for example `Prepared.research_dir`, `SyncSnapshot.projects`, `NeedsList.version`) follows the services specs' draft 1 and the reconciled contract. If the revised service specs name a field differently, the app follows them in `dto.rs`, and the behaviour doesn't change.
- **`core` v3 names:** `snapshot`, `CoreSnapshot.edges` as `(task, waits_on)`, `data_version` and `path` are taken from the EM's list. The app follows `core` if they land differently.
- **LaunchAgent autostart passing `--hidden`** through to the app on login.
- **Bundled `airboss` size:** the stripped binary is 5.4 MB, likely about 2 MB compressed in the `.dmg`; to be measured.

## Conflicts resolved

| Where | DESIGN / prototype says | Spec / rule says | Resolution |
|---|---|---|---|
| Quick-add hotkey | ⌃⌥Space | macOS uses ⌃⌥Space for "Select next source in Input menu"; idea.md rules out ⌥Space (Raycast), and ⌘Space, ⌃Space and ⌥⌘Space are taken too | **⌃⌥N**: N for New, the same letter as ⌘N and ⌥N in the app, and not a macOS default. It can be changed in Settings. |
| Agents connection | "Agents · Listening" | SPEC-mcp: stdio per thread, nothing listens | "Agents · Ready" (CLI installed, an agent tool has the server, last report time), "Not connected" or "Not set up" |
| New task key | C or ⌥N; the first-run screen shows ⌘N | Mac convention | C, ⌘N and ⌥N all open it; menus show ⌘N |
| Waves | Done subtasks shown in their wave; "4 waves" counts them | Rule 18: `run_order` leaves done out | The map, Waves list and Plans use display waves that include done (`runmap.rs`); `core` still decides what's ready |
| Menu-bar dropdown | A 360px custom popover with relative times ("6m") | Native feel; the memory target | A native menu with the same content and clock times |
| Every shortcut in menus | Single letters listed in menus | Bare-letter key equivalents would swallow typing | Single letters appear in item titles, not as key equivalents |
| System accent | One accent, indigo, for done, selection, focus and primary | PRODUCT: respect the system accent where it fits | The system accent drives focus and selection outlines only |
| Capability map | `launch` depends on `app`; `sync` on `core` only | One-way dependencies | The contract lives in `crates/services`; the services depend on it, and the app binary wires them |
| Ready pill | On a task that just became ready | `ready()` lists every ready task | The pill shows only with an attention Ready item; the Ready view lists every ready leaf |
| ⌘0 | "Open Airboss on Needs me" | Not a global hotkey | ⌘0 is a Window-menu item, shown in the menu-bar menu; only quick add is global |
| Toast duration | Toasts leave after 2.8 s | Undo and Retry need time to reach | Toasts with an action stay 5 s and pause on hover |
| Undo of Reject | (not designed) | Rule 6: `me` can't set `proposed` | Reject is written 5 s later; until then it can be undone. Accept isn't undoable, for the same reason. |

## Parents are containers (27 Nov 2026)

Core rule 10 (task 53): a task with steps shows what they add up to, and takes no status change but Drop, no branch and no PR.

- `RowDto.has_steps` carries it. The status menu, Space, ⌘B and the context menu's Mark done respect it; Drop still works and drops the open steps; undoing a drop puts back the steps, never the parent.
- The parent actions stay as they were: Start ready and Plan it, with Do it as one thread in the context menu and ⌘K.
- "Finish it?" is gone from Needs me, the plan card, notifications, the tray icons and the shortcut sheet.
- The design data has no parent statuses or branches: Dark mode tokens keeps one step open (in progress, 75%), and the steps agents work on carry the branches.

## Open questions

None. Launch at login and appearance were answered in tasks/program.md (25 Nov 2026).

## Changes in draft 2

1. Interfaces and Appendix A now use the reconciled std-only `crates/services` contract word for word, including six-state `ThreadState` with `chip()`.
2. Open thread uses `sync.thread_url(key)` (`t3code://threads/<environment>/<thread>`). `links.ts` and the opener allowlist accept `t3code://`.
3. Patches carry their own `seq`, and services' `Changes` bumps trigger a rebuild. Writes return before `attention` recomputes; needs fields follow in a second patch, and acted-on items hide locally.
4. Everything is built from one `core.snapshot()` per change, with no per-row `dependencies()`. The budget is restated from the measured 4.3 ms `tree()`.
5. kqueue on `-wal`, re-armed from a folder watch, plus `data_version()` on focus and wake. FSEvents is removed.
6. Focus zones with a rule per command (Appendix C). Task-menu items are disabled in fields and overlays, and quick add's focus blocks the main window's menu commands.
7. Quick add resets and focuses on every hide. `ui_ready` is sent from `onMount` with a 2 s Rust backstop, and each window's background colour follows the theme.
8. `window-state` restores size and position only and skips `quickadd`. Autostart passes `--hidden`. The hotkey acts on Pressed only, and the default is ⌃⌥N. First run has "Open Airboss at login" ticked.
9. Do it has a per-task "Starting…" state with a Rust in-flight guard, `LaunchReport` toasts with Retry and Add to T3, the Start in menu in the panel, and the main window shown on a failure from quick add.
10. Undo: a Rust stack 20 deep, cleared on quit; ⌘Z and toast Undo; refused after outside changes; reject deferred 5 s.
11. ts-rs ids are `number`, and `dto.rs` mirrors the `core` and service types.
12. Locking: one lock per `core` call, services called only after it's released, never held while `t3` or `gh` runs. Panics unwind and are caught in commands and threads.
13. Tests: the mock replays scripted writes recorded from Rust and fails on unknown calls; 24 screenshot baselines plus screen 08 by hand; selection-rule tests.
14. Selection moves to the row at the same index when the selected row leaves, and stale responses are ignored by request number.
15. Week-1 gaps: reorder with ⌥⌘↑/↓; first-run Add repos from `sync`'s T3 project list; ↓ from the quick-add title to notes.
16. Memory is measured with both web views alive, and quick add is released after 10 idle minutes if the total passes 72 MB.
17. `create_task` passes repos in `NewTask`. The native menu uses clock times. The two open questions are removed (answered in program.md). `airboss-mcp` is a sidecar, not a Cargo dependency.

## Appendix A: Commands

Writes return `Patch` (shown as → P). All `core` calls act as `me`, with one lock per call. "VM" is the view model's cached `CoreSnapshot` and rows.

| Command | Arguments | Returns | Calls |
|---|---|---|---|
| `bootstrap` | | `{seq, rows, order, repos, needs, connections, availability, settings, accent}` | `snapshot`, `repos`, `sync.snapshot`, `attention.needs_me`, every `connections`, `launch.availability` |
| `task_detail` | `id, req, history_limit` (30 or 200) | `TaskDetail` (echoes `req`) | `task`, `links`, `history_with_subtasks`; deps from VM `edges`; `sync.snapshot` threads and PRs; `attention.for_task`; `launch.prepare(id, DoIt)`; `sync.refresh(PrsOf(id))` if an open PR |
| `run_map` | `id, req` | `RunMap {nodes, edges, waves, outside, tangled}` | VM, `run_order(id)` |
| `plans` | `req` | `Vec<PlanCard>` | VM, `sync.snapshot` |
| `quickadd_data` | | `{repos, parents}` | VM |
| `t3_projects` | | `Vec<{path, name, is_git, added}>` | `sync.snapshot().projects`, a `.git` check per folder |
| `herdr_folders` | | `Vec<{path, name, is_git, added}>` | `herdr-app ls`: its spaces' folders and recent folders (SPEC-herdr), a `.git` check per folder |
| `create_task` | `{title, body?, parent?, repos[], links[], then: none / do_it / plan_it}` | `{id, patch, launch?}` | `add_task(NewTask { repos, … })`, `link` per link, then the `do_it` / `plan_it` flow |
| `rename_task` / `edit_notes` | `id, title` / `id, body?` | → P | `edit_task` |
| `set_status` | `id, status, note?` | → P, undo entry | `set_status` (blocked needs `note`) |
| `toggle_done` | `id` | → P, undo entry | `set_status` done, or the previous status when reopening |
| `accept_proposals` | `ids[]` | → P | `set_status` todo, each |
| `reject_proposals` | `ids[], reason?` | undo entry | deferred 5 s, then `set_status` dropped (writes `rejected`) |
| `move_task` | `id, parent?, position?` | → P, undo entry | `move_task` |
| `reorder` | `id, up / down` | → P, undo entry | `move_task(id, same parent, position ∓ 1)` |
| `link_repo` / `unlink_repo` | `id, repo` | → P | `link_repo` / `unlink_repo` |
| `add_repos` | `paths[]` | `{repos, patch}` | `add_repo` each; name from the folder, remote from `.git/config` |
| `edit_repo` | `repo, name, path, remote?` | → P | `edit_repo` with only what changed; a new folder must be an existing absolute folder (symlinks resolved); a blank remote clears it |
| `remove_repo` | `repo` | `{unlinked, patch}` | `remove_repo` (unlinks it from every task, done ones too) |
| `back_up_database` | | `path?` | save dialog, then `Store::backup(to, overwrite)` (not the database problem state if it fails) |
| `pick_folder` | | `path?` | dialog plugin |
| `set_branch` | `id, branch` | → P | `set_branch` |
| `set_due` | `id, due` | → P | `set_due`; blank clears it (task 59) |
| `add_dependency` / `remove_dependency` | `id, waits_on` | → P | `add_dependency` / `remove_dependency` |
| `add_link` | `id, text, title?` | → P | classify, `link`; for a PR, `sync.lookup_pr` afterwards, then `link` again with the title |
| `remove_link` | `link` | → P | `unlink` |
| `add_note` | `id, text` | → P | `add_note` |
| `do_it` | `id, place?: linked / repo(id) / no_repo, again` | `NeedsChoice {choices, research_dir}` or `Started {report, patch}` | in-flight guard; `link_repo` for `repo(id)`; `launch.prepare`; `launch.start` (no lock held) |
| `plan_it` | `id, again` | `Started {report, patch}` | in-flight guard; `launch.start(PlanIt, TaskRepos)` |
| `add_to_t3` | `repo` | | `launch.add_to_t3` |
| `reopen_and_start` | `id` | as `do_it` | `set_status` todo, then `do_it` |
| `open_thread` | `ThreadKey` | | `sync.thread_url`, then opener |
| `open_url` | `url` or `link` | | `opener.rs`'s allowlist (see "Opening links"), then the opener: browser, T3, a document's app, or Finder |
| `needs_dismiss` / `needs_snooze` | `key` / `key, until?` | undo entry | `attention.dismiss` / `attention.snooze` |
| `needs_restore` | `key` | | `attention.restore` |
| `undo` | `entry?` (none: the top) | `{patch?, message}` | the entry's inverse calls, after the `events_after` check |
| `recheck_connections` | | | `recheck()` on every service |
| `copy_text` | `task_number(id) / agent_note / setup_command / codex_setup / link(url)` | | clipboard plugin |
| `install_cli` | | `{path, on_path}` | symlink `~/.local/bin/airboss` to the sidecar |
| `connect_claude` | | | `claude mcp add --scope user airboss -- <home>/.local/bin/airboss mcp` (argv only, 20 s), then the connections refresh; already registered is success |
| `get_settings` / `set_settings` | `Settings` | `Settings` | `app-settings.json`; re-registers the hotkey; autostart plugin; window backgrounds; asks to notify when notifications are turned on and macOS hasn't been asked |
| `notification_permission` | | `granted / denied / not_asked / unknown` | `UNUserNotificationCenter` settings (inside the bundle) |
| `open_notification_settings` | | | opens System Settings › Notifications at Airboss |
| `set_folded` | `ids[], folded` | | settings, debounced |
| `set_focus_context` | `{window, zone, selection?, view}` | | enables, disables and relabels menu items |
| `ui_ready` | `window` | | shows the main window (2 s backstop) |
| `quickadd_shown` / `quickadd_hide` / `quickadd_resize` | `height` | | timing, hide (then the UI resets), size |
| `show_main` | `view?, task?, needs_key?` | | shows and focuses the main window, emits `nav:open` |
| `update_status` | | `UpdateStatus` | the in-app update's status ([SPEC-updates](SPEC-updates.md)) |
| `check_for_updates` | | `UpdateStatus` | checks for a newer release now, and toasts the outcome |
| `install_update` | | | downloads, verifies and installs the update, then restarts; progress as `update:changed` |

## Appendix B: Events

| Event | Payload | Sent when |
|---|---|---|
| `tree:patch` | `Patch` | `catch_up()` produced a change from `core` or a service |
| `needs:changed` | `{version, items, count, checked_at}` | `attention`'s `Changes` bumped |
| `connections:changed` | `{connections, availability, agent_report, cli, agents}` | a service's connections or `availability()` changed, an agent reported, or the CLI's state changed |
| `menu:action` | `{command}` | a native menu item was chosen |
| `nav:open` | `{view?, task?, needs_key?}` | the menu-bar menu, the Dock, a second launch, or a failed Do it from quick add |
| `quickadd:reset` | `{hotkey}` | the panel was hidden; the UI resets and refetches |
| `toast` | `{text, actions, undo?}` | Rust needs a toast the UI didn't ask for (a Do it result after the panel closed) |
| `appearance:changed` | `{accent?, theme}` | the system accent or appearance changed |
| `error:background` | `{source, message}` | the watcher or an app thread failed; also updates the footer |
| `db:unavailable` | `AppError` (`db_unavailable`, message, detail `"<path>: <reason>"`) | the database broke while the app ran (once); the UI shows the database problem state |
| `update:changed` | `UpdateStatus` | an update was found, or an install moved on ([SPEC-updates](SPEC-updates.md)) |

## Appendix C: Keyboard map and focus rules

Single letters, Space and arrows act only in `list`, `panel` and `sidebar` (see Keyboard, focus and menus).

| Key | List views | Needs me | Plans | Page / map |
|---|---|---|---|---|
| ↑ ↓ | move | move | move by row of cards | map: within a wave |
| → ← | open or fold; into the first child or up to the parent | | move by card | map: across waves |
| ↩ | rename | main action | open page | map: open station |
| Space | done / not done (not on a parent: its status follows its subtasks) | Mark done (Agent stopped) | | |
| S, 1–6 | status | | | |
| R | link repos | | | |
| ⌘↩ / ⇧⌘↩ | Do it / Plan it | main action | | the page's task or the selected station |
| ⌘↓ / ⌘↑ | open page | | | back |
| ⌥⌘↑ / ⌥⌘↓ | reorder (tree layouts) | | | |
| Esc | close panel | | | close page, then clear the map selection, then close the panel |
| ⌘D / ⌘L / ⌘B / ⌘O | waits-on / link / blocked / latest thread or PR | | | same |
| ⌘⌫ | drop (confirms) | dismiss | | drop |
| ⇧⌘C | copy task number | | | same |
| Tab | to the panel | to the card | | |

| Command group | `list` | `panel` | `sidebar` | `field` | `overlay` | `quickadd` focused |
|---|---|---|---|---|---|---|
| ⌘K, ⌘N, ⌘,, ⌘W | yes | yes | yes | yes | ⌘K and ⌘N replace the overlay | ⌘W closes quick add; the rest do nothing |
| ⌘Q, ⌘0 (menu) | yes | yes | yes | yes | yes | ⌘Q only |
| ⌘↩, ⇧⌘↩, ⌘D, ⌘L, ⌘B, ⌘O, ⇧⌘C, ⌘⌫, ⌘↓, ⌘↑, ⌥⌘↑↓ | yes | yes | no | no (the field's: ⌘↩ saves, ⌘⌫ and ⌘↑↓ edit text) | no | no |
| ⌘Z | app undo | app undo | app undo | text undo | no | text undo |
| Single letters (C, S, R, G…, ?), Space | yes | yes (on the selected task) | yes | no | no | no |
| ↑ ↓ → ← ↩ | the list | the focused control; ↩ and Space press buttons | the sidebar | the field | the overlay | the form (↓ title → notes) |
| Esc | close panel | to the list | to the list | cancel the edit, then leave the field | close it | hide quick add |
| F6 | toast | toast | toast | toast | no | no |
| Native Task menu items | enabled | enabled | enabled | disabled | disabled | ignored |

Global, from any app: the quick-add hotkey (⌃⌥N by default), on Pressed only.

## Appendix D: Error messages

| Case | Message |
|---|---|
| `EmptyTitle` | "A task needs a title." |
| `NotFound` | "Task 144 no longer exists." (or "That link no longer exists.") |
| `InvalidTarget` | Moving: "Tasks can't go under a proposed, done or dropped task." Dependency: "A task can't wait on its own parent or subtask." Branch: "Branch names can use letters, digits, `.`, `_`, `/` and `-`." |
| `HasSteps` (code `invalid_target`) | "This task has subtasks: its status follows them, and PRs go on the subtasks." |
| `Loop` | "That would make a loop: 145 already waits on 144." |
| `AlreadyExists` | "A repo with that name or folder is already added." |
| `NotAllowed`, `InvalidTransition` | "That change isn't allowed." (shouldn't happen as me; logged with detail) |
| `Db` | "Couldn't save. The database is busy or unreadable; details are in the log." |
| `AlreadyStarting` | (no toast; the button already says "Starting…") |
| Undo refused | "144 changed since; nothing was undone." |
| `Internal` | "Something went wrong inside Airboss; details are in the log." |
| `DbUnavailable`, at start | "Airboss can't open its database", with `"<path>: <reason>"` as detail |
| `DbTooNew` | "A newer Airboss has updated this database. Install it, then open Airboss again." (headed "Update Airboss") |
| `DbUnavailable`, while running | "Quit Airboss (⌘Q) and open it again. Changes made since the problem began may not have been saved. Troubleshooting has more." |
| `UrlNotAllowed` | "Airboss only opens https links, files and T3 threads." |
| `AppUnverified`, a T3 link | "Couldn't verify the app that opens T3 links. Open the thread from T3 Code." (with Copy link) |
| `AppUnverified`, a web link | "Couldn't verify your browser. Copy the link instead." (with Copy link) |
| A FIFO, socket or device | "That isn't a file or folder Airboss can show." |
| A file link whose file is gone | "That file no longer exists." (or "Airboss can't reach that file.") |
| A `ServiceError` | its own `message`, from the service's spec |
