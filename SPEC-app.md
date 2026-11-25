# Spec: app

Module `app` from [CAPABILITIES.md](CAPABILITIES.md). State: **draft 1, waiting for review**.

It builds on [`core`](SPEC-core.md) v2 for every rule and on [DESIGN.md](DESIGN.md) for every pixel. `app` adds no rules about tasks. It shows them, and it hosts `sync`, `launch` and `attention` in one process.

## Objective

A native-feeling Mac app that answers two questions in a glance: what is waiting on me, and where does every task stand. It also makes the two main actions one keystroke away: capture a task, and hand it to an agent.

**User stories** (one per design screen)

- As me, I open the app and land on **Needs me**: questions, reviews, failed runs, proposals, newly ready tasks, parents to finish and quiet work, each with its main action on ↩.
- As me, I scan **All tasks** as a tree of 2,000 tasks. Each row shows status, progress, live agent state, repos and when it last changed, and I can move through it without a mouse.
- As me, I select a task and see its **panel**: what needs me, its properties, notes, threads and activity. ⌘↓ opens it as a **full page**, with the run-order **map** for a parent.
- As me, I see every **plan** as a card with its progress, current wave and what's next.
- As me, I press ⌃⌥Space in any app, type a title, and the task exists (**quick add**). ⌘↩ also starts it.
- As me, I press ⌘K to **jump** to any task or run any command.
- As me, I change status with S then 1–6, or Space for done, and right-click for everything else.
- As me, I glance at the **menu bar** for the "waiting on you" count and open an item from there.
- As me, I press ? to see **every shortcut**.
- As me, on **first run** I see what is set up and what isn't. Without T3 Code, Do it and Plan it switch off and everything else works. When **nothing needs me**, the app says so and offers the ready tasks.

## Decisions (settled in this spec)

1. **Rust owns the view model; the UI renders it.** The Rust side keeps a snapshot of every row (progress, repo union, live state, needs-me flag) and sends **patches**. Svelte never recomputes tree facts. One code path, tested in Rust, serves both our own writes and other processes' writes.
2. **One catch-up path.** `Store::subscribe()`, the file watcher, window focus and wake from sleep all call `catch_up()`, which reads `events_after(cursor)` and, only if something changed, rebuilds the snapshot and emits one patch. Every write command runs `catch_up()` before returning and returns the patch, so the UI updates without waiting for the watcher.
3. **The service contract lives in a types-only crate,** `crates/services` (`airboss-services`). `sync`, `launch` and `attention` implement its traits. `app/src-tauri` is the only place that builds them and wires them together. All trait methods are synchronous; the app calls them off the main thread.
4. **Three surfaces, two web views.** The main window and the quick-add panel are web views. The menu-bar dropdown is a **native menu**, so it costs no memory and feels native. The quick-add panel is created hidden at start-up and is a non-activating `NSPanel`, so the hotkey only has to show it.
5. **The mock backend serves real Rust output.** The browser build of the UI (for component and end-to-end tests) reads fixtures that a Rust test generates from a seeded `Store`, and TypeScript types are generated from the Rust DTOs. The UI and the mock can't drift from the real contract.

## Tech stack

Versions are the latest stable on crates.io and npm on 25 Nov 2026, pinned exactly (`=` in Cargo, no `^` in `package.json`).

| Rust crate | Version | Why |
|---|---|---|
| `tauri` | 2.12.7, features `tray-icon`, `macos-private-api`, `image-png` | The shell. Private API is needed for the transparent quick-add panel. |
| `tauri-build` | 2.7.6 | Build script |
| `tauri-plugin-global-shortcut` | 2.4.1 | ⌃⌥Space from any app |
| `tauri-plugin-autostart` | 2.7.0 | Launch at login |
| `tauri-plugin-single-instance` | 2.5.5 | A second launch focuses the first |
| `tauri-plugin-window-state` | 2.5.0 | Main window size and position |
| `tauri-plugin-opener` | 2.7.3 | Open PRs, docs and Slack in their apps |
| `tauri-plugin-clipboard-manager` | 2.5.0 | Copy task number, agent note, setup command |
| `tauri-plugin-dialog` | 2.8.4 | Folder picker for "Add repo…" |
| `tauri-plugin-log` | 2.10.2 | `~/Library/Logs/Airboss/app.log` |
| `tauri-nspanel` | 2.1.1 | Quick add as a non-activating panel over full-screen apps |
| `notify` | 9.0.0 | FSEvents file watcher |
| `tokio` | 1.54.1 | Comes with Tauri; `sync::watch` channels and timers |
| `serde` 1.0.229, `serde_json` 1.0.154, `thiserror` 2.0.21 | | As in `core` |
| `ts-rs` | 12.0.1 | TypeScript types from the DTOs |
| `airboss-core`, `airboss-services`, `airboss-sync`, `airboss-launch`, `airboss-attention`, `airboss-mcp` (binary only) | path | |

| npm package | Version | Use |
|---|---|---|
| `svelte` | 5.59.2 | UI, runes mode only |
| `vite` | 8.3.8 | Build, two entry pages |
| `@sveltejs/vite-plugin-svelte` | 7.3.2 | |
| `@tauri-apps/api` | 2.12.5 | `invoke`, `listen`, window calls. The only runtime dependency besides Svelte. |
| `@tauri-apps/cli` | 2.12.6 | `tauri dev` / `tauri build` |
| `typescript` | 6.0.3 | 7.0 isn't supported by `svelte-check` yet |
| `svelte-check` | 4.7.8 | Type checks |
| `vitest`, `@vitest/browser-playwright` | 5.1.2 | Unit tests in Node, component tests in WebKit |
| `vitest-browser-svelte` | 3.1.0 | Rendering components in Vitest browser mode |
| `@playwright/test`, `playwright` | 1.65.1 | WebKit engine for component and smoke tests |

- **Node 24** is needed to build and test only. Nothing Node runs in the shipped app.
- **No component library, no CSS framework, no state library, no virtual-list library.**
- **Inter** ships as one local variable `woff2`, Latin subset (about 110 KB). No font is fetched from the network.
- **Release profile** (workspace `Cargo.toml`): `lto = "fat"`, `codegen-units = 1`, `opt-level = "s"`, `strip = true`, `panic = "abort"`.
- **Apple silicon only** (`aarch64-apple-darwin`), signed ad hoc. No notarisation for a personal build.

## Commands

```
cd app && npm ci
npm run tauri dev                          # the app; set AIRBOSS_DB to use a throwaway database
npm run dev:mock                           # the UI in a browser, mock backend, http://localhost:5173
npm run tauri build -- --bundles dmg       # release .dmg (builds the `airboss` CLI first)
npm run check                              # svelte-check and tsc
npm test                                   # Vitest: unit (Node) and component (WebKit)
npm run test:e2e                           # Playwright smoke test against the mock build
cargo test -p airboss-app -p airboss-services
cargo test -p airboss-app -- --ignored perf             # snapshot and patch timings
cargo test -p airboss-app --test fixtures               # regenerates mock fixtures and TS types
cargo run -p airboss-app --example seed -- --tasks 2000 --db /tmp/ab.db
cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check
scripts/measure.sh                         # size, memory, CPU, start-up, hotkey; prints the machine
```

## Project structure

