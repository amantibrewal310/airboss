# Implementation plan: app backend

The Rust side of module `app` from [CAPABILITIES.md](../CAPABILITIES.md): the crate `airboss-app` in `app/src-tauri`. It builds what [SPEC-app.md](../SPEC-app.md) (draft 2, approved) asks of Rust, on `core` v3 and the std-only contract in `crates/services`. The tasks and checkpoints are in [app-backend-todo.md](app-backend-todo.md). The Svelte UI is built in parallel in its own worktree; `sync`, `launch` and `attention` are built in parallel too, so this crate runs on the `fakes` of `crates/services` until integration.

State: **planned 26 Nov 2026**.

## Overview

One Tauri 2.12.7 app with two web views (the main window and the quick-add panel), a native menu-bar item and native menus. Rust owns the view model: it reads one `core.snapshot()` per change, builds every row, diffs, and sends patches with their own sequence numbers. Every trigger (our writes, other processes' writes seen through kqueue on the `-wal` file, focus and wake through `data_version()`, and any service's `Changes` bump) leads to one `catch_up()`.

The work is cut into thin slices with tests first: state and errors, then the view model, then the commands as plain functions, then the background triggers, then the Tauri glue and the native pieces, then types, fixtures and measurements.

## Architecture decisions

- **A library plus a thin binary.** `src/lib.rs` holds every module; `src/main.rs` builds the Tauri app (plugins, windows, run loop). Integration tests in `tests/` call the plain command functions against `Store::open_in_memory()` with fake services and no Tauri.
- **`Shell` is the one seam to the native side.** `AppState` holds an `Arc<dyn Shell>` for everything that isn't `core` or a service: emitting events, showing windows, the clipboard, the opener, the folder picker, autostart, the hotkey, menus and the tray. `TauriShell` implements it in the app; `RecordingShell` in tests records every call. So every command, including `open_url`, `copy_text` and `set_settings`, is testable without Tauri.
- **One lock per `core` call.** `AppState::core(|store| …)` locks the `SharedStore`, recovers a poisoned lock with `into_inner()` (logged once), runs one call and drops the guard. Services and the shell are only ever called outside it.
- **A second mutex, `catchup`, owns the view model,** the event cursor, the last `repos()`, the last `data_version`, `seq`, the last needs version and the last connections. A write's inline `catch_up()` and the worker's are serialised by it.
- **Commands are declared once.** A `commands!` macro in `ipc.rs` declares each command's arguments once and generates: the `…Args` struct (serde + ts-rs), the async `#[tauri::command(rename_all = "snake_case")]` wrapper (which runs the plain function in `spawn_blocking` inside `catch_unwind`), `handler()` for `generate_handler!`, and `dispatch(name, json)` for the fixtures recorder. `build.rs` reads the same list of names for the app manifest, so each window's capability file can allow only its own commands.
- **DTOs** in `dto.rs` mirror the `core` and service types the UI sees, with `From` conversions; ids and `seq` are `number` in TypeScript.
- **App threads are std threads** in `catch_unwind`: the catch-up worker (one `mpsc` channel of triggers, debounced 30 ms after the last and at most 150 ms after the first), the WAL watcher, the `subscribe()` forwarder, one listener per service (`wait_past(seen, 15 min)`), and the deferred-reject timer. Each restarts once after a panic; a second panic stops it and adds a "Live updates stopped" row to the connections footer.
- **Unsafe only in `src/macos.rs`:** the `tauri_panel!` class for quick add, the wake observer, the system accent and the local time offset. The crate's lints override the workspace's `forbid` with `deny`, and `macos.rs` alone allows it.

## Dependency graph

```
T1 scaffold (crate, conf, capabilities, icons, profile)
 └─ T2 AppState, errors, settings, services.rs, RecordingShell
     └─ T3 view model + runmap + diff → Patch
         └─ T4 catch_up() (inline) + bootstrap and the read commands
             ├─ T5 task writes (+ patches)
             │   └─ T6 undo stack + deferred rejects
             ├─ T7 launch commands (do_it, plan_it, reopen, add_to_t3)
             ├─ T8 needs, links, repos, app commands
             └─ T9 background: worker, debounce, WAL watcher, listeners, focus/wake
T10 ipc.rs wrappers, capability files, tests/ipc.rs   (after T5–T8)
T11 windows, quick-add panel, hotkey, autostart        (after T10)
T12 tray, native menus, macos.rs                       (after T11)
T13 ts-rs types + generated fixtures + seed example     (after T10)
T14 performance: perf tests, release build, measure     (after T12, T13)
T15 self-review and fixes                              (last)
```

