# Integration QA: the whole product with the real tools

Branch `wave3/integration-qa`, from `main` at f23ef42. Done 26 Nov 2026, on an Apple M4 with 16 GB
running macOS 27.2.1, with `t3` 0.1.12, `gh` 2.105.0 (two accounts) and Claude Code 2.1.332.

Goal: prove that the integrated product works end to end with the real `airboss`, `t3`, `gh` and
`claude`, and find the bugs that only show when the parts run together. The unit and component
suites (941 Rust, 147 UI) already passed, so this plan covers only what they can't.

## Ground rules (kept)

- Every run used a throwaway `AIRBOSS_DB` under the session scratch folder. The real database was
  never created (`~/Library/Application Support/Airboss/` still doesn't exist).
- Each temporary database folder got an `app-settings.json` with open at login off before its first
  launch. `tests/e2e/app.sh db` writes it. See F1.
- The only writes to `~/.t3` and `~/.claude` came from step 4's single Do it and step 3's single
  `claude -p` run. That run used `--no-session-persistence`, `--strict-mcp-config` and a temporary
  `--mcp-config`. It wrote only Claude's own cache files, and `~/.claude.json` was never touched.
- The screen can't be captured, so the evidence comes from four places:
  - the app log
  - the database
  - the CLI's `--json` output
  - **`AIRBOSS_TRACE=1`**, a new debug trace that is off by default. It logs one line for each
    event sent to a web view (a `tree:patch` with its `seq`, ids, statuses and live chip;
    `needs:changed`; `connections:changed`), for each menu-bar redraw (the count it draws) and for
    each catch-up batch (its causes, time and `seq`). Each line carries `epoch_ms`.

## Plan

1. **Build and boot.** Build the release bundle and launch it through LaunchServices on a temporary
   database. Check for a clean log, that all three services start, and that the connection states
   are true. Measure cold start and idle memory.
2. **Live updates across processes.** While the app runs, make CLI and MCP writes:
   - add, nest and make one task wait on another
   - link a real merged PR
   - finish a task
   - accept and reject proposals

   For each write, time it to its `tree:patch`, check that `seq` rises, and check the Needs me
   items and the menu-bar count.
3. **A real agent over MCP.** Drive `airboss mcp` with a scripted JSON-RPC client, then make one
   `claude -p` run with a temporary `--mcp-config` and a fake `T3CODE_THREAD_ID`.
4. **One controlled real Do it.** In a throwaway repo, run the app's own `do_it` with the real tools.
   Use Add to T3 if T3 says the repo isn't a project yet.
5. **Failure paths:**
   - `gh` missing
   - T3 missing
   - an empty `PATH`
   - a long write lock
   - the WAL deleted while the app runs
   - wake
   - the hotkey taken by another app
6. **Performance at scale.** Use 2,000 tasks and 1,000 dependencies. Measure boot, idle memory and
   CPU, and two bursts of 100 CLI writes.

Scripts, all in `tests/e2e/` and all run on temporary databases only:

| Script | What it does |
|---|---|
| `app.sh` | start, wait, stop, memory and CPU |
| `live.py` | step 2 |
| `mcp_client.py` | step 3 |
| `real_launch.sh` | step 4, together with `app/src-tauri/tests/real_launch.rs` (`#[ignore]`) |
| `burst.py` and `scale.sh` | step 6 |
| `hold_hotkey.swift` | holds ⌃⌥N, for step 5 |

## Results

### 1. Build and boot: passed

- `pnpm tauri build` builds cleanly: the `.dmg` is 5.75 MB and the bundle 12.26 MB.
- The boot log has no `WARN` or `ERROR` lines. Sync, launch and attention all start, with no
  "Didn't start" row.
- The connection states are true for this Mac, once F2 is fixed:
  - T3 Code shows "Live state", and availability is `Ready { version: "0.1.12" }`.
  - GitHub shows "Checking", then "Signed in" about 8–13 s later. Both `gh` accounts are signed in.
  - Claude Code has no row, which means its sessions are readable.
  - Agents shows "Not set up". That's true: `~/.local/bin/airboss` doesn't exist.

### 2. Live updates across processes: passed

These numbers come from the final bundle, in `live.py`. A CLI write reaches the app as a
`tree:patch` in a median of **44 ms** (37–50 ms). `seq` rose strictly, from 2 to 14.

| Step | What happened |
|---|---|
| Add a parent and two children, then make B wait on A | Each write gives one patch with the right `created` ids; B shows as `todo:waiting`. |
| Link A to the real merged PR `<account-1>/airboss#1` | Sync looked it up with the second `gh` account and finished A itself, as `sync:gh`, after 9.1 s. B then became ready, as the Needs me item `ready:3:…`, and the menu bar redrew with the title "1". |
| An agent proposes 2 tasks over MCP | Both arrive as `proposed` after 41 ms. The Needs me `proposed` item shows after 241 ms, and the menu bar shows "2". |
| Reject one, accept the other | The rejected task is in `removed`, and the accepted one moves to `todo`. The proposal item clears after 190 ms. |
| `done` | Arrives after 43 ms. |

### 3. A real agent over MCP: passed

- **The scripted client** (`mcp_client.py`) worked through every call:
  - `initialize`: the instructions name `agent:t3/<id>`.
  - `tools/list` lists the 6 tools.
  - `list_tasks` and `get_task` work.
  - `link` with no ref gives `ref` = the thread id and `linked_by` = `agent:t3/<id>`.
  - `mine: true` lists only that task.
  - `update_task` moves it to `doing`, then to `in_review` with a note.
  - `propose_tasks` adds two `proposed` tasks.
  - `import_plan` with a real file creates 4 tasks with their dependencies. Sending it again
    creates nothing new and lists all 4 as `existing`.

  Each call takes 0.02–0.6 ms, and `initialize` 2.4 ms. The running app showed the whole session as
  a single patch, 78 ms after it started.

  Errors read well. Two examples: "There's no task #999999. List the tasks to find the right id.",
  and "Give `ref` for a slack link: the message's permalink." Bad arguments get rmcp's own message
  ("unknown variant `todo`, expected one of `doing`, `in_review`, `blocked`, `done`"). It's terse
  but exact.
- **The real Claude Code run** (Haiku: 6 turns, 7.5 s, $0.0037) used the identity
  `agent:t3/qa-claude-0001`. It linked the thread (`linked_by` the agent) and moved task 12 to
  `in_review` with its note, and the app showed both patches. It tried `done` with no note and was
  refused with "Say what you did in `note` when finishing a task.", which it quoted back word for
  word.

### 4. One real Do it: passed, with no change to launch

The test is `tests/e2e/real_launch.sh`, which runs `real_launch.rs`. It drives the app's own
`commands::launch::do_it` and `add_to_t3` on the real services, and records `t3`'s raw output.

1. **The first Do it.** `t3 thread start --json --title "1 QA throwaway: add one line to the README" --worktree main --branch task-1-qa-throwaway-add-one-line-to-the-readme <repo> -`
   (1,761 bytes on stdin) exited 1 with stderr
   `No T3 Code project matches '<repo>'. Pass a project id or a path inside a project.` Launch
   read that as `NotAProject` ("airboss-qa-throwaway isn't a T3 project yet."), wrote no thread and
   left the task as it was.
2. **Add to T3.** `t3 project add --title airboss-qa-throwaway <repo>` printed
   `Added project <project-id> (airboss-qa-throwaway) at <repo>.`
3. **Do it again.** This time stdout was
   `{"threadId":"mcp:<thread-id>","projectId":"<project-id>","runId":"run:thread:mcp%3A<thread-id>:ordinal:1"}`.
   That's the shape `t3cli.rs` expects. The thread id contains `:`, which `parse_start` allows, and
   the deep link percent-encodes it.
4. **The checks after the launch:**
   - The thread link is by `launch`.
   - The branch was set, by `launch`, before the thread started.
   - The task is `doing`.
   - Sync saw the thread as `Running` 254 ms later, with its branch already recorded.
     `find_thread(repo, branch)` resolved it at once.
   - The app's row chip was `Working`.
   - `thread_url` is `t3code://threads/<env>/mcp%3A<thread-id>`.
5. **An agent with the real id.** An MCP session with `T3CODE_THREAD_ID=mcp:<thread-id>` found the task
   with `mine: true`, and `link` with no ref returned launch's own link. Thread ids in a T3
   thread's environment have the same `mcp:` shape (this QA thread's own id is
   `mcp:launch:<thread-id>`), so the agent's identity matches launch's link.