```
crates/services/                 airboss-services: the traits and shared types in "Interfaces", no logic
app/
  package.json  vite.config.ts  vitest.config.ts  playwright.config.ts  svelte.config.js  tsconfig.json
  index.html                     main window entry
  quickadd.html                  quick-add entry (its own small bundle)
  src/
    main.ts  quickadd.ts
    styles/tokens.css            tokens copied from docs/design/airboss.css, dark and light
    styles/base.css              reset, fonts, focus ring, reduced motion, increased contrast
    lib/ipc/                     transport.ts (interface), tauri.ts, mock.ts, api.ts (one typed function per command)
    lib/types/                   generated by ts-rs; never edited by hand
    lib/state/                   tasks, needs, connections, ui, detail, toasts, settings (*.svelte.ts)
    lib/logic/                   views.ts, search.ts, runMapLayout.ts, time.ts, links.ts, keymap.ts, commands.ts
    lib/components/              StatusIcon, ProgressRing, StackedBar, Pill, Tag, LiveChip, Avatar, Kbd,
                                 Button, Menu, Popover, Modal, Toast, VirtualList
    lib/mock/fixtures/           generated by the Rust fixtures test
    windows/main/                App, Sidebar, Header, TaskList, TaskRow, TaskPanel, TaskPage, RunMap,
                                 WavesList, NeedsInbox, NeedsCard, PlansGrid, PlanCard, Jump, NewTask,
                                 StatusMenu, ContextMenu, ShortcutSheet, Settings, FirstRun, Connections
    windows/quickadd/QuickAdd.svelte
  tests/unit/  tests/component/  tests/e2e/
  src-tauri/
    Cargo.toml                   airboss-app
    tauri.conf.json  capabilities/main.json  capabilities/quickadd.json
    icons/  tray/                app icon, menu-bar template icon, one red icon per Needs me kind
    resources/agent-note.md      the note "Copy agent note" puts on the clipboard
    src/main.rs                  builder, plugins, windows, run loop
    src/state.rs                 AppState: store, services, snapshot, settings
    src/snapshot.rs              rows, progress, repo union, diff → Patch
    src/runmap.rs                display waves and lifted edges for the map
    src/watcher.rs               file watcher and catch_up()
    src/commands/                tasks.rs, detail.rs, needs.rs, launch.rs, app.rs
    src/tray.rs  src/menu.rs  src/quickadd.rs  src/settings.rs
    src/dto.rs                   every type the UI sees, with #[derive(Serialize, TS)]
    src/errors.rs                AppError and the message table
    examples/seed.rs
    tests/commands.rs  tests/snapshot.rs  tests/watcher.rs  tests/fixtures.rs  tests/ipc.rs  tests/perf.rs
scripts/measure.sh
```

Both new crates join the workspace `members`.

## Architecture

### Processes and windows

One process, `Airboss.app`, with a Dock icon (activation policy Regular). It holds:

| Part | What it is |
|---|---|
| `AppState` | `Arc<Mutex<Store>>` (one `core` connection), the three services as `Arc<dyn …Service>`, the current `Snapshot`, the event cursor and the settings |
| Background tasks | the file watcher, the catch-up loop, a listener per service `changes()` channel, and the services' own tasks |
| Main window `main` | 1100 × 720 by default, minimum 900 × 560. Overlay title bar with hidden title; traffic lights at x 18, y 20; the sidebar top and the 44px header are drag regions. Created hidden and shown after the UI's first paint (`ui_ready`), so there's no white flash. |
| Quick-add panel `quickadd` | 680px wide, frameless, transparent, always on top, on every Space and over full-screen apps. Created hidden at start-up; height follows its content. A non-activating `NSPanel`, so showing it doesn't bring the main window forward. |
| Menu-bar item | a template icon plus the Needs me count as its title (no number at 0), with a native menu |
| Bundled CLI | `airboss` (from `crates/mcp`) as a Tauri sidecar in `Contents/MacOS/`. "Install command-line tool" links it to `~/.local/bin/airboss`. |

- **Closing** the main window (⌘W or the red button) hides it; the app keeps running in the menu bar. ⌘Q quits. Clicking the Dock icon, ⌘0 in the menu-bar menu, or launching again (single instance) shows it.
- **Launch at login** is turned on at first run, with a switch in Settings (see Open questions).
- **No network.** The CSP is `default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'`. `withGlobalTauri` is off. Each window's capability file allows only the commands it uses.
- **Native menu bar:** Airboss (About, Settings… ⌘,, Install Command-Line Tool, Quit ⌘Q), File (New Task ⌘N, Quick Add ⌃⌥Space, Close Window ⌘W), Edit (the standard items, which text fields need), Task, View, Window, Help. Task and View list every command; items whose key is a single letter show it in their title rather than as a key equivalent, because a bare-letter key equivalent would swallow typing.

### The command layer

