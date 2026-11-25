# Tasks: app backend

The plan is in [app-backend-plan.md](app-backend-plan.md), and the spec is [SPEC-app.md](../SPEC-app.md).

Every task meets the Definition of Done:

- tests are written first where there's logic, and pass
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean
- no `unwrap` or `expect` outside tests
- no test or dev run touches the real database: tests use `Store::open_in_memory()` or a temp file, dev runs set `AIRBOSS_DB`

The focused test command is `cargo test -p airboss-app`.

After the UI landed, the command layer was matched to `app/src/lib/api/contract.ts`, and the real `sync`, `launch` and `attention` replaced the fakes (which stay for tests).

Sizes: XS = 1 file, S = 1–2 files, M = 3–5 files.

## Phase 1: Foundation

- [x] **T1. Scaffold** (M)
  - **Description:** `app/src-tauri` as `airboss-app` in the workspace, with Tauri 2.12.7 and the spec's plugins at their exact latest versions; `tauri.conf.json` with both windows created hidden, `quickadd` transparent and always on top, the bundle identifier and the sidecar; the capability files; icons and tray icons; the size-tuned release profile with `panic = "unwind"`; `build.rs` with a placeholder sidecar.
  - **Acceptance:** `cargo build -p airboss-app` and a release `tauri build` with a placeholder `dist` succeed; the app starts with no window shown.
  - **Verify:** `cargo build -p airboss-app`; `tauri build --bundles app`.
  - **Files:** `Cargo.toml`, `app/src-tauri/{Cargo.toml,build.rs,tauri.conf.json,capabilities/*,icons/*,tray/*,src/main.rs,src/lib.rs}`
  - **Depends on:** none

- [x] **T2. AppState, errors, settings, services** (M)
  - **Description:** `AppState` with the shared store, the services as `Arc<dyn …>`, the shell, the catch-up state, the undo stack, the launch set and settings. `AppError` and Appendix D's messages. `Settings` read and written as JSON next to the database. `services.rs` builds the fakes. `RecordingShell` and lock-checking fakes for tests.
  - **Acceptance:** one lock per `core` call; a poisoned lock recovers; each Appendix D message; settings round-trip and defaults (⌃⌥N, open at login, System).
  - **Verify:** `cargo test -p airboss-app --lib`, `tests/commands.rs` (errors).
  - **Files:** `src/state.rs`, `src/errors.rs`, `src/settings.rs`, `src/services.rs`, `src/shell.rs`, `tests/common/mod.rs`
  - **Depends on:** T1

### Checkpoint 1

- [x] All tests pass; clippy and fmt are clean; the app builds.

## Phase 2: Data

- [x] **T3. View model, run map, diff** (M)
  - **Description:** `RowDto` from one `CoreSnapshot` plus repos, `sync.snapshot()` and `attention.needs_me()`; plan cards; display waves and lifted edges; `diff` into `Patch { seq, upserts, removed, order, repos, created }`.
  - **Acceptance:** every view-model test in the spec's Testing section, and the run-map tests (the Refunds example, a lifted edge, done kept, a tangled pair, the fallback threshold).
  - **Verify:** `tests/viewmodel.rs`.
  - **Files:** `src/viewmodel.rs`, `src/runmap.rs`, `src/dto.rs`, `tests/viewmodel.rs`
  - **Depends on:** T2

- [x] **T4. Inline `catch_up()` and the read commands** (M)
  - **Description:** `catch_up()` for a `core` trigger and a service trigger; `bootstrap`, `task_detail`, `run_map`, `plans`, `quickadd_data`, `t3_projects`.
  - **Acceptance:** `catch_up` makes two small reads when nothing changed; services are told after the lock is released; each read's output.
  - **Verify:** `tests/commands.rs`.
  - **Files:** `src/catchup.rs`, `src/commands/{detail,mod}.rs`, `tests/commands.rs`
  - **Depends on:** T3

## Phase 3: Writes

- [x] **T5. Task writes** (M)
  - **Description:** `create_task`, `rename_task`, `edit_notes`, `set_status`, `toggle_done`, `accept_proposals`, `move_task`, `reorder`, `link_repo`, `unlink_repo`, `set_branch`, `add_dependency`, `remove_dependency`, `add_note`, each returning its patch.
  - **Acceptance:** each checks its `core` result, its patch and that `store_changed` came after the lock was released; errors map to Appendix D.
  - **Verify:** `tests/commands.rs`.
  - **Files:** `src/commands/tasks.rs`, `tests/commands.rs`
  - **Depends on:** T4

- [x] **T6. Undo** (S)
  - **Description:** the 20-deep stack, every inverse in the spec's table, refused undo, deferred rejects (5 s, cancelled, written at shutdown), `undo`, `reject_proposals`.
  - **Acceptance:** every undo test in the spec's Testing section.
  - **Verify:** `tests/undo.rs`.
  - **Files:** `src/undo.rs`, `src/commands/tasks.rs`, `tests/undo.rs`
  - **Depends on:** T5

- [x] **T7. Do it and Plan it** (S)
  - **Description:** `do_it`, `plan_it`, `reopen_and_start`, `add_to_t3`, with the in-flight guard, `NeedsChoice`, Start in <repo>, research only, `TaskClosed`, partial reports and their toasts.
  - **Acceptance:** the Do it tests in the spec; `launch.start` is called with no lock held.
  - **Verify:** `tests/commands.rs`.
  - **Files:** `src/commands/launch.rs`, `tests/commands.rs`
  - **Depends on:** T5