6. **The thread itself** ran Claude (`claude-opus-5-5`) in
   `~/.t3/worktrees/airboss-qa-throwaway/<worktree-id>`. It committed
   one line (7dced9f) and finished on its own after about 2.5 minutes, as `completed`. The `t3` CLI
   can't stop a thread: its only subcommands are `list`, `start`, `send` and `show`. The agent said
   the Airboss tools weren't available, which is right for this Mac (see R5).

**To clean up in T3:** archive the thread `mcp:<thread-id>`, "1 QA
throwaway: add one line to the README". Then remove the project `airboss-qa-throwaway`
(<project-id>) with `t3 project remove`, and remove its worktree.

### 5. Failure paths

- **T3 missing, `gh` signed out, no Claude.** Launched with `HOME` set to an empty folder, the app
  boots in 633 ms. The footer reads "T3 Code: Not installed" and "GitHub: Not signed in", 3.4 s
  after launch once F4 is fixed. Before F4, it said "Checking" for over 90 s. Availability is
  `Missing { reason: "Do it and Plan it need T3 Code. Install it, then choose Check again." }`,
  which is what the UI uses to disable Do it, show its banner and say why
  (`DoItButtons.svelte`, `T3Banner`).
- **An empty `PATH` (`PATH=/nonexistent`).** Everything still works. `gh` is found at its fixed
  Homebrew path, and `t3`, `git` and `ps` are run by absolute path. That's right for an app started
  from Finder, so an empty `PATH` can't make `gh` go missing.