## Phases

1. **Foundation (T1–T2):** the crate builds with every plugin; state, errors and settings.
2. **Data (T3–T4):** rows, patches and every read command.
3. **Writes (T5–T8):** every write command with its patch, undo and the launch flow.
4. **Live (T9):** writes from other processes, focus and wake.
5. **Native (T10–T12):** Tauri commands, windows, the panel, the tray and the menus.
6. **Finish (T13–T15):** types, fixtures, measurements, review.

## Decisions (where the spec can't work as written, or left a gap)

1. **Command arguments are snake_case** (`#[tauri::command(rename_all = "snake_case")]`), the same as every DTO field, so the UI calls `invoke('task_detail', { id, req, history_limit })`.
2. **Read commands with a request number echo it.** `plans` returns `{ req, cards }` rather than a bare `Vec<PlanCard>`, as `task_detail` and `run_map` do.
3. **A PR link's title can't be filled in by linking again:** `core`'s `link` returns the existing link unchanged. `add_link` links the PR at once; the background `lookup_pr` then corrects its state with `set_pr_state` (as me) if GitHub says merged or closed. The title shown comes from `sync`'s `PrView`.
4. **Settings live next to the database** (`<db folder>/app-settings.json`), so a temporary `AIRBOSS_DB` isolates a dev run's settings as well.
5. **First run turns autostart on only inside an `.app` bundle,** so `cargo run` and tests never register a LaunchAgent for a debug binary.
6. **The release profile is in the root `Cargo.toml`,** because Cargo ignores profiles in member crates. It also shrinks the `airboss` sidecar.
7. **Lifted run-map edges are done** when every underlying blocker is done or dropped.
8. **The Agents connection's "last report"** is the newest `last_event_at` among tree nodes whose `last_actor` is an agent (no extra `core` read).
9. **Error codes are snake_case strings** (`"needs_choice"`, `"already_starting"` …), as `crates/services` serialises them. `AlreadyStarting` has the EM's message, "Already starting — check T3 in a moment."
10. **Do it and Plan it results carry their toast** (`Started.toast`, built in Rust from `LaunchReport` with the spec's table), so quick add and the main window word them the same way.
11. **A second launch with `--quick-add`** opens the quick-add panel through single-instance. It lets the hotkey path be timed without synthetic key events.
12. **The menu-bar item's accessibility label** is its tooltip: Tauri 2.12 exposes no other label for a tray item.
13. **`scripts/measure.sh` lives in `app/src-tauri/scripts/`,** inside this crate's folder.
14. **The database failing to open at start** makes `bootstrap` return `db_unavailable` with the path and reason; calling `bootstrap` again is "Try again".

## Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| `tauri-nspanel` 2.1.1 doesn't make a Tauri 2.12.7 web view a key, non-activating panel | High | Checked in T11 with a real release build; the fallback (an always-on-top normal window) is written down if it fails. |
| `notify` 9's kqueue watcher misses WAL writes or the re-arm | High | T9 tests a second `Store` writing, and the WAL deleted and recreated; a small kqueue module is the fallback. |
| A service or the shell called with the store lock held | High | A lock-checking wrapper around every fake asserts the lock is free on each call. |
| `generate_context!` or the sidecar needing build artefacts on a clean checkout | Medium | `build.rs` writes a placeholder sidecar when it's missing; debug builds use `devUrl`, so no `dist` is needed for tests. |
| Fixtures that change on every run (wall-clock times) | Medium | Times from `core` are mapped to a fixed clock before writing; services and view model take a `Clock`. |
| Memory over 80 MB with two web views | Medium | Measured in T14; the 10-minute panel release is the fallback. |
| Tests touching the real database | High | Tests use in-memory or temporary files only; dev runs set `AIRBOSS_DB`. |

## Parallel work

Built in one thread, in order: every slice touches `state.rs`, `dto.rs` or `ipc.rs`. The UI is built in parallel against the generated types and fixtures, which land early in T13 if the UI needs them sooner.

## Open questions

None.
