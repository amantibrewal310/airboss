# Plan: the Airboss app UI

The Svelte 5 user interface for the Airboss Mac app, built against the command contract in
[SPEC-app.md](../SPEC-app.md) (draft 2, approved) and the look in [DESIGN.md](../DESIGN.md),
the Paper file "Airboss" and `docs/design/`. The Rust side (`app/src-tauri/**`) is built
separately; this plan covers everything in `app/` except `src-tauri/`.

## Overview

The UI draws what Rust sends. Rust owns the view model and sends rows and patches; the UI
keeps them in `$state.raw` stores, works out the visible list with pure functions, and draws
it in an in-house virtual list. Every command in Appendix A and every event in Appendix B has a
typed wrapper in `src/lib/api/`. Until the Rust side exists, a mock backend in
`src/lib/mock/` answers the same commands from fixtures in the browser, so every screen can be
built, tested and screenshotted now.

## Architecture decisions

- **One transport interface, two implementations.** `lib/api/transport.ts` defines
  `invoke(command, args)` and `listen(event, handler)`. `lib/api/tauri.ts` uses
  `@tauri-apps/api`; `lib/mock/` implements the same interface. The mock is chosen with
  `vite --mode mock`, through a compile-time constant, so a production build has no mock code.
- **Types are provisional.** `lib/types/` mirrors the Rust DTOs and service types by hand,
  with ids as `number` and serde's enum shapes. Every file says it will be replaced by ts-rs
  output.
- **Stores are classes in `lib/state/*.svelte.ts`.** Rows and order are `$state.raw`; a patch
  swaps in a new `Map` and replaces only the changed row objects, so a row component re-renders
  only when its own object changes.
- **Logic is pure and tested** in `lib/logic/`: views, selection, virtual-list maths, patch
  application, keymap and focus rules, the command registry, search, run-map layout, time,
  links, connection merging, launch toasts, the reject window.
- **The mock simulates the backend.** It holds a small model (tasks, edges, repos, links,
  threads, PRs, needs) and builds `RowDto`s the way SPEC-app's view-model table says, so every
  write returns a real patch and the same patch arrives again as `tree:patch`. It throws
  `UnscriptedCall` for any command it doesn't know. A second, strict replay transport plays a
  recorded `{command, args, response, events}` script in order; that is where the Rust-generated
  fixtures plug in.
