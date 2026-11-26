# Hands-on QA of the real app (26 Nov 2026)

The first time the built app was driven on screen: `app/QA.md` run through by keystrokes and clicks,
with screenshots and the macOS accessibility tree as evidence. It used a throwaway database and a
scratch repo, a real T3 thread (Do it, then four agent questions) and a real Plan it.

Machine: macOS 27 (Darwin 27.2), external 2560×1440 display at 1x, **Light** appearance, Keyboard
navigation **off** (the macOS default).

## What passed

- **First run:**
  - "Nothing here yet", "Open Airboss at login" unticked on a throwaway database, notifications ticked.
  - The folder picker ("Add a repo" sheet) works.
  - Install command-line tool creates `~/.local/bin/airboss`, and the footer reads "All connected".
- **Tab and quick add:**
  - Tab reaches buttons even with Keyboard navigation off.
  - ⌃⌥N from Finder shows quick add in about 12–32 ms, 22% from the top of the screen, with Finder still the active app.
  - ↩ saves the task and hides the panel; Esc discards the draft, and the panel reopens empty.
- **Undo and the Task menu:**
  - Space marks a task done, with an Undo toast, and the Task menu gets "Undo Mark done 1".
  - ⌘Z undoes it exactly once, with one status event back.
  - While renaming, ⌘Z undoes only the typing.
- **Do it:**
  - On a repo T3 doesn't know, Do it shows "Couldn't start 1: scratch isn't a T3 project yet. · Add to T3 and retry", and F6 focuses the action.
  - "Add to T3 and retry" starts the thread: the toast reads "Started a thread for 1 in scratch · Open thread", the task moves to In progress, and the "Agent working" chip and branch `task-1-qa-one` appear.
  - ⌘O brings T3 forward on exactly that thread, and a `t3code://` link from outside T3 works too.
- **Plan it:** the toast reads "Started a planning thread for 2. Its steps become subtasks when you settle the plan.", and the task moves to In progress with the thread linked.
- **Notifications:**
  - A real agent question with Finder in front posts one banner, titled "1 QA one" with the question as its text.
  - Clicking it in Notification Center opens Needs me with the question selected.
  - Answering in T3 clears Needs me and the menu-bar count within a second.
  - No banner while Airboss is in front.
  - No banner with Settings › Notifications off, though the menu-bar count still goes up.
- **Menu-bar menu:** it lists "Needs me 1", the item with its time, Open Airboss ⌘0, Quick Add… and Quit. Choosing the item opens Needs me with it selected. After ⌘⌫ dismiss, it reads "Nothing needs you" and the count is gone.
- **Links:**
  - `file://…x.command?a` is refused with the toast "Airboss only opens https links, files and T3 threads."
  - A plain `.command` path is revealed in Finder, selected, and not run.
  - `plan.md` opens in TextEdit. Calculator never opened, and the marker file was never created.
- **Performance and window restore:**
  - 2,000 tasks: `ui_ready` in 619 ms. Holding ↓ through 150 rows, the selection keeps up with no blank rows.
  - The window comes back at the same position and size (300,200, 1280×800), and quick add still opens 22% from the top.
  - Writes from the CLI show in the open window within about 65 ms.
- **Appearance:** Dark from Settings switches instantly with no visible flash, and Dark matches the design.
- **Agent stopped:** it appears exactly 10 minutes after the thread stops.

## Not covered

- **A real sleep and wake:** it would sleep the user's Mac.
- **"Hotkey taken":** macOS lets a second process register ⌃⌥N without Airboss's registration failing, and the key still reaches Airboss. The Shortcuts-app case wasn't tried, because it means editing the user's shortcuts.
- **Clicking a notification while Airboss is quit:** LaunchServices would start it on the real database.
- **macOS-denied notifications:** it means changing System Settings.
- **VoiceOver speech itself:** the accessibility tree was read instead.

## Findings

Severity: **B** = bug a user hits, **U** = UX/polish, **A** = accessibility, **N** = nit.

