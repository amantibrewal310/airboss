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

- [ ] **T1 Scaffold.** Vite 8 + Svelte 5 + TS 6, two entry pages, Vitest (unit in Node,
  component in WebKit), Playwright, scripts `dev`, `dev:mock`, `check`, `test`, `test:e2e`,
  `build`, `build:mock`. Versions pinned exactly as SPEC-app's table.
  *Check:* `pnpm check`, `pnpm test` (a smoke test), `pnpm build` all pass.
- [ ] **T2 Tokens, type and focus.** `styles/tokens.css` (dark, light, system with
  `data-theme` override, Increase Contrast, Reduce Motion, system accent hook) and
  `styles/base.css` (reset, Inter variable woff2, focus rings, selection). Theme store applies
  System/Dark/Light. *Check:* unit test of the theme resolver; a token page renders in both
  themes.
- [ ] **T3 Types and API.** `lib/types/` (core, services, dto, commands, events) and
  `lib/api/` with one function per Appendix A command and one listener per Appendix B event.
  *Check:* a unit test that every Appendix A command name and Appendix B event name has a
  wrapper (table-driven), and that the wrappers pass the documented argument names.
- [ ] **T4 Mock backend and fixtures.** Model, row builder, `design`/`full`/`perf` fixtures,
  handlers for every command, `UnscriptedCall`, the replay transport, and the plug-in note.
  *Check:* unit tests: rows match the view-model rules (progress, repo union, branch
  inheritance, chip precedence, waiting), unknown command throws, replay is strict about order.

### Checkpoint A: data flows

- [ ] `bootstrap` in the browser returns rows for each fixture; check, tests and build clean.

### Phase 2: Shell and list

- [ ] **T5 Stores and patch application.** `tasks`, `needs`, `connections`, `ui`, `detail`,
  `toasts`, `settings`. `applyPatch` by `seq`. *Check:* unit tests: stale `seq` ignored, only
  changed rows replaced, order and repos only when sent.
- [ ] **T6 Shell.** Window grid (232px sidebar, main, 480/380px panel), traffic-light space
  and drag regions, sidebar (nav with counts, Needs me badge, Parents with rings, Repos with
  dots, "+"), connections footer and popover (merged rows). *Check:* component test of the
  footer text for each connection state; screenshot by eye against screen 01.
- [ ] **T7 Views and visible rows.** `logic/views.ts` for Ready, In progress, All (tree and By
  status), per repo, per parent; folds. *Check:* unit tests for each view and fold rule.
- [ ] **T8 Virtual list and rows.** `VirtualList` (prefix sums, binary search, overscan 8,
  `translateY`, sticky group header, `nearest` scrolling) and `TaskRow` (all lanes, pills,
  chips, dot, avatar, time, narrow widths by container query). *Check:* unit tests of the
  maths; component test: 2,000 rows render under 60 DOM rows and the selection stays visible;
  Playwright frame-time measurement at 2,000 rows.

### Phase 3: Keyboard

- [ ] **T9 Registry, keymap, focus zones.** `logic/commands.ts`, `keymap.ts`, `focus.ts`,
  one `keydown` listener, G-sequences, Mac arrows, F6. *Check:* unit tests of every row of
  Appendix C's focus table and of the key map.
- [ ] **T10 Selection rules.** `logic/selection.ts` (same index on removal, fold selects
  parent, reveal unfolds and switches view, needs same index, map selection clears) and stale
  responses by request number. *Check:* unit tests per rule.

### Checkpoint B: the list works by keyboard

- [ ] 2,000 rows scroll smoothly; every list key works; tests clean.

### Phase 4: Panel and page

- [ ] **T11 Task panel.** Header (breadcrumb, Plan it, Do it with Starting…, open page, close),
  T3-missing line, title rename, callouts, properties (status, repos checklist, branch edit,
  parent, waits on, blocks, links with titles), notes editor, Waves list, threads, activity
  with subtasks and "Show more", note input, skeleton after 150 ms. *Check:* component tests
  (rename, notes, banner, Starting…).
- [ ] **T12 Full page and run map.** Page layout with the properties rail; `runMapLayout.ts`
  (waves as columns, curves, wave skips, outside stubs, tangled list, fallback over 40 subtasks
  or 8 waves); map keys. *Check:* unit tests with DESIGN's Refunds example; screenshot against
  screen 03.

### Phase 5: Views

- [ ] **T13 Needs me.** Inbox groups, card, every kind's actions, local hiding with the 180 ms
  exit, Accept/Reject with the 5 s window and Undo, Snooze, Dismiss, empty state.
  *Check:* unit tests of the reject window; component test of each kind's actions.
- [ ] **T14 Ready, In progress, Plans.** Ready with "since"; Plans cards and grid keys; Plan
  something. *Check:* component test of a plan card; screenshot against screen 04.

### Phase 6: Overlays

- [ ] **T15 Menus.** Status menu (1–6, blocked reason, drop confirmation, proposed
  Accept/Reject), context menu from the registry, Start in menu, repo checklist.
  *Check:* component tests for the status menu and Start in menu.
- [ ] **T16 Jump, New task, quick add.** Jump with fuzzy search, commands, create, and picker
  mode; `NewTask` for both the modal and `quickadd.html` (reset on hide, ↓ title→notes, Tab
  and Shift-Tab dropdowns, paste links, ⌘↩). *Check:* unit tests for `search.ts` and
  `links.ts`; component tests for Jump and NewTask.
- [ ] **T17 Sheet, Settings, toasts and launch results.** Shortcut sheet from the registry,
  Settings (hotkey recorder, login, appearance, CLI, agent setup), toasts with Undo/Retry/
  "Add to T3", F6, `LaunchReport` toasts. *Check:* unit tests of the toast builder.

### Phase 7: States and patches

- [ ] **T18 States.** First run (open at login ticked, Add repos, connections), T3 missing,
  nothing needs you, database error, empty views, Add repos dialog. *Check:* component test of
  Add repos.
- [ ] **T19 Patches and optimism.** Event wiring, seq dedupe, fresh-row highlight, status pop,
  detail refetch on upsert, plans refetch debounce, needs local hiding with the 2 s cap.
  *Check:* unit tests of patch order and dedupe.

### Phase 8: Tests and polish

- [ ] **T20 End-to-end smoke and baselines.** Playwright keyboard-only smoke test; 24
  screenshot baselines (12 web screens, dark and light). *Check:* `pnpm test:e2e` passes.
- [ ] **T21 Polish.** Screen by screen against the PNGs and Paper: spacing, type, colour,
  hover and focus, motion, truncation tooltips, narrow widths. *Check:* side-by-side review.

### Checkpoint C: done

- [ ] Check, every test and the build are clean; bundle sizes recorded; frame times recorded.

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