- **Fixtures:** `design` (the design's sample data, for screenshot baselines), `full`
  (about 60 tasks over 4 repos, the default), `perf` (2,000 generated tasks), plus states
  (`empty`, `t3missing`, `nothing`, `dberror`). Chosen with `?fixture=` in mock mode.
- **No libraries** beyond Svelte and `@tauri-apps/api` at run time. Plain CSS with tokens.

## Task list

Each task ends with `pnpm check`, `pnpm test` and `pnpm build` clean, then one commit.

### Phase 1: Foundations

- [x] **T1 Scaffold.** Vite 8 + Svelte 5 + TS 6, two entry pages, Vitest (unit in Node,
  component in WebKit), Playwright, scripts `dev`, `dev:mock`, `check`, `test`, `test:e2e`,
  `build`, `build:mock`. Versions pinned exactly as SPEC-app's table.
  *Check:* `pnpm check`, `pnpm test` (a smoke test), `pnpm build` all pass.
- [x] **T2 Tokens, type and focus.** `styles/tokens.css` (dark, light, system with
  `data-theme` override, Increase Contrast, Reduce Motion, system accent hook) and
  `styles/base.css` (reset, Inter variable woff2, focus rings, selection). Theme store applies
  System/Dark/Light. *Check:* unit test of the theme resolver; a token page renders in both
  themes.
- [x] **T3 Types and API.** `lib/types/` (core, services, dto, commands, events) and
  `lib/api/` with one function per Appendix A command and one listener per Appendix B event.
  *Check:* a unit test that every Appendix A command name and Appendix B event name has a
  wrapper (table-driven), and that the wrappers pass the documented argument names.
- [x] **T4 Mock backend and fixtures.** Model, row builder, `design`/`full`/`perf` fixtures,
  handlers for every command, `UnscriptedCall`, the replay transport, and the plug-in note.
  *Check:* unit tests: rows match the view-model rules (progress, repo union, branch
  inheritance, chip precedence, waiting), unknown command throws, replay is strict about order.

### Checkpoint A: data flows

- [x] `bootstrap` in the browser returns rows for each fixture; check, tests and build clean.

### Phase 2: Shell and list

- [x] **T5 Stores and patch application.** `tasks`, `needs`, `connections`, `ui`, `detail`,
  `toasts`, `settings`. `applyPatch` by `seq`. *Check:* unit tests: stale `seq` ignored, only
  changed rows replaced, order and repos only when sent.
- [x] **T6 Shell.** Window grid (232px sidebar, main, 480/380px panel), traffic-light space
  and drag regions, sidebar (nav with counts, Needs me badge, Parents with rings, Repos with
  dots, "+"), connections footer and popover (merged rows). *Check:* component test of the
  footer text for each connection state; screenshot by eye against screen 01.
- [x] **T7 Views and visible rows.** `logic/views.ts` for Ready, In progress, All (tree and By
  status), per repo, per parent; folds. *Check:* unit tests for each view and fold rule.
- [x] **T8 Virtual list and rows.** `VirtualList` (prefix sums, binary search, overscan 8,
  `translateY`, sticky group header, `nearest` scrolling) and `TaskRow` (all lanes, pills,
  chips, dot, avatar, time, narrow widths by container query). *Check:* unit tests of the
  maths; component test: 2,000 rows render under 60 DOM rows and the selection stays visible;
  Playwright frame-time measurement at 2,000 rows.

### Phase 3: Keyboard

- [x] **T9 Registry, keymap, focus zones.** `logic/commands.ts`, `keymap.ts`, `focus.ts`,
  one `keydown` listener, G-sequences, Mac arrows, F6. *Check:* unit tests of every row of
  Appendix C's focus table and of the key map.
- [x] **T10 Selection rules.** `logic/selection.ts` (same index on removal, fold selects
  parent, reveal unfolds and switches view, needs same index, map selection clears) and stale
  responses by request number. *Check:* unit tests per rule.

### Checkpoint B: the list works by keyboard

- [x] 2,000 rows scroll smoothly; every list key works; tests clean.

### Phase 4: Panel and page

- [x] **T11 Task panel.** Header (breadcrumb, Plan it, Do it with Starting…, open page, close),
  T3-missing line, title rename, callouts, properties (status, repos checklist, branch edit,
  parent, waits on, blocks, links with titles), notes editor, Waves list, threads, activity
  with subtasks and "Show more", note input, skeleton after 150 ms. *Check:* component tests
  (rename, notes, banner, Starting…).
- [x] **T12 Full page and run map.** Page layout with the properties rail; `runMapLayout.ts`
  (waves as columns, curves, wave skips, outside stubs, tangled list, fallback over 40 subtasks
  or 8 waves); map keys. *Check:* unit tests with DESIGN's Refunds example; screenshot against
  screen 03.

### Phase 5: Views

- [x] **T13 Needs me.** Inbox groups, card, every kind's actions, local hiding with the 180 ms
  exit, Accept/Reject with the 5 s window and Undo, Snooze, Dismiss, empty state.
  *Check:* unit tests of the reject window; component test of each kind's actions.
- [x] **T14 Ready, In progress, Plans.** Ready with "since"; Plans cards and grid keys; Plan
  something. *Check:* component test of a plan card; screenshot against screen 04.

### Phase 6: Overlays

- [x] **T15 Menus.** Status menu (1–6, blocked reason, drop confirmation, proposed
  Accept/Reject), context menu from the registry, Start in menu, repo checklist.
  *Check:* component tests for the status menu and Start in menu.
- [x] **T16 Jump, New task, quick add.** Jump with fuzzy search, commands, create, and picker
  mode; `NewTask` for both the modal and `quickadd.html` (reset on hide, ↓ title→notes, Tab
  and Shift-Tab dropdowns, paste links, ⌘↩). *Check:* unit tests for `search.ts` and
  `links.ts`; component tests for Jump and NewTask.
- [x] **T17 Sheet, Settings, toasts and launch results.** Shortcut sheet from the registry,
  Settings (hotkey recorder, login, appearance, CLI, agent setup), toasts with Undo/Retry/
  "Add to T3", F6, `LaunchReport` toasts. *Check:* unit tests of the toast builder.

### Phase 7: States and patches

- [x] **T18 States.** First run (open at login ticked, Add repos, connections), T3 missing,
  nothing needs you, database error, empty views, Add repos dialog. *Check:* component test of
  Add repos.
- [x] **T19 Patches and optimism.** Event wiring, seq dedupe, fresh-row highlight, status pop,
  detail refetch on upsert, plans refetch debounce, needs local hiding with the 2 s cap.
  *Check:* unit tests of patch order and dedupe.

### Phase 8: Tests and polish

- [x] **T20 End-to-end smoke and baselines.** Playwright keyboard-only smoke test; 24
  screenshot baselines (12 web screens, dark and light). *Check:* `pnpm test:e2e` passes.
- [x] **T21 Polish.** Screen by screen against the PNGs and Paper: spacing, type, colour,
  hover and focus, motion, truncation tooltips, narrow widths. *Check:* side-by-side review.

### Checkpoint C: done

- [x] Check, every test and the build are clean; bundle sizes recorded; frame times recorded.

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| WebKit headless frame timing is noisy | Med | Measure rAF deltas over a scripted 3 s scroll in WebKit and Chromium; report both. |
| The Rust DTOs land with other names | Med | All names live in `lib/types/` and `lib/api/`; the report lists every name the backend must match. |
| Component tests need the WebKit browser | Low | `playwright install webkit` once; documented in the README. |
| Key equivalents in WKWebView reach the menu first | Med | One page listener with `preventDefault`; the 50 ms dedupe lives in Rust. Noted for QA. |

## Decisions

Readings of the spec chosen while building, closest to DESIGN.md. Each is something the
backend or a reviewer may want to know.

1. **`lib/api/` instead of `lib/ipc/`.** The brief for this work names `lib/api/`; the layout is
   otherwise SPEC-app's.
2. **The mock simulates writes until Rust records them.** SPEC-app's mock replays a script
   recorded from Rust, which doesn't exist yet. `MockBackend` applies each write to a small model
   and builds the patch the way the view-model table says; it throws `UnscriptedCall` for any
   unknown command. `mock/replay.ts` is the strict, in-order replay transport, ready for
   `tests/script.json`; `mock/fixtures/index.ts` is the one loader the generated fixtures replace.
3. **Display waves put a subtask that waits outside in the last wave.** SPEC-app's text says "the
   column after its latest inside blocker, or wave 1", but its own test (DESIGN's Refunds: waves
   of 3, 2, 1 and 2) and the design put Backfill (208) in wave 4. The mock follows the test and
   the design; Rust owns this rule.