- [x] **T8. Needs, links, repos and app commands** (M)
  - **Description:** `needs_dismiss`, `needs_snooze`, `needs_restore`, `add_link` (classify, PR lookup in the background), `remove_link`, `add_repos`, `pick_folder`, `open_thread`, `open_url` (allowlist), `recheck_connections`, `copy_text`, `install_cli`, `get_settings`, `set_settings`, `set_folded`, `set_focus_context`, `ui_ready`, `quickadd_*`, `show_main`; merged connection rows.
  - **Acceptance:** each command has a test; `open_url` refuses anything but `http`, `https`, `file` and `t3code`.
  - **Verify:** `tests/commands.rs`.
  - **Files:** `src/commands/{needs,repos,app}.rs`, `src/classify.rs`, `tests/commands.rs`
  - **Depends on:** T5

### Checkpoint 2

- [x] Every Appendix A command has a Rust test; all tests pass.

## Phase 4: Live

- [x] **T9. Background triggers** (M)
  - **Description:** the catch-up worker with its debounce; the `subscribe()` forwarder; the kqueue watch on `-wal` and its folder, re-armed; focus and wake through `data_version()`; one listener per service; panics caught and restarted once.
  - **Acceptance:** the spec's catch-up tests: a second `Store` writes and one patch arrives within 300 ms; 50 writes give at most 3 patches; our own write gives no second patch; `repo add` updates `repos`; the WAL deleted and recreated re-arms; an unchanged `data_version` reads nothing.
  - **Verify:** `tests/catchup.rs`.
  - **Files:** `src/catchup.rs`, `src/watcher.rs`, `tests/catchup.rs`
  - **Depends on:** T4

## Phase 5: Native

- [x] **T10. Tauri commands and capabilities** (M)
  - **Description:** `ipc.rs`: the `commands!` macro, wrappers in `spawn_blocking` and `catch_unwind`, `handler()` and `dispatch()`; the app manifest in `build.rs`; `capabilities/main.json` and `capabilities/quickadd.json` with only each window's commands.
  - **Acceptance:** one read and one write through `invoke` on the mock runtime; quick add can't call `bootstrap`.
  - **Verify:** `tests/ipc.rs`.
  - **Files:** `src/ipc.rs`, `build.rs`, `capabilities/*`, `tests/ipc.rs`
  - **Depends on:** T5–T8

- [x] **T11. Windows, quick add, hotkey, autostart** (M)
  - **Description:** the main window (overlay title bar, traffic lights at 18,20, background per theme, shown on `ui_ready` or after 2 s, hidden with `--hidden`, ⌘W hides); `quickadd` as a non-activating `tauri-nspanel` panel on every Space and over full screen, positioned 22% down the screen with the mouse; the global hotkey on Pressed only; autostart with `--hidden`; single instance; window-state for size and position only.
  - **Acceptance:** with a real release build, the panel shows over another app without bringing the main window forward, in under 100 ms from the trigger.
  - **Verify:** manual run with a temporary `AIRBOSS_DB`; timings from the log.
  - **Files:** `src/main.rs`, `src/quickadd.rs`, `src/shell.rs`, `src/macos.rs`
  - **Depends on:** T10

- [x] **T12. Tray, menus, system accent, wake** (M)
  - **Description:** the menu-bar item (count as title, tooltip, native menu with up to six items and clock times, rebuilt on needs changes, debounced 250 ms); the app menus with key equivalents and Appendix C's disabling; `macos.rs` accent and wake observers.
  - **Acceptance:** clock-time labels and menu enabling rules have unit tests; the menus show in a real run.
  - **Verify:** `cargo test -p airboss-app --lib`; manual run.
  - **Files:** `src/tray.rs`, `src/menu.rs`, `src/macos.rs`
  - **Depends on:** T11

### Checkpoint 3

- [x] The app runs from a release build with both windows, the tray and the menus (the tray isn't visible to this session's tools; it's created without error).

## Phase 6: Finish

- [x] **T13. Types and fixtures** (M)
  - **Description:** ts-rs types into `app/src/lib/types/generated/`; `tests/fixtures.rs` seeds DESIGN's sample data and a 2,000-task tree, writes every read's output and records each scripted write's `{command, args, response, events}` into `app/src/lib/mock/fixtures/`; `examples/seed.rs`.
  - **Acceptance:** running the test twice changes nothing; `AIRBOSS_FIXTURES_CHECK=1` fails on any difference.
  - **Verify:** `cargo test -p airboss-app --test fixtures`, twice, then `git status`.
  - **Files:** `src/dto.rs`, `tests/fixtures.rs`, `tests/fixtures/script.json`, `examples/seed.rs`
  - **Depends on:** T10

- [x] **T14. Performance** (S)
  - **Description:** `tests/perf.rs` (`#[ignore]`): rebuild, diff and write timings at 2,000 tasks; `scripts/measure.sh`; measure cold start, idle CPU, idle memory with both web views, and the `.dmg` size from a real `tauri build`.
  - **Acceptance:** numbers recorded here against the spec's targets, with the machine.
  - **Verify:** `cargo test -p airboss-app --release -- --ignored perf`; `scripts/measure.sh`.
  - **Files:** `tests/perf.rs`, `scripts/measure.sh`
  - **Depends on:** T12, T13

- [x] **T15. Self-review** (S)
  - **Description:** code-review-and-quality over the whole crate; fix what it finds.
  - **Acceptance:** no open findings above "nit".
  - **Verify:** the full Definition of Done.
  - **Depends on:** T14

### Checkpoint 4

- [x] Everything above checked; measurements and deviations are in the plan. Idle memory misses its target (129 MB).