- **`gh` really missing.** I hid it with a `sandbox-exec` profile that denies
  `/opt/homebrew/bin/gh` and its Cellar folder. The app boots, and the footer reads
  "GitHub: Not installed". The sandbox also blocked `/bin/ps`, which showed the renamed
  "Claude Code: Sessions can't be read" row (F6) in the real app.
- **The database locked by a long write** (another connection held `BEGIN IMMEDIATE` for 8 s).
  - Before F3, every CLI command failed to *open* the database, reads included.
  - After F3, `airboss ls` answers in 10 ms. A CLI write and an agent's `update_task` each wait
    2 s and say "The Airboss database is busy. Try again in a moment."
  - The app logged nothing and missed nothing. The write made after the lock was released showed
    up as `seq` 2.
- **The WAL deleted while the app runs.** The watcher saw the folder change and kept catching up,
  and the app didn't crash. But SQLite then refuses every new connection with "disk I/O error",
  and the writes that hadn't been checkpointed were lost when the app quit. See R2.
- **Wake.** A real sleep can't be scripted, so with the trace on, `airboss-app --trace-wake` (a
  second launch) sends the same `Trigger::Wake` that the wake observer sends. The log shows
  `catch_up causes=wake`, then the services' own catch-ups: launch checked T3 again and sync read
  again. There were no warnings and it stayed a single instance. A new test checks that a wake
  tells all three services and then catches up.
- **The hotkey taken.** `hold_hotkey` registered ⌃⌥N through Carbon, then Airboss started.
  Airboss's own registration *succeeded*: macOS gave no error. So the new "Quick add: Shortcut
  taken" row (F7) only appears when the registration really fails. Which app gets the key in that
  case needs a person to press it.

### 6. Performance at scale

These figures use 2,000 tasks and 1,000 dependencies, from the `seed` example. The table at the end
has the full measurements.

- **Cold start:** a median of 494 ms over 10 launches, from `open` to `ui_ready`.
- **Idle CPU:** 0.00% over 60 s.
- **A single CLI write:** 58–64 ms from the write to its patch.
- **100 sequential writes:** 295 ms in all, with 0 failures. They arrived as 2 patches, and the
  last one showed 58 ms after the burst ended.
- **100 writes, 8 at a time:** 92 ms, with 0 failures. They arrived as 1 patch, 61 ms after the
  burst ended.

## Findings

### Fixed (each with a test that failed first, except F5's configuration)

| # | Bug | Commit |
|---|---|---|
| F1 | A first run with `AIRBOSS_DB` set (QA, `measure.sh`) wrote `~/Library/LaunchAgents/Airboss.plist` for that build. The LaunchAgent can't carry `AIRBOSS_DB`, so at the next login it opens the **real** database from a worktree build. **One such plist, left by the app-backend measurements, is on this Mac now:** run `rm ~/Library/LaunchAgents/Airboss.plist`. | 7ff44e1 |
| F2 | For the first ~10 s after launch, the footer said "3 connections need setup": T3 Code showed "Not installed" before sync's first read, and GitHub's "Checking" counted as a problem. | e605545 |
| F3 | `Store::open` took the write lock (one `BEGIN IMMEDIATE` per migration) even when every migration was already applied. So while another process wrote for over 2 s, every CLI command, `airboss ls` included, and every agent's `airboss mcp` start failed with "database is locked". | b7614e1 |
| F4 | A sync health change alone (`gh` signed out, T3 missing) never bumped `Changes`, so the app never re-read the connections: the footer stayed on "GitHub Checking" until something unrelated changed. | 45518cb |
| F5 | The log plugin's default deletes `app.log` at 40 KB (`KeepOne`), and the whole history went with it, mid-QA. It now rotates at 1 MB and keeps 3 files. | 5c0f97d |
| F6 | (Coordinator) Sync named its Claude sessions row "Agents". That merged with the app's own "Agents" row (the CLI), so the "Claude Code" row never showed. | 60547dd |
| F7 | (Coordinator) A hotkey taken at start-up was only logged. Now a "Quick add" row in the connections footer says "Quick add shortcut ⌃⌥N is taken by another app. Change it in Settings.", and choosing a key that works clears it. No UI change was needed. | 8a14a27 |