- Every command is an `async` Tauri command that runs its work in `spawn_blocking`, holds the store lock only for its `core` calls, and returns `Result<T, AppError>`. Commands never run on the main thread.
- Each command body is a plain function `fn(&AppState, Args) -> Result<T, AppError>` in `src/commands/`, so tests call it without Tauri.
- **Write commands return `Patch`** (or a struct containing it). They run `catch_up()` after their `core` calls, inside the same lock, so the returned patch already holds every derived change (parent progress, waiting flags, the Ready pill).
- `core` calls are made as `Actor::Me`. The app never acts as anyone else.
- The full list, each mapped to its `core`, `sync`, `launch` or `attention` call, is in [Appendix A](#appendix-a-commands). Events are in [Appendix B](#appendix-b-events).
- DTOs live in `dto.rs` with `#[derive(Serialize, Deserialize, TS)]`. `cargo test --test fixtures` writes `app/src/lib/types/*.ts`; CI fails if they differ from the committed files.

### Snapshot and patches

`Snapshot` is the whole list's view model, rebuilt in Rust from:

- `tree()` (v2: with `last_event_at`, `last_actor`, `subtree_last_event_at`)
- `repos()` and `ready()`
- `dependencies(id)` for each node that is `waiting` (for the "waits on 144, 145" pill; usually a few dozen calls)
- `sync.threads()` for live chips
- `attention.needs_me()` for the needs-me dot, the Ready pill and the Quiet pill

Each row is a `RowDto`:

| Field | Meaning |
|---|---|
| `id`, `parent`, `depth`, `position`, `title`, `status`, `origin` | From `core` |
| `waiting`, `waits_on` | Open blockers' ids, only when waiting |
| `blocked_reason` | Shown on hover of the Blocked pill and in the panel |
| `has_children`, `progress` | `{done, total, in_review, doing, blocked}` over leaf subtasks at any depth. Dropped left out, proposed counted as todo (DESIGN "Progress"). `None` for a leaf. |
| `repos`, `repos_from_subtasks` | Own repos; for a parent with none, the union of its subtasks' repos, flagged |
| `branch` | `tasks.branch`, or the nearest ancestor's |
| `ready` | In `core::ready()` and a leaf |
| `ready_pill`, `quiet_days`, `needs` | From attention: a Ready item, a Quiet item, any item |
| `live` | Strongest thread state of its linked threads: waiting on you, then running, then stopped. Only while the task is open. |
| `last_at`, `last_actor` | `subtree_last_event_at` for a parent, `last_event_at` for a leaf, and who did it |

`diff(old, new)` gives `Patch { cursor, upserts: Vec<RowDto>, removed: Vec<TaskId>, order: Option<Vec<TaskId>>, repos: Option<Vec<RepoDto>>, created: Vec<TaskId> }`. `order` (ids in tree order) is sent only when it changed. `created` drives the new-row highlight. The UI drops any patch whose cursor is not newer than its own, so a returned patch and the same patch arriving as an event apply once.

Repo colours are fixed by id: `repo-((id − 1) mod 4) + 1`. A repo keeps its colour for ever, as the Owned Colour Rule asks. A fifth repo shares repo-1's colour.

### Seeing other processes' writes

- **Watch** `~/Library/Application Support/Airboss/` (or the folder of `AIRBOSS_DB`) with `notify` (FSEvents), non-recursive, keeping only `airboss.db` and `airboss.db-wal`.
- **Debounce:** 30 ms after the last change, and never more than 150 ms after the first. Then `catch_up()`.
- **`catch_up()`** locks the store, reads `events_after(cursor)` and re-reads `repos()` (the CLI's `repo add` writes no event). If neither changed, it stops: our own writes and WAL checkpoints cost one indexed query. Otherwise it rebuilds the snapshot, emits `tree:patch`, and passes the new events to `sync.store_changed` and `attention.store_changed`.
- **Cursor on start:** read the newest event id before calling `tree()`. A write landing in between is applied twice, which is harmless because patches are diffs.
- **Insurance, not polling:** `catch_up()` also runs when the main window gains focus and when the Mac wakes. No timer reads the database.
- **The file disappears or is replaced:** the footer goes orange with "Database moved", and the popover says to quit and reopen.
- **Target:** an agent's `update_task` shows in an open window within 300 ms.

### UI state (Svelte 5 runes)

Stores are classes in `lib/state/*.svelte.ts`, one exported instance each, with `$state` fields and `$derived` getters. Components read them; only store methods write them.

| Store | Holds |
|---|---|
| `tasks` | `rows: Map<TaskId, RowDto>` and `order: TaskId[]` as `$state.raw` (no deep proxies over 2,000 objects; a patch swaps in a new `Map` and replaces only changed row objects), `repos`, `cursor`, `fresh` (ids to highlight), `applyPatch()` |
| `needs` | items, count, `lastChecked`, the selected item |
| `connections` | the three services' states, and whether `launch` is available |
| `ui` | view (`needs`, `ready`, `plans`, `progress`, `all`, `repo:<id>`, `parent:<id>`), layout (`tree` or `status`), selection, map selection, folded ids, panel open, page open, focus zone (`sidebar`, `list`, `panel`), open overlay, rename target |
| `detail` | the `TaskDetail` for the selection, refetched when a patch upserts that id (a parent's id is upserted whenever a subtask changes, because its `last_at` moves) |
| `toasts`, `settings` | |

- **Visible rows** come from pure functions in `logic/views.ts`, `(rows, order, view, folded, layout) → VisibleItem[]`, wrapped in one `$derived`. They are unit-tested without Svelte.
- **Folds** are saved in settings, debounced 1 s. Parents start folded, except the ancestors of whatever is selected.
- **Times** ("6m", "2d") read a `now` value that ticks once a minute, only while the window is visible.
- The quick-add window has its own tiny state: the form, plus `repos` and open parents fetched with `quickadd_data` each time it opens.

### Virtualised list

`VirtualList` is our own component, about 150 lines:

- Items have two fixed heights: task rows 38px, and group headers 34px (By status layout only). Offsets are a prefix-sum array rebuilt when the visible list changes; the first visible item is a binary search.
- Rows render inside a `contain: strict` scroller, absolutely positioned with `transform: translateY`, keyed by task id, with 8 rows of overscan.
- Group headers stick to the top with one extra header element that mirrors the current group, not CSS `sticky` inside the absolute layer.
- Moving the selection scrolls just enough to keep the row visible (`nearest`), with no smooth scrolling.
- `TaskRow` takes one `RowDto` and the selection and focus flags. It has no `$effect`, no store subscriptions and no per-row listeners; the list handles clicks by delegation.
- Narrow widths follow DESIGN's two steps through a container query on the list.

### Keyboard and menus

- `logic/commands.ts` is the one registry: id, label, keys, the contexts it works in, and `run()`. The keymap, the Task and View menus, the context menu, ⌘K's command list and the shortcut sheet are all built from it.
- One `keydown` listener on `window`:
  - Single-letter and Space keys are ignored while focus is in an input, a textarea or rename.
  - `G` waits 900 ms for its second key.
  - Every handled key calls `preventDefault()`.
- Native menu items send `menu:action {command}` to the same `run()`. If a key and its menu item both fire for one press, the second call within 50 ms is ignored (see Not verified yet).
- Open overlays (menus, Jump, New task, the sheet) take the keys first. Esc closes the top one.

### Errors

`AppError { code, message, detail? }`. `message` is one plain sentence for the UI; `detail` goes to the log. The table is in [Appendix D](#appendix-d-error-messages).

| Where it fails | How it shows |
|---|---|
| An action I took (status, rename, link…) | A toast with an orange warning icon and the message. The row keeps its real state, because the UI only ever shows what Rust returned. |
| A form (New task, link, blocked reason) | Inline under the field. The confirm button stays enabled, except Create with an empty title. |
| A service (`gh` signed out, T3 missing, watcher failed) | The connections footer turns orange and names it; the popover gives the fix. Never a toast loop. |
| The database won't open at start | A full-window state: "Airboss can't open its database", the path, the reason, and buttons "Show in Finder" and "Try again". |
| A Rust panic | `panic = "abort"`: the app quits and the log keeps the message. Commands never `unwrap`. |

Red is never used for errors (The Red Is Mine Rule).

## Screens

Every screen keeps DESIGN.md's layout, sizes and motion. This section adds the data, the behaviour and the states. The keyboard map per context is in [Appendix C](#appendix-c-keyboard-map).

### Sidebar and connections footer

- **Data:** `tasks` (counts), `needs.count`, `connections`.
- **Counts:** Needs me is the attention count as a red badge, hidden at 0. Ready to start: ready leaves. Plans: plan cards. In progress: `doing` + `in_review`. All tasks: open tasks.
- **Parents:** open top-level tasks with subtasks, in position order, with status icon, percentage and ring. Clicking one opens the per-parent view.
- **Repos:** every repo with its colour dot. Clicking one opens the per-repo view. A "+" on the section header runs Add repo.
- **Footer:**
  - "All connected" in text-3 with a green dot, or the first problem in orange, or "N connections need setup".
  - Clicking opens the 300px popover: one line per service with its state, what it does or how to fix it, and a "Check again" button (`recheck_connections`).
- The search and pencil icons at the top open Jump and New task.

### List views: Needs me aside, every view is a list

| View | Rows | Layout |
|---|---|---|
| Ready to start | ready leaves, tree order; time shows when it became ready (`since`) | flat |
| In progress | `doing` and `in_review`, tree order | flat |
| All tasks | the whole tree minus dropped; done rows dimmed | Tree or By status |
| Per repo | tasks whose own or inherited repos include it | flat, tree order |
| Per parent | the parent's subtree, its children at depth 0 | tree |

- **Header:** title, row count, the Tree / By status switch (All tasks only), Jump ⌘K and New task.
- **By status** groups, in order: In progress, In review, Blocked, Todo, Waiting, Proposed, Done. The header shows the group's status icon, name and count.
- **Rows:** exactly DESIGN "Task rows". Click selects and opens the panel; double-click opens the page; right-click opens the context menu; the status icon opens the status menu; the arrow folds.
- **Empty view:** "Nothing here" with "No tasks match this view." Ready's empty state adds "Tasks show here when nothing they wait on is open."
- **Loading:** none. Data is in memory before the window shows.
- **New rows** get the indigo wash and fade it out over 600 ms. A status change pops the icon.

### Task panel (480px, 380px when the window is 980px or less)

- **Data:** `task_detail(id)`, which returns the task, its links with PR info and live thread state, `history_with_subtasks(id, 30)`, `dependencies(id)`, its display waves if it has subtasks, its Needs me items, the branch and `launch.availability()`.
- **Header:** the breadcrumb ("Booking flow › 144"; clicking the parent selects it), Plan it (⇧⌘↩), Do it (primary, ⌘↩), open page (⌘↓) and close (Esc).
- **T3 missing banner:** when `launch` isn't available, an orange-iconed line under the header: "Do it and Plan it need T3 Code. Install it, then choose Check again." Do it and Plan it stay focusable but read as disabled (`aria-disabled`); pressing them repeats the line as a toast.
- **Title:** click or ↩ to rename in place. ↩ or blur saves (`rename_task`); Esc cancels.
- **Needs me callouts:** one red-washed callout per item, with the item's main action and Dismiss. These are the same items Needs me shows.
- **Properties:**
  - Status opens the status menu.
  - Repos are tags, plus "Link repo", which opens the repo checklist; its last line is "Add repo…". "· from subtasks" shows when the repos are inherited.
  - Branch (mono): shown when set, or as the name Do it would use, in text-3. Click to edit (`set_branch`).
  - Parent opens the Jump picker in "Move under…" mode (`move_task`, last position).
  - Waits on lists every dependency, finished ones too, each with its status icon. "Nothing · ⌘D to add". Hovering a tag shows ✕ (`remove_dependency`).
  - Blocks: the same, read-only.
  - Links: tags titled by link title, or by kind and ref. Click opens the link; ✕ unlinks. "None · ⌘L to add".
- **Notes:** the body in Notes type. Click to edit in an auto-growing textarea; ⌘↩ or blur saves (`edit_notes`), Esc cancels. Empty notes read "Add notes…".
- **Subtasks:** the Waves list (DESIGN "Waves list") when the task has subtasks. A row click selects that subtask.
- **Threads:** one 8px-rounded row per linked thread: live chip (Running, Waiting, Stopped), name, summary and "Open ↗" (`open_thread`).
- **Activity:** one line per event:
  - The actor's avatar and name: You, Agent (or "Agent in <thread name>" when `sync` knows the name), GitHub, T3, Plan, Launch.
  - A verb phrase per event kind.
  - For a subtask's event, its id first: "143 moved to Done".
  - "Show more" refetches with a limit of 200.
- **Note input:** "Leave a note for the agent…". ↩ adds a note (`add_note`); ⇧↩ adds a new line.
- **Loading:** the previous task stays on screen until the new detail arrives. Only if that takes over 150 ms do the sections show 3 grey skeleton lines.
- **Error:** if the task was deleted elsewhere, the panel shows "Task 144 no longer exists" and closes on the next key.

### Full task page

- ⌘↓ or double-click opens it; ⌘↑ or Esc goes back to the list with the same selection. It shows the same data as the panel: a centred document column (max 860px) and a 280px properties rail.
- The rail adds Progress: the bar, "13% · 1 of 8".
- **Subtasks:**
  - "N waves", the 8px stacked bar with its legend.
  - The Waves / Map switch, opening on Map.
  - The line "Nothing starts by itself. Select a ready task and press ⌘↩."
- A leaf task's page is the panel's content in the document layout, with no subtasks section.

### Run-order map

- **Data:** `run_map(id)`, computed in `runmap.rs`:
  - **Nodes** are the parent's direct subtasks that are neither dropped nor proposed. Done ones are included.
  - **Edges** are dependencies lifted to those subtasks (any descendant's dependency counts for its top-level subtask under the parent, as in rule 18). Each edge is marked done when its blocker is done.
  - **Display waves:** Kahn layering over all the nodes, done ones included, in sibling order.
  - **Outside blockers:** from `run_order(id).waiting_outside`. Such a task sits in the column after its latest inside blocker, or in wave 1.
  - **Tangled tasks** (`run_order(id).tangled`) are listed under the map, with "These wait on each other in a loop."
- **Drawing:** `runMapLayout.ts` turns the graph and the width into station positions and SVG paths, following DESIGN's curve and wave-skip rules. It is a pure function with unit tests.
  - Columns are 150px or wider. Stations in a wave are 80px apart.
  - The station state line reads ready, blocked, "waits on 202, 204" or "waits outside: 128", then the station's repo dots.
- **Controls:** hover traces a station; click selects it (indigo wash) so ⌘↩ starts that subtask; double-click opens it. With the map focused, ←→ move between waves, ↑↓ within one, ↩ opens and Esc clears.
- **Fallback:** above 40 subtasks or 8 waves, the page opens on the Waves list and the Map tab says "Map shows up to 40 subtasks".

### Needs me

- **Data:** `needs` (from `attention.needs_me()`, pushed with `needs:changed`), and `task_detail` for the selected item's task.
- **Inbox:** the 420px list, grouped in DESIGN's fixed order. Questions holds both Question and Approval items. ↑↓ move, and the selection starts on the first item.
- **Detail:** the card (kind, time, task, the question at 17px, actions), then the task's properties, notes and activity, as in the panel.
- **Actions** (the main action is ↩ or ⌘↩, Dismiss is ⌘⌫):

  | Kind | Main action | Others |
  |---|---|---|
  | Question, Approval, Failed run | Open thread (`open_thread`) | Dismiss |
  | Review | Open PR (`open_url`) | Dismiss |
  | Proposed | Accept all (`accept_proposals`) | Accept, Edit (inline rename, then accept) or Reject each; Reject all |
  | Ready | Do it (`do_it`) | Dismiss |
  | Finish it? | Mark done (`set_status` done) | Keep open (`needs_dismiss`) |
  | Quiet | Open (selects the task in All tasks) | Snooze 3 days (`needs_snooze`) |

- **Clearing:** an item resolved by an action, or by an agent elsewhere, slides 10px left and fades over 180 ms. The selection moves to the next item.
- **Header:** "Needs me N", and "checked 10:44" from `last_checked`.
- **Empty:** DESIGN "Nothing needs you": a check, "Checked 10:44 · 10 in progress · 5 ready to start", and "Show ready tasks", which opens Ready.

### Plans

- **Data:** `plans()` returns one `PlanCard` per open top-level parent:
  - progress
  - display waves: the count, the current wave (the lowest with an open task) and one dot per wave
  - Next up: up to two ready subtasks, or "Finish it?" at 100%, or "Nothing ready"
  - the number of working agents (subtasks with a running thread) and of blocked subtasks
  - the repo union and `last_at`

  It is refetched when a patch upserts any parent.
- **Header:** "Plans N", "35% of all planned work done · 6 of 17", and "Plan something", which opens New task in plan mode (↩ creates it, then runs Plan it).
- **Grid:** cards at least 340px wide, 12px apart. ←→↑↓ move by card and by row; ↩ opens the page.
- **Empty:** "No plans yet". "Plan it on any task turns its plan into subtasks here." and the Plan something button.

### New task (in the app) and quick add (from anywhere)

One `NewTask.svelte` component serves both. In the app it is a modal on the scrim; in the panel it is the whole window.

- **Opening:**
  - In the app: C, ⌘N or ⌥N. In a per-parent view the parent is preset; in a per-repo view the repo is.
  - From anywhere: ⌃⌥Space. Rust positions the panel at 22% from the top of the screen with the mouse, shows it, focuses it and emits `quickadd:open`. The UI focuses the title. No data fetch blocks showing it: repos and parents arrive with `quickadd_data` afterwards.
- **Fields:**
  - The title (18px).
  - Notes ("Add notes, or paste a Slack link…").
  - The property pills: status (Todo, fixed), "Repos · optional" (a multi-select checklist; ↩ or click toggles a repo; the last line is "Add repo…" in the app), and Parent (a filterable list of open tasks plus "Top level").
  - Tab opens repos and Shift-Tab opens parent; ↑↓ move and Esc closes the dropdown.
- **Pasting** a link into the title or notes attaches it as a link pill (with ✕) and keeps the rest of the text. Links are classified by `links.ts`: GitHub PR, GitHub issue, Slack permalink, `t3://` thread, a file path as a doc, anything else as a doc with its URL.
- **↩ Create task:** `create_task`, which runs `add_task`, then `link_repo` and `link` for each, under one lock. The panel hides at once, and in the app the new row is highlighted and selected.
- **⌘↩ Create and Do it:** the same, then `do_it`.
- **Esc** closes. In the panel, a draft survives a hide by blur for 10 minutes; Esc clears it.
- **Errors:**
  - An empty title disables Create.
  - If a link step fails after the task exists, the task stays and a toast says "Created 153, but couldn't link booking-api".
- The header line shows "⌃⌥Space from any app", or the hotkey chosen in Settings.

### ⌘K Jump

- **Data:** in memory. `search.ts` fuzzy-matches id and title over `tasks.rows` (dropped left out), plus the command registry.
- **Results:** with no query, the 5 most recently changed open tasks and 4 commands; with a query, 8 tasks and 5 commands. A number puts that id first.
- ↩ on a task reveals it: switch to All tasks if needed, unfold its ancestors, select it and open the panel. ↩ on a command runs it.
- With no match, the only row is "Create task “…”", and ↩ creates it.
- **Picker mode** reuses the palette for Add waits-on (⌘D) and Move under. Invalid targets (itself, its ancestors and descendants) are shown greyed with the reason.

### Status menu and context menu

- **Status menu** (S, or click a status icon): 1 Todo, 2 In progress, 3 In review, 4 Blocked, 5 Done, 6 Dropped. Number keys, ↑↓ and ↩ pick.
  - **Blocked** opens a one-line popover, "Why is it blocked?". ↩ saves, and an empty reason can't be saved.
  - **Dropped** opens a confirm dialog: "Drop 142 and its 3 open subtasks?". Drop is the default button; Esc cancels.
  - On a proposed task the menu shows Accept and Reject instead.
- **Context menu** (right-click a row or a map station): Do it, Plan it, Status… S, Mark done Space, Rename ↩, Link repos… R, Add waits-on… ⌘D, Open page ⌘↓, Copy task number ⇧⌘C, Drop ⌘⌫. Commands that can't run show greyed, with the reason as a tooltip.
- **Do it with no repo** opens a small menu: "Start in <repo>" for each repo (this links the repo as me, then starts) and "No repo: research only".

### Menu bar

- **Title:** the count, or nothing at 0. The template icon follows the menu-bar appearance.
- **Native menu**, rebuilt on `needs:changed` (debounced 250 ms):
  - "Needs me 8" (disabled).
  - Up to six items, "144  Agent in slot-picker asks · 6m", each with its kind's red icon. Choosing one opens the main window on Needs me with that item selected.
  - A separator, then "Open Airboss ⌘0" and "Quick Add… ⌃⌥Space".
  - A separator, then "Quit Airboss".
- **Empty:** "Nothing needs you" (disabled), then the same items below it.

### Shortcut sheet

- ? opens the sheet, built from the command registry, in DESIGN's two-column layout. The hotkey shown is the one in Settings.
- Esc or ? closes it.

### Settings (⌘,)

A modal in the main window, built from the design's components:

- **Quick-add hotkey:** a recorder. If the key is taken: "That key is in use by another app. Pick another."
- **Launch at login:** a switch.
- **Appearance:** System, Dark or Light.
- **Command-line tool:** Install, with its path.
- **Agent setup:** "Copy agent note" and "Copy setup command" (`claude mcp add --scope user airboss -- ~/.local/bin/airboss mcp`).

### First run, T3 missing, nothing needs you

- **First run** is any start with no tasks. All tasks shows DESIGN screen 10: "Nothing here yet", New task ⌘N, "Change hotkey…" (which opens Settings), and the Connections card:

  | Row | OK | Off | Button |
  |---|---|---|---|
  | Agents | "Agents · Ready": the CLI is installed, and the time of the last agent report if there is one | "Agents · Not set up" | Install command-line tool, then Copy agent note |
  | T3 Code | "T3 Code · Found" | "T3 Code · Not found": Do it and Plan it are off until T3 Code is installed | Check again |
  | GitHub | "GitHub · Signed in" | "GitHub · Not signed in": PRs won't sync. Run `gh auth login`. | Check again |

- **T3 missing** anywhere: as in the panel above, plus "T3 Code not found" in the footer. ⌘↩ shows the toast.
- **Nothing needs you:** as in Needs me above. The menu bar shows no number.

## Interfaces: sync, launch, attention

In `crates/services` (`airboss-services`), types only. The app holds each service as `Arc<dyn …>`.

```rust
pub type SharedStore = Arc<std::sync::Mutex<airboss_core::Store>>;
pub type Timestamp = String;                 // UTC, as core writes it

/// Every service: may block briefly, never on the main thread; cheap reads come from memory.
pub trait Service: Send + Sync {
    fn connection(&self) -> Vec<Connection>;           // one per outside dependency it has
    fn recheck(&self);                                 // schedule a check now; returns at once
    fn changes(&self) -> tokio::sync::watch::Receiver<u64>;  // bumps when anything it reports changed
    fn store_changed(&self, events: &[airboss_core::Event]); // called by the app after every catch-up
}

pub struct Connection { pub service: ServiceId, pub ok: bool, pub state: String /* "Signed in" */,
                        pub what: String, pub fix: Option<String> }
pub enum ServiceId { T3, GitHub, Agents }

// ---------- sync ----------
pub trait SyncService: Service {
    fn threads(&self) -> HashMap<TaskId, Vec<ThreadLive>>;   // only tasks with linked threads
    fn prs(&self) -> HashMap<LinkId, PrInfo>;                // only PR links it has seen
    fn last_checked(&self) -> Option<Timestamp>;
}
pub struct ThreadLive { pub link: LinkId, pub thread_ref: String, pub name: String,   // "slot-picker"
                        pub repo: Option<RepoId>, pub state: LiveState, pub since: Timestamp,
                        pub summary: Option<String> }                              // "asked a question"
pub enum LiveState { Running, WaitingOnYou, Stopped }
pub struct PrInfo { pub link: LinkId, pub state: PrState, pub checks: Checks, pub additions: u32,
                    pub deletions: u32, pub title: Option<String>, pub url: String }
pub enum Checks { Passing, Failing, Pending, None }

// ---------- launch ----------
pub trait LaunchService: Service {
    fn availability(&self) -> Availability;
    fn branch_for(&self, task: TaskId) -> Result<String, LaunchError>;  // stored branch, or the one Do it would set
    fn do_it(&self, task: TaskId, start: StartIn) -> Result<Launched, LaunchError>;
    fn plan_it(&self, task: TaskId) -> Result<Launched, LaunchError>;
    fn open_thread(&self, link: LinkId) -> Result<(), LaunchError>;     // brings the thread up in T3
}
pub enum Availability { Ready, T3Missing, T3Unusable { reason: String } }
pub enum StartIn { LinkedRepos, NoRepo }      // "Start in <repo>": the app links the repo first
pub struct Launched { pub threads: Vec<StartedThread>, pub branch: Option<String> }
pub struct StartedThread { pub link: LinkId, pub repo: Option<RepoId>, pub name: String }
pub enum LaunchError { T3Missing, NeedsRepoChoice, Waiting { on: Vec<TaskId> }, AlreadyDone,
                       Failed { message: String }, Core(airboss_core::Error) }

// ---------- attention ----------
pub trait AttentionService: Service {
    fn needs_me(&self) -> NeedsList;
    fn count(&self) -> u32;                                   // the menu-bar number: items not dismissed or snoozed
    fn dismiss(&self, key: &NeedsKey) -> Result<(), AttentionError>;
    fn snooze(&self, key: &NeedsKey, until: Timestamp) -> Result<(), AttentionError>;
}
pub struct NeedsList { pub items: Vec<NeedsItem>, pub last_checked: Option<Timestamp> }
pub struct NeedsKey(pub String);              // stable across restarts, e.g. "question:t3/97b5:req-12"
pub struct NeedsItem { pub key: NeedsKey, pub kind: NeedsKind, pub task: TaskId, pub at: Timestamp,
                       pub headline: String,   // the question, or "+412 −38 · checks passed"
                       pub context: String,    // one line, e.g. "Agent in slot-picker asks"
                       pub link: Option<LinkId>, pub url: Option<String>,
                       pub proposed: Vec<TaskId> }                // Proposed kind only
pub enum NeedsKind { Question, Approval, Review, FailedRun, Proposed, Ready, FinishIt, Quiet }
pub enum AttentionError { NotFound, Core(airboss_core::Error) }
```

- **The app** decides whether Do it may run before calling it: not done, not waiting, and a repo choice made. `launch` checks again and returns the matching `LaunchError`.
- **The Agents row** of the connections belongs to the app: whether the CLI is installed, and the time of the newest `agent:*` event.
- **Items vanish when their cause does.** `attention` recomputes from `core` and `sync` on `store_changed` and on `sync.changes()`. Accepting proposals or starting a ready task clears the item without a dismiss.
- All these types derive `Serialize` and `Clone`; the app wraps them in DTOs for the UI.

## Performance

Targets from CAPABILITIES, measured by `scripts/measure.sh` on this Mac, release build, with a 2,000-task seeded database (`examples/seed.rs`: 2,000 tasks, 5 levels, 1,000 dependencies, 40 linked threads). The machine is printed next to the numbers.

| Target | How it's met | How it's measured |
|---|---|---|
| Download under 15 MB | Release profile above; one Inter subset; no JS libraries; main bundle under 150 KB and quick-add under 40 KB (minified) | Size of the `.dmg` from `tauri build --bundles dmg`, `airboss` sidecar included |
| Idle memory under 80 MB | Two web views, a native tray menu, `$state.raw` rows | 60 s after start, window visible on All tasks: the sum of `footprint` for the app process and its WebKit WebContent, Networking and GPU processes |
| Idle CPU near 0% | No polling and no timers except the minute tick while visible; the live pulse animates only `transform` and `opacity` | `top -l 31 -s 2` over 60 s: average under 0.5% with no working chip on screen and under 1% with one; 0% with the window hidden |
| Cold start under 1 s | Window created hidden; one `bootstrap` call; shown on first paint | Rust logs from process start (`sysctl kern.proc` start time) to `ui_ready` after two animation frames; median of 10 cold starts after `purge` |
| Hotkey to quick add under 100 ms | The panel exists from start-up; showing it does no I/O | Rust logs from the shortcut callback to `quickadd_shown` (sent after one animation frame); 95th percentile of 20 presses |
| 2,000 rows at 60 fps | Virtual list, fixed heights, flat rows | Playwright WebKit on the mock build with 2,000 fixture rows: scroll the whole list in 3 s; 95th-percentile frame time under 16.7 ms and no task over 50 ms. Checked once by hand with the Web Inspector timeline in the real app. |
| Agent write visible under 300 ms | Watcher debounce plus `catch_up()` | `tests/watcher.rs`: a second `Store` writes; time until the patch arrives |
| Snapshot rebuild and diff under 15 ms; any write command under 25 ms | | `tests/perf.rs` (`#[ignore]`), 95th percentile of 200 runs |

## Accessibility

- **Keyboard only:** every action has a key or is reachable by Tab.
  - Tab order: sidebar, then the list (one stop with roving selection), then the panel's controls. Tab from the list goes to the panel and Shift-Tab comes back.
  - Menus, popovers and modals trap focus and give it back on close.
- **Focus:** a 1px inset accent outline on the focused row or inbox item. Other controls get a 2px accent ring with a 2px offset on `:focus-visible`.
- **Reduce Motion:** `prefers-reduced-motion` turns off every animation and transition, the live pulse included. The pulse becomes a static ring.
- **Increase Contrast** (`prefers-contrast: more`): hairlines go to 20% and 30%, text-3 takes text-2's value, hover and selected washes double, and the keyboard outline becomes 2px.
- **System accent:** Rust reads `NSColor.controlAccentColor` at start and on `NSSystemColorsDidChangeNotification`, and sends it as `appearance:changed`. It replaces the accent in focus rings, the keyboard outline and text selection only. Done, progress and the primary button stay Signal Indigo, and the Multicolor setting keeps indigo everywhere.
- **Never colour alone:** status always has its shape; live chips have a label (at narrow widths, a tooltip and accessible name); repo dots carry the repo name; the needs-me dot is named "Needs you".
- **VoiceOver:**
  - Tree layout is `role="tree"`, with rows as `treeitem` and `aria-level`, `aria-expanded`, `aria-setsize`, `aria-posinset`. Flat views are a `listbox`.
  - The list uses `aria-activedescendant`. The active row is always rendered, because selection scrolls it into view.
  - A row's label reads "144, Web: slot picker, In progress, agent working, needs you, booking-web, 6 minutes ago".
  - Map stations are buttons labelled "203 Payment webhook, blocked, wave 2, waits on 201".
  - Toasts sit in a `role="status"` live region.
  - The menu-bar item's accessibility label is "Airboss, 8 waiting on you".
- **Text:** nothing below 11.5px except key caps and initials (The 11.5 Floor Rule). The UI is laid out at 100% zoom; ⌘+ is not offered in v1.

## Testing

Tests come first, as in `core`.

- **Rust command tests** (`tests/commands.rs`):
  - Every command, called as a plain function against `AppState` built on `Store::open_in_memory()` and fake services. The fakes implement the traits with settable data and record their calls.
  - Each write checks the `core` result and the returned patch.
  - Each `core::Error` checks its message from Appendix D.
  - The quick-add sequence, with a forced link failure, leaves the task and reports the failure.
- **Snapshot tests** (`tests/snapshot.rs`): progress (leaves only, dropped left out, proposed as todo); the repo union and its flag; branch inheritance; `live` precedence; `diff` (upserts, removals, order only when changed); an ancestor upserted when a subtask changes.
- **Run-map tests:** the DESIGN example (Refunds: waves of 3, 2, 1 and 2 with one outside blocker), a phase-level lifted edge, done subtasks kept, a tangled pair, and the fallback threshold.
- **Watcher tests** (`tests/watcher.rs`), on a temporary file database:
  - A second `Store` handle (standing in for `airboss mcp`) writes, and one patch arrives within 300 ms.
  - A burst of 50 writes gives at most 3 patches.
  - Our own write gives no second patch.
  - `repo add` from another handle updates `repos`.
- **IPC test** (`tests/ipc.rs`): `tauri::test::mock_builder` with the real command handlers. One read and one write go through `invoke`, proving serialisation and capability wiring.
- **Fixtures** (`tests/fixtures.rs`):
  - Seeds the DESIGN sample data and a 2,000-task tree.
  - Writes every read command's output as JSON to `app/src/lib/mock/fixtures/`.
  - Writes the TS types.
  - CI fails if the generated files changed.
- **UI unit tests** (Vitest 5.1.2, Node): `views.ts`, `search.ts`, `runMapLayout.ts`, `time.ts`, `links.ts`, `keymap.ts`, and the patch logic in `tasks.svelte.ts`.
- **Component tests** (Vitest 5.1.2 browser mode with `@vitest/browser-playwright` 5.1.2 on WebKit, `vitest-browser-svelte` 3.1.0), on the mock transport:
  - `TaskRow` at each status and width.
  - `VirtualList`: 2,000 rows render under 60 DOM rows, and the selection stays visible.
  - `StatusMenu` (keys 1–6, blocked reason, drop confirm).
  - `NewTask` (Tab and Shift-Tab dropdowns, paste a Slack link, ⌘↩).
  - `Jump`, `NeedsInbox` (actions per kind), `TaskPanel` (rename, notes, T3 missing banner).
  - The keyboard map per context, and the reduced-motion class.
- **End-to-end smoke test:** Playwright 1.65.1, WebKit, against `vite build --mode mock`, keyboard only, in dark and then light:
  1. Open on Needs me and accept a proposal with ↩.
  2. G A, then ↓↓, Space, and S 4 with a reason.
  3. ⌘K, type an id, ↩, and ⌘↓ for the page with the map.
  4. C, type a title, Tab to a repo, ↩.
  5. ? for the sheet.

  It also saves screenshots for review. Tauri's WebDriver (`tauri-driver`) doesn't support macOS, so this is the honest choice: the real UI code on WebKit with a contract-true mock, plus the Rust integration tests above.
- **What that leaves unchecked:** the real WKWebView inside Tauri (as opposed to Playwright's WebKit), native windows and chrome, the quick-add panel over a full-screen app, the global hotkey and its timing, the native menus and menu-bar item, launch at login, single instance, focus moving between windows, the real watcher with a real `airboss mcp` process, VoiceOver and the system accent. A manual checklist, `app/QA.md`, covers these before each merge to main.

## Code style

- Rust: as in `core`. Rustfmt, clippy `-D warnings`, no `unwrap` or `expect` outside tests, wrapper id types, a short "why" doc comment on every public item. No SQL in the app; everything goes through `Store`.
- TypeScript in strict mode. Svelte 5 runes only (no `export let`, no stores from `svelte/store`).
- Components get props and callbacks and call no IPC; stores and `lib/ipc/api.ts` do. Styles are scoped in each component, with no bare global classes (DESIGN Do's). Colours, sizes and motion come only from `tokens.css` variables.

```ts
/** Patches can arrive twice (returned and as an event); apply each cursor once. */
applyPatch(p: Patch) {
  if (p.cursor <= this.cursor) return;
  const rows = new Map(this.rows);
  for (const r of p.upserts) rows.set(r.id, r);
  for (const id of p.removed) rows.delete(id);
  this.rows = rows;
  if (p.order) this.order = p.order;
  this.cursor = p.cursor;
}
```

## Boundaries

- **Always:** act as `me` through `Store` or a service trait; return the patch from every write; keep rows at 38px; take colours from tokens; add every shortcut to the command registry (it feeds the keymap, menus and sheet); run fmt, clippy, `npm run check` and every test before each commit.
- **Ask first:** adding any crate or npm package; adding a window or web view; changing a trait in `crates/services`; anything needing network access; changing a DESIGN.md rule.
- **Never:** poll the database or a service on a timer; open a port; write SQL; call `t3` or `gh` (that's `launch` and `sync`); use red for anything but needs-me; load remote content; show ports, protocols or acronyms in the main UI.

## Success criteria

- [ ] Every screen in `docs/design/screens/` is matched in the real app, in dark and in light, checked side by side.
- [ ] The QA flow in program.md works keyboard only: capture, plan, Do it, an agent reports, a PR merges, Needs me clears.
- [ ] A write from `airboss mcp` or the CLI shows in the open window within 300 ms, with no polling.
- [ ] Every command in Appendix A has a Rust test; every view function, the layout and the keymap have unit tests; the listed components have component tests; the smoke test passes.
- [ ] Every performance target is measured with `scripts/measure.sh` and met, with the machine recorded.
- [ ] Without T3 Code, Do it and Plan it are off with the DESIGN wording, and everything else works. Without `gh`, the footer says so.
- [ ] VoiceOver reads a row, a needs-me item and a map station correctly. Reduce Motion and Increase Contrast visibly apply.
- [ ] No AI attribution anywhere. Clippy, fmt, `svelte-check` and every test are clean.

## Not verified yet

- **Key-equivalent order in WKWebView:** whether a native menu accelerator (⌘K, ⌘↩) fires as well as, or instead of, the page's `keydown`. The 50 ms dedupe covers both cases; the first build task checks which one happens.
- **`tauri-nspanel` 2.1.1 with Tauri 2.12.7:** converting a web-view window to a non-activating panel that takes key focus. If it fails, the fallback is a normal always-on-top window, which brings the main window forward when the app is in the background.
- **Memory with two web views:** whether WebKit shares one WebContent process between them. If the total is over 80 MB, quick add is created on first use and kept, which costs the first press its 100 ms.
- **`notify` 9.0.0** came out on 21 Nov 2026. If it misbehaves, pin 8.x.
- **Partial `tree()` refresh:** a full rebuild at 2,000 tasks is about 10 ms. If the profile shows the snapshot dominating, ask `core` for a `nodes(ids)` call.
- **`core` v2 names:** this spec uses the names in the brief (`link_repo`, `set_branch`, `history_with_subtasks`, `dependencies`, `ready() -> [{task, since}]`). If `core` v2 lands with other names, the app follows `core`.
- **Bundled `airboss` size:** the sidecar adds roughly 2 MB to the `.dmg`; to be measured.

## Conflicts resolved

| Where | DESIGN / prototype says | Spec / rule says | Resolution |
|---|---|---|---|
| Agents connection | "Agents · Listening" | SPEC-mcp: stdio per thread, nothing listens | Read as "Agents · Ready" (CLI installed, last report time) or "Not set up" |
| New task key | C or ⌥N; first-run screen shows ⌘N | Mac convention | C, ⌘N and ⌥N all open it; menus show ⌘N |
| Waves | Done subtasks shown in their wave; "4 waves" counts them | Rule 18: `run_order` leaves done out | The map, Waves list and Plans use display waves that include done (`runmap.rs`); `core` still decides what's ready |
| Menu-bar dropdown | A 360px custom popover | "Must feel like a real Mac app"; memory target | A native menu with the same content; times sit in the item text |
| Every shortcut in the menus | Single letters listed in menus | Bare-letter key equivalents would swallow typing | Single-letter keys appear in item titles, not as key equivalents |
| System accent | One accent, indigo, for done, selection, focus and primary | PRODUCT: respect the system accent where it fits | The system accent drives focus and selection outlines only |
| Capability map | `launch` depends on `app`; `sync` on `core` only | One-way dependencies | The traits live in a types-only crate, `crates/services`; the services depend on it, and the app binary wires them |
| Ready pill | On a task that just became ready | `ready()` lists every ready task | The pill shows only with an attention Ready item; the Ready view lists every ready leaf |
| ⌘0 | "Open Airboss on Needs me" | Not a global hotkey | ⌘0 is a Window-menu item and is shown in the menu-bar menu; only quick add is global |

## Open questions

1. **Launch at login:** on by default at first run (this spec), or off until I turn it on?
2. **Appearance:** follow the system (this spec, with dark designed first), or always open dark unless I choose light?

## Appendix A: Commands

Writes return `Patch` (shown as `→ P`). `S` is `Store`; all `core` calls act as `me`.

| Command | Arguments | Returns | Calls |
|---|---|---|---|
| `bootstrap` | | `{rows, order, repos, cursor, needs, connections, launch, settings}` | `S.tree`, `S.repos`, `S.ready`, `S.dependencies` (waiting rows), `sync.threads`, `attention.needs_me`, every `connection()`, `launch.availability` |
| `task_detail` | `id`, `history_limit` (30 or 200) | `TaskDetail` | `S.task`, `S.links`, `S.history_with_subtasks`, `S.dependencies`, `sync.threads`/`prs`, `launch.branch_for`, attention items for `id` |
| `run_map` | `id` | `RunMap {nodes, edges, waves, outside, tangled}` | `S.tree` (from the snapshot), `S.dependencies` per descendant, `S.run_order` |
| `plans` | | `Vec<PlanCard>` | the snapshot, `run_map` per parent, `sync.threads` |
| `quickadd_data` | | `{repos, parents}` | the snapshot |
| `create_task` | `{title, body?, parent?, repos[], links[], then: none / do_it / plan_it}` | `{id, patch, launched?}` | `S.add_task`, `S.link_repo` ×n, `S.link` ×n, then `launch.do_it` or `plan_it` |
| `rename_task` | `id, title` | → P | `S.edit_task` (title) |
| `edit_notes` | `id, body?` | → P | `S.edit_task` (body) |
| `set_status` | `id, status, note?` | → P | `S.set_status` (blocked needs `note`) |
| `toggle_done` | `id` | → P | `S.set_status` done, or todo when reopening |
| `accept_proposals` | `ids[]` | → P | `S.set_status` todo, each |
| `reject_proposals` | `ids[], reason?` | → P | `S.set_status` dropped (writes `rejected`) |
| `move_task` | `id, parent?, position?` | → P | `S.move_task` |
| `link_repo` / `unlink_repo` | `id, repo` | → P | `S.link_repo` / `S.unlink_repo` |
| `add_repo` | `path?` (none: open the folder picker) | `{repo, patch}` | dialog plugin, read `.git/config` for the remote, `S.add_repo` |
| `set_branch` | `id, branch` | → P | `S.set_branch` |
| `add_dependency` / `remove_dependency` | `id, waits_on` | → P | `S.add_dependency` / `S.remove_dependency` |
| `add_link` | `id, text, title?` | → P | classify, then `S.link` |
| `remove_link` | `link` | → P | `S.unlink` |
| `add_note` | `id, text` | → P | `S.add_note` |
| `do_it` | `id, start: linked_repos / repo(id) / no_repo` | `{launched, patch}` | `S.link_repo` for `repo(id)`, then `launch.do_it` |
| `plan_it` | `id` | `{launched, patch}` | `launch.plan_it` |
| `open_thread` | `link` | | `launch.open_thread` |
| `open_url` | `url` or `link` | | opener plugin (http, https, file and `t3://` only) |
| `needs_dismiss` | `key` | | `attention.dismiss` |
| `needs_snooze` | `key, days` | | `attention.snooze(now + days)` |
| `recheck_connections` | | | `recheck()` on every service |
| `copy_text` | `what: task_number(id) / agent_note / setup_command` | | clipboard plugin |
| `install_cli` | | `{path, on_path}` | symlink `~/.local/bin/airboss` to the bundled sidecar |
| `get_settings` / `set_settings` | `Settings` | `Settings` | `app-settings.json`; re-registers the hotkey; autostart plugin |
| `set_folded` | `ids[], folded` | | settings, debounced |
| `ui_ready` | `window` | | shows the window; records the start-up time |
| `quickadd_shown` / `quickadd_hide` / `quickadd_resize` | `height` | | panel show timing, hide, size |
| `show_main` | `view?, task?, needs_key?` | | shows and focuses the main window, emits `nav:open` |

## Appendix B: Events

| Event | Payload | Sent when |
|---|---|---|
| `tree:patch` | `Patch` | `catch_up()` found a change, or `sync`/`attention` changed a row field |
| `needs:changed` | `{items, count, last_checked}` | `attention.changes()` bumped |
| `connections:changed` | `{connections, launch}` | any service's `changes()` bumped and its connection differs |
| `menu:action` | `{command}` | a native menu item was chosen |
| `nav:open` | `{view?, task?, needs_key?}` | the menu-bar menu, the Dock or a second launch |
| `quickadd:open` | `{hotkey}` | the hotkey showed the panel |
| `appearance:changed` | `{accent?: string}` | the system accent changed |
| `error:background` | `{source, message}` | the watcher or a service failed; also moves the footer |

## Appendix C: Keyboard map

Single letters work only when no field has focus. DESIGN's table is the source; this adds where each key works.

| Key | List views | Needs me | Plans | Page / map | Anywhere in the app |
|---|---|---|---|---|---|
| ↑ ↓ | move | move | move by row of cards | map: within a wave | |
| → ← | open or fold; into first child or up to parent | | move by card | map: across waves | |
| ↩ | rename | main action | open page | map: open station | |
| Space | done / not done | | | | |
| S, 1–6 | status | | | | |
| R | link repos | | | | |
| ⌘↩ / ⇧⌘↩ | Do it / Plan it | main action | | on the page or the selected station | |
| ⌘↓ / ⌘↑ | open page | | | back | |
| Esc | close panel | | | close page, then clear the map selection, then close the panel | close the top overlay |
| ⌘D / ⌘L / ⌘B / ⌘O | waits-on / link / blocked / latest thread or PR | | | same | |
| ⌘⌫ | drop (confirms) | dismiss | | drop | |
| ⇧⌘C | copy task number | | | same | |
| Tab | to the panel | to the card | | | |
| C, ⌘N, ⌥N | | | | | new task |
| ⌘K, ?, ⌘, | | | | | Jump, shortcut sheet, Settings |
| G then N, L, R, A, P | | | | | Needs me, Plans, Ready, All tasks, In progress |
| ⌃⌥Space | | | | | quick add (global, from any app) |

## Appendix D: Error messages

| `core::Error` (as me) | Message |
|---|---|
| `EmptyTitle` | "A task needs a title." |
| `NotFound` | "Task 144 no longer exists." (or "That link no longer exists.") |
| `InvalidTarget` | Moving: "Tasks can't go under a proposed, done or dropped task." Dependency: "A task can't wait on its own parent or subtask." |
| `Loop` | "That would make a loop: 145 already waits on 144." (ids from the request) |
| `AlreadyExists` | "A repo with that name or folder is already added." |
| `NotAllowed`, `InvalidTransition` | "That change isn't allowed." (shouldn't happen as me; logged with detail) |
| `Db` | "Couldn't save. The database is busy or unreadable; details are in the log." |
| `LaunchError::Failed` | "T3 Code couldn't start the thread: <first line of its message>." |
| `LaunchError::Waiting` | "144 still waits on 143, 146." |