| # | Sev | Finding | Where |
|---|---|---|---|
| 1 | B | The main window shows **two sets of traffic lights**: the sidebar's fake ones (drawn for the browser mock) stay, because `App.native` is a plain field set after `mount`, so `{#if !app.native}` never re-runs. | `app/src/windows/main/Sidebar.svelte:44`, `app/src/lib/state/app.svelte.ts:96,143`, `app/src/main.ts` |
| 9 | B | **Agents can't reach Airboss after first run.** The Agents row reads "Ready" once the CLI is installed, but nothing registers the MCP server. `claude mcp add …` appears only in Settings. The real Do it agent replied: "Airboss tools aren't available." The core loop silently doesn't work for a new user. | `app/src-tauri/src/connections.rs:105`, `FirstRun.svelte`, `Settings.svelte` |
| 10 | B | **The footer turns orange ("Claude Code sessions can't be read") whenever any Claude session waits on the user.** Claude Code 2.1 writes `"status":"waiting"` (with `waitingFor`), and `state_of` knows only busy/idle, so Claude sync goes Broken. | `crates/sync/src/claude.rs:113` |
| 7 | B | **The same footer flashes orange for about 1.4 s whenever a Claude session starts.** The new `<pid>.json` is read before Claude writes `status`, and the missing field marks Claude sync Broken. | `crates/sync/src/claude.rs:83` |
| 19 | B | **Quick add's Repos and Parent pickers are clipped** by the 680×221 panel window. Only the first row shows, cut in half, so you can't see or pick most repos or parents from quick add. | `app/src/windows/quickadd/`, `app/src/windows/shared/NewTask.svelte`, quick-add window size |
| 23 | B | **"Agent stopped" didn't notify.** `Memory::decide` sets `launching = !self.settled` before applying the SETTLE timeout, and it only runs when the Needs version changes. So the first Needs change after a launch counts as "launching", and an item whose `since` predates the launch is swallowed. That's exactly Agent stopped after a restart (its `since` is the stop time). | `app/src-tauri/src/notify.rs:93` |
| 24 | B | The Agent stopped card's five actions overflow the card at a 1280 px window, and "Dismiss" is clipped. | `app/src/windows/main/NeedsCard.svelte` |
| 16 | A | **The task list isn't a tree for VoiceOver.** WebKit exposes All tasks as AXGroup with AXGroup rows, not an outline with rows, so there's no "tree", level or position. Likely cause: VirtualList's wrapper divs (`.space`, `.slot`) between `role=tree` and the `treeitem`s have no role, so WebKit judges the tree invalid. | `app/src/lib/components/VirtualList.svelte:98`, `TaskList.svelte` |
| 18 | A | Segmented control (Appearance, Tree/By status): ←/→ change the value, but focus stays on the old option, so VoiceOver keeps reading "System, unchecked". | `app/src/lib/components/Segmented.svelte` |
| 11 | U | Activity shows raw thread ids: "Started thread mcp:9f5361a5-…-081f286d7495 in scratch". The design shows the thread's name ("Started thread slot-picker in booking-web"). | Activity rendering |
| 17 | U | "now ago": the card header reads "Question · now ago · 1 QA one", and the thread line "stopped now ago". The relative-time formatter returns "now" and callers add "ago". | time formatting + callers |
| 6 | U | An error toast whose fix is its action ("Couldn't start 1… · Add to T3 and retry") disappears after 5 s, like any action toast. An error with a fix should stay until it's used or dismissed. | `app/src/lib/state/toasts.svelte.ts:21` |
| 3 | U | First run: adding one folder ("Add another folder…") hides the whole Repos step at once, so any ticked T3 projects are lost, and focus drops to the document (the next Tab starts at the sidebar top). | `FirstRun.svelte:46`, `AddRepos.svelte` |
| 2 | U | First run: each repo row is its own Tab stop. With 25+ T3 projects, a keyboard user Tabs about 30 times to reach "Add another folder…" and Connections. | `AddRepos.svelte` |
| 4 | U | Quick add in Light has no shadow (`shadow: false`, the card fills the window), so over a white window it blends in. | `tauri.conf.json` quickadd, `QuickAdd.svelte` |
| 12 | U | A notification stays in Notification Center after its question is answered. Withdraw delivered notifications when their Needs me item goes away. | `app/src-tauri/src/macos.rs`, `notify.rs` |
| 22 | U | Plans shows "No plans yet" while a Plan it thread is running, so Plan it has no visible result there. Show tasks being planned as "Planning…" cards. | `PlansGrid.svelte` + data |
| 20 | A | Menu-bar item: VoiceOver gets the title "1" with role "status menu". "Airboss, 1 waiting on you" is only the tooltip (help). Set the button's accessibility label. | `app/src-tauri/src/tray.rs` |
| 8 | N | At start the log shows "T3 Code off (Can't open threads)" for an instant before "ok". Make sure it can't reach the UI. | `catchup.rs`, connections |
| 13 | N | The menu-bar menu's "Quick Add…" shows no ⌃⌥N, and items have no kind icon (design 08 has both). | `tray.rs` |
| 15 | N | "Nothing needs you" offers "Show ready tasks" even with 0 ready to start. | `NeedsEmpty` |
| 21 | N | The status menu (S) opens with Todo highlighted instead of the current status. | status popover |
| 5 | N | Task menu: single-key shortcuts are written into the item names ("Mark Done  Space"), because native key equivalents would steal typing. Kept as is. Toast "Marked 1 done" reads like "marked one done": use "#1". | `menu.rs`, toasts |
| 14 | N | Needs me header "checked 08:03" stays put while new items arrive; it's sync's last full round. Kept as is. | – |