Also added: the trace (fae687b, c00be98) and a simulated wake with a test of the wake wiring
(fcb1c44). `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings` and
`cargo test --workspace` are clean: 958 passed, 0 failed, 20 ignored.

### Reported (not fixed)

- **R1. The service crates' diagnostics are lost** (owner: app backend). `airboss-sync:` and
  `airboss-launch:` lines go to stderr, which is `/dev/null` when the app runs from Finder. One
  example: "Claude sync broken: ps failed" was visible only because I captured stderr. Either
  redirect stderr into `~/Library/Logs/Airboss/`, or give the services a log hook.
- **R2. Deleting the live `-wal` loses data** (owner: backend, low priority). After the delete,
  new connections fail with "disk I/O error", so the CLI and agents can't open the database, and
  the un-checkpointed writes vanish when the app quits. The footer says nothing. Suggestion: treat
  a WAL removed while the database is still open as a "Database moved"-style row.
- **R3. Cross-process catch-up costs 17–21 ms at 2,000 tasks** (owner: core and app). In-process,
  `tests/perf.rs` measures 5.8 ms. Another process's commit throws away the app's page cache, so
  `snapshot()` reads the file again. That's still far under the 300 ms target. `PRAGMA mmap_size`
  is worth measuring.
- **R4. Idle memory at 2,000 tasks is 131 MB** (136 MB at 2,306 tasks), just over the revised
  130 MB target (owner: EM decision). Small databases use 104–113 MB. WebKit's GPU process briefly
  reaches about 68 MB after a burst of UI activity, then drops back to 14 MB within about a minute.
- **R5. Agents can't report on this Mac yet** (product and attention). T3 agents had no Airboss
  tools, because `airboss mcp` isn't set up here ("Agents: Not set up"). That's expected until the
  setup step is done. And when a run ends with no PR, the task stays `doing` with a "Stopped" chip,
  and nothing reaches Needs me until it has been Quiet for 5 days.
- **R6. A freshly copied bundle's first launch takes 1.0–1.5 s**, because LaunchServices
  registers it and checks its signature. Later launches take 465–506 ms.
- **R7. A note for the UI owner:** `conn.background` is set by `error:background` and never
  cleared. That's fine today, because every source of it is permanent, but a transient source
  added later would stay on screen.
- **R8. Log times are in UTC**, the plugin's default, so `21:41` in the log is 03:11 local.

No UI bugs were observed: none could be seen without the screen.

## Measurements (final bundle, at commit 5c0f97d)

| Target | Measured |
|---|---|
| Download under 15 MB | `.dmg` 5.75 MB, bundle 12.26 MB |
| Cold start under 1 s | 2,000 tasks: median 494 ms over 10 runs (465–506; the first launch of a new copy took 1,001 ms). Empty database, first launch: 1,529 ms. |
| Idle memory under 130 MB | 112–113 MB after boot on an empty database. 131 MB at 2,000 tasks, 136 MB at 2,306 tasks. |
| Idle CPU near 0% | 0.13% (empty database), 0.00% (2,000 tasks, 60 s). WebKit processes 0.00%, with Needs me items showing. |
| Agent write visible under 300 ms | Small database: 44 ms median. 2,000 tasks: 58–64 ms. |
| Burst of 100 CLI writes | Sequential: write to patch median 111 ms and p95 185 ms; 2 patches; catch-up 17–22 ms per batch. 8 at a time: 0 failures, 1 patch, p95 147 ms. |
| MCP call under 20 ms (p95) | 0.02–0.6 ms per call |
| Needs me and menu bar after a write | Needs me 190–241 ms, menu bar about 250 ms later (debounced) |
| A merged PR finishes its task | 9.1 s after the link (sync's lookup through the second `gh` account) |

## Still needs a person with eyes on the screen

1. **The menu-bar item.** Its count matches Needs me (1, then 2, then 1 in step 2), its menu lists
   the items, and choosing one opens Needs me on it.
2. **⌃⌥N.** It opens quick add over another app, and Esc hides it. With `hold_hotkey` running, see
   which app gets the key.
3. **The native menus:**
   - ⌘Z with its undo label
   - ⌘W
   - Install Command-Line Tool, then Copy Agent Note
4. **The first-launch footer.** There's no orange flash, only "Agents not set up", and the
   popover's fix text reads well. With `HOME` set to an empty folder, Do it is disabled and shows
   the T3 banner and its reason.
5. **The launched thread's row.** Its chip goes from Working to Stopped, and Open thread opens T3 at
   the thread.
6. **Appearance and windows.** Dark and light, both windows; the window position is restored; and
   no "reopen windows?" alert appears after `kill -9`.
7. **A real sleep and wake.** Close the lid for a minute, open it, make a CLI write, and check it
   shows and the footer is still live.
8. **Scrolling.** 2,000 rows scroll at 60 fps (`tests/e2e/scale.sh` leaves the database).