4. **Narrow widths step at 700px and 560px of list width, not 820px and 680px.** The Paper screens
   show full repo pills in a 728px list beside the panel; DESIGN's numbers would turn them into
   dots. Two more steps (460px, 360px) hide the actor and the quieter pills so a 288px list
   (900px window, 380px panel) never overflows.
5. **Branch shows only the task's own branch.** Screens 01 and 02 have no Branch line for a
   subtask; the branch Do it would use (`Prepared.branch`) is in Do it's tooltip instead of a
   text-3 line.
6. **The panel has no Parent line for a top-level task, and the page rail drops "+ Link repo"
   once a task has repos** (its repo tags open the checklist), matching screen 03. Move under…
   stays in ⌘K, the context menu and the Task menu.
7. **Drop's icon in the context menu is grey.** The Paper menu draws it red; the Red Is Mine Rule
   keeps red for needs-me only.
8. **`AlreadyStarting` gets a toast**, "Already starting — check T3 in a moment." (EM decision of
   26 Nov), although Appendix D says no toast. A second ⌘↩ while the button says "Starting…"
   does nothing, as the spec asks.
9. **Esc on the page clears a map selection first, then closes the page.** Appendix C lists
   "close page, then clear the map selection"; clearing first is the progressive reading.
10. **In the panel, ↩ and Space press the focused button** (Appendix C, `panel` zone); with focus
    elsewhere in the panel, Space marks the selected task done.