## EM decisions

- **#9:** the Agents row checks whether an agent tool actually has the `airboss` MCP server. It reads `~/.claude.json` (top-level `mcpServers.airboss`) and `~/.codex/config.toml` (`[mcp_servers.airboss]`) **read-only**. "Ready" needs the CLI **and** at least one tool. With the CLI but no tool, the state is "Not connected", with a one-click **Connect Claude Code**: on the user's click it runs `claude mcp add --scope user airboss -- ~/.local/bin/airboss mcp`. "Copy setup command" stays as the fallback. A change to the user's agent config happens only on that explicit click.
- **#5 and #14:** kept as they are.
- **#22:** do it if the data is already there (launch knows which starts were Plan it). Otherwise defer, with a note.

## After the fixes (wave 8, same day)

Five engineers fixed the findings on branches `wave8/*`, merged into `main`. The rebuilt app was then driven on screen again. Re-checked there:

- **#1:** there's one set of traffic lights.
- **#2 and #3:** the project list is one Tab stop (↑↓, Space), with names as labels and paths as help. After adding a folder, the Repos step stays and focus returns to "Add another folder…".
- **#9:**
  - The rows go "Not set up" → "Not connected" → **Connect Claude Code** → "Ready" with the toast "Connected Claude Code".
  - The connect check ran with a throwaway HOME: it wrote `mcpServers.airboss` with the absolute CLI path to that HOME's `.claude.json`, in about 2 s.
  - With the real HOME, the row reads "Not connected", which is correct. The user's real config wasn't touched.
- **#7 and #10:** with an agent waiting on an AskUserQuestion, Claude sync never broke ("Claude sync broken" 0 times), and no Claude Code row appeared.
- **#12:** answering the question logged "withdrew 1 notification(s)".
- **#16:** All tasks is an AXOutline with AXOutlineRow rows, read as "1, QA one, In progress, agent stopped, needs you, scratch, 50 minutes ago".
- **#18:** focus follows ←/→ in Tree/By status.
- **#21:** the status menu opens on "In progress".
- **#5 and #6:** "Couldn't start #4: scratch2 isn't a T3 project yet." stays after 10 s with **Close**. F6 then Esc closes it and returns focus to the list.
- **#11:** "Started thread Plan 2 QA plan in scratch".
- **#17:** "Agent stopped · 42m ago".
- **#24:** the actions wrap, with Dismiss on its own line.
- **#13:** the menu-bar menu reads "Quick Add… ⌃⌥N", and the hotkey still works.
- **#20:** the menu-bar item's AX description is "Airboss, 2 waiting on you".
- **#19 and #4:** both pickers show in full, and quick add has a visible shadow in Light.

Found during the re-check and fixed by the EM:

- **Quick add stale data:** quick add loaded its repos and parents only at start and after hiding, so the first open after adding a repo or task showed old lists. It now reloads whenever it gains focus. A test covers it.
- **Ghost outline under quick add's pickers:** AppKit's window shadow traced the picker's blurred CSS shadow. In quick add, pickers and the Start in menu now get a ring, like the card.
- **Wording:** the install toast now shows `~/.local/bin/airboss`, and the Agents hint says "then connect Claude Code".

Not done:

- **#22 (Plans "Planning…" cards):** no field tells the UI that a Plan it thread is running. It needs a small backend addition, so it's left for v1.1.
- **#5's menu item names and #14:** kept as they are.
- **Bare ids in launch errors:** the launch crate's own messages ("Task 1 waits on 2.") still use them.