11. **⌘W is left to the native menu** (a page can't stop a browser closing a tab); ⌘0 is handled
    by the page as well as the menu (the 50 ms dedupe is Rust's).
12. **The header's count is the open tasks the view holds, whatever is folded**, like the
    sidebar's (screen 01 shows 21 in both).
13. **A parent whose only subtasks are proposals isn't a plan yet**: it gets no plan card and no
    Parents entry, though its row folds and shows 0/3.
14. **Folds come from `Settings.folded`.** A task that becomes a parent later (Plan it, accepted
    proposals) starts open, so a new plan is visible.
15. **The Plans grid is `repeat(auto-fill, minmax(min(100%, 420px), 500px))`**: two 500px cards at
    1440px like screen 04, still at least 340px wide.
16. **The design fixture follows the contract where the design doesn't**: 144's thread is waiting
    on a question, so its chip reads "Waiting on you" (the design shows "Agent working"), and a
    proposal-only parent folds.
17. **Tabular figures only on numbers.** Inter's `tnum` widens the hyphen, so it's set on ids,
    counts, times and percentages, not on the body.
18. **New task in the main window creates with `then: 'none'` and then runs Do it or Plan it
    through the normal flow** (Starting…, result toasts). Quick add sends `then`, so Rust handles
    a failure while the panel is hidden, as the spec describes.
19. **Quick add keeps a typed title only when it was hidden by something other than Esc or
    Create** (clicking elsewhere, the hotkey).
20. **Screen 07's baseline shows the right-click menu**; only one overlay is open at a time, so
    the status menu is covered by component tests.
21. **List keys beyond Appendix C:** Home, End, Page Up, Page Down, ⌥↑ and ⌥↓ move by page or to
    the ends, as Mac lists do.
22. **Connections merge by name in the UI too** (worst state wins), so it doesn't matter whether
    Rust merges first.

## What the backend must match

Everything is in `src/lib/api/contract.ts` and `src/lib/types/`. Fields the UI needs beyond the
spec's text are marked EXTRA there:

- `Bootstrap.cli: { installed, path, on_path }` (Settings and the Agents row).
- `TaskDetail.history_more` (Show more) and `TaskDetail.suggested_prs` (the ⌘L popover).
- `HistoryEntry.thread_title` (for "Agent in <thread title>").
- Undoable writes (`set_status`, `toggle_done`, `reject_proposals`, `move_task`, `reorder`,
  `needs_dismiss`, `needs_snooze`) return `{ patch, undo: { id, label } | null }`, so a toast's
  Undo undoes its own entry.
- `CreateTaskResult.link_errors` ("Created 153, but couldn't link …").
- `AppError.code` values `db_unavailable` (`bootstrap`, `detail` = the database path) and
  `undo_refused`.
- `menu:action { command }` uses the ids in `src/lib/logic/commands.ts`.
- Command arguments use Tauri's camelCase names (`historyLimit`, `waitsOn`, `needsKey`); struct
  fields inside stay snake_case; enums are snake_case strings and data enums externally tagged.

## Results

- **Tests:** 106 unit tests (Node) and 41 component tests (WebKit) in Vitest; 30 Playwright tests
  (smoke, 60 fps, 4 accessibility, 24 screenshot baselines).
- **60 fps:** Playwright WebKit, perf fixture (2,000 tasks, every parent open, 1,991 rows,
  75,694px), whole list scrolled in 3 s: 300 frames, p50 10 ms, p95 12 ms, max 12 ms, none over
  20 ms; at most 40 rows in the DOM. (Headless WebKit runs rAF at 100 Hz.)
- **Bundle** (`pnpm build`, minified / gzip -9): the main window loads 245 KB of JS / 80 KB
  (its entry 148 KB / 44 KB, plus a 96 KB / 36 KB chunk it shares with quick add: the Svelte
  runtime, New task, menus, icons and the API) and 59 KB / 11 KB of CSS. Quick add loads its own
  2.8 KB entry plus the shared chunk: 100 KB / 38 KB of JS and 11 KB / 3.4 KB of CSS. The font is
  48 KB (Inter variable, Latin). No mock code reaches the production build.
- **Gap:** SPEC-app's budget asks for quick add under 40 KB minified. Its own code is 2.8 KB, but
  the shared Svelte runtime and form components make it 100 KB minified (38 KB gzipped). Meeting
  the letter of the budget needs a smaller form for the panel (or counting gzip); left for the
  integration pass.
