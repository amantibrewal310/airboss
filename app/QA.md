# Manual QA

What the automated tests can't reach: the real app, its native menus, macOS itself, and a
human at the keyboard. About 15 minutes, **keyboard only** (no mouse or trackpad, except where a
step says so), on a throwaway database. Tick each box as you go; anything that doesn't match
the expected result is a bug to file.

## Before you start (2 minutes, once)

```sh
cd app && pnpm install && pnpm bundle --bundles app
mkdir -p /tmp/airboss-qa/scratch && cd /tmp/airboss-qa/scratch && git init -q && cd -
cargo run -p airboss-app --example seed -- --tasks 2000 --db /tmp/airboss-qa/big.db
```

- Turn on **System Settings › Keyboard › Keyboard navigation**, so Tab reaches buttons.
- Start the app on an empty throwaway database, with the trace on (for the sleep check):

  ```sh
  AIRBOSS_DB=/tmp/airboss-qa/airboss.db AIRBOSS_TRACE=1 \
    ../target/release/bundle/macos/Airboss.app/Contents/MacOS/airboss-app
  ```

- Put `~/.local/bin` on your `PATH` in `~/.zshrc` if it isn't, and open a new Terminal tab.
- Do the whole list in **Dark** (System Settings › Appearance). At the end, switch to **Light**
  and do the last section.

## 1. First run and notifications permission

- [ ] The window opens on "Nothing here yet" with no white flash. "Notify me when an agent asks
      something or stops" is ticked. ("Open Airboss at login" is unticked: a run on a throwaway
      database never registers a login item.)
- [ ] Notifications need a bundle identifier and a signature. Check them once:
      `codesign -dv ../target/release/bundle/macos/Airboss.app 2>&1 | grep -E 'Identifier|Signature'`
      prints `Identifier=io.github.amantibrewal310.airboss` and `Signature=adhoc`.
- [ ] macOS asks whether "Airboss" may send notifications (a banner with **Allow**, or with
      **Options › Allow**). Allow it. Airboss now appears in System Settings › Notifications,
      with Allow notifications on. macOS asks once per Mac: if Airboss was already listed
      there, there's no prompt, and the switch there is what counts.
- [ ] In the Repos step, Tab to **Add another folder…**, ↩, choose `/tmp/airboss-qa/scratch`.
      It's added.
- [ ] Tab through the Connections rows: **Agents** reads "Not set up" with **Install
      command-line tool**. Press it (↩). The row then reads "Agents · Ready" and "No agent has
      reported yet."

## 2. Quick add from another app (⌃⌥N)

- [ ] Switch to Finder (⌘Tab). Press **⌃⌥N**. The panel appears at once, over Finder, with the
      cursor in the title; the Airboss main window does **not** come forward.
- [ ] Type `QA one`, ↩. The panel hides; Finder still has focus. ⌘Tab to Airboss: "QA one" is
      in All tasks.
- [ ] **Hotkey taken:** quit Airboss (⌘Q). In the Shortcuts app, give any shortcut the keyboard
      shortcut ⌃⌥N (its Info panel › Add Keyboard Shortcut). Start Airboss again with the command
      above. The sidebar footer reads "Quick add shortcut taken" (orange). Open ⌘, and record
      ⌃⌥K: it's accepted and the footer clears. Record ⌃⌥N back after removing it from Shortcuts.

## 3. Native ⌘Z and the Task menu

- [ ] In All tasks (G A), select "QA one" (↑↓), press **Space**. It's done, and a toast offers
      Undo. Open the **Task** menu (⌃F2, with fn on a laptop keyboard, then → to Task): it has
      "Undo Mark done <id>".
- [ ] Esc out of the menu, press **⌘Z**. "QA one" is todo again, once (not undone twice).
- [ ] Press ↩ to rename, type a few letters, press **⌘Z**: only the typing is undone (the
      field's own undo), and the task's status doesn't change.

## 4. Do it and Plan it on the scratch repo

- [ ] Select "QA one", press **R**, ↩ on `scratch`, Esc. Press **⌘↩**. The button
      reads "Starting…", then a toast says "Started a thread for <id> in scratch" with
      **Open thread**.
- [ ] Press F6 to reach the toast, ↩ on **Open thread**. T3 Code comes forward **on that
      thread** (not just the app).
- [ ] Back in Airboss, the row shows the live chip "Working" within a few seconds.
- [ ] Create `QA plan` (⌘N, type, ↩). Link `scratch` (R). Press **⇧⌘↩** (Plan it). The toast
      says "Started a planning thread…". In T3, let it propose 2–3 steps and settle the plan:
      the steps arrive under "QA plan" as proposals in **Needs me** (G N), and ↩ accepts them.

## 5. Notifications

- [ ] In the "QA one" thread in T3, ask the agent: *"Before you change anything, ask me which
      colour to use, and wait."* Switch to Finder **before** it asks.
- [ ] A banner appears titled "<id> QA one", with the agent's question as the text and the
      default sound. It appears **once**. `grep 'notification' ~/Library/Logs/Airboss/app.log`
      shows "posted 1 notification(s)" and no "macOS didn't take a notification".
- [ ] Click the banner (mouse allowed). Airboss comes forward on **Needs me** with that question
      selected.
- [ ] Have the agent fail or stop on the same task too (or ask a second question): open
      Notification Center (click the clock). That task's notifications sit together in one
      stack, under Airboss.
- [ ] Quit Airboss (⌘Q). In Notification Center, click one of its notifications: Airboss
      starts and opens on Needs me.
- [ ] Ask again in the thread with Airboss in front and focused: **no** notification this time.
- [ ] ⌘, › Notifications: switch it off (Tab to it, Space). Answer the agent in T3 and have it
      ask a second question with Finder in front: no notification. Switch it back on.
- [ ] **Denied:** in System Settings › Notifications › Airboss, turn **Allow notifications** off.
      Back in Airboss, ⌘, shows "macOS has notifications turned off for Airboss…" with **Open
      System Settings**; ↩ on it opens that pane. Turn it back on there; returning to Airboss,
      the note is gone.

## 6. The menu-bar menu and count (screen 08)

- [ ] With the question waiting, the menu-bar item's count (next to the Airboss icon) went up by
      one, and matches the Needs me badge in the sidebar.
- [ ] Open it (⌃F8, then ← or → to Airboss's item, ↓). It lists "Needs me N", up to six items
      with a red icon and a clock time ("10:32"), then "Open Airboss ⌘0", "Quick Add… ⌃⌥N",
      "Quit Airboss". Choosing the question opens Needs me with it selected.
- [ ] Capture it with `screencapture -i` in a dark and a light menu bar and compare with
      `docs/design/screens/08-menu-bar.png`.
- [ ] Dismiss every item (⌘⌫ in Needs me, accepting or rejecting proposals). The count
      disappears and the menu says "Nothing needs you".

## 7. VoiceOver (⌘F5)

- [ ] In All tasks, ↓ onto a row. VoiceOver reads it like "<id>, QA one, In progress, agent
      working, scratch, 2 minutes ago", and says it's in a tree, with its level. The menu-bar
      item reads "Airboss, N waiting on you" (⌃F8 to reach it).
- [ ] In Needs me (G N), ↓ through the items: each reads its kind, task and headline; ↩ runs the
      main action. Toasts are announced. Turn VoiceOver off (⌘F5).

## 8. Links never run code

- [ ] In Terminal: `printf '#!/bin/sh\nopen -a Calculator\n' > /tmp/airboss-qa/x.command &&
      chmod +x /tmp/airboss-qa/x.command`, then
      `airboss link <QA one id> doc /tmp/airboss-qa/x.command --url 'file:///tmp/airboss-qa/x.command?a'`.
- [ ] In Airboss, select "QA one", Tab into its panel to the link's tag under Links, ↩. A toast
      says "Airboss only opens https links, files and T3 threads." **Calculator does not
      open.**
- [ ] `airboss link <id> doc /tmp/airboss-qa/x.command` (no `?a`), open it: Finder shows the
      file selected, and Calculator still doesn't open.
- [ ] `echo '# Plan' > /tmp/airboss-qa/plan.md && airboss link <id> doc /tmp/airboss-qa/plan.md`,
      open it: it opens in **TextEdit** (whatever app usually opens `.md` files on this Mac).

## 9. A real sleep and wake

- [ ] Close the lid (or  › Sleep) for at least a minute, then wake the Mac and unlock it.
- [ ] Within a few seconds: `grep 'causes=.*wake' ~/Library/Logs/Airboss/app.log | tail -1`
      shows a line from just now, and the footer is unchanged (no "Live updates stopped").
- [ ] `airboss done <QA one id> --note "after wake"` in Terminal: the row shows Done in the open
      window within a second.

## 10. 2,000 rows, and window restore

- [ ] Quit (⌘Q). Start the app on the big database:
      `AIRBOSS_DB=/tmp/airboss-qa/big.db ../target/release/bundle/macos/Airboss.app/Contents/MacOS/airboss-app`.
      It opens in under a second.
- [ ] G A, then hold ↓ for 5 seconds, then hold ↑: the selection keeps up, with no blank rows
      or stutter. (Optional: Develop › Show Web Inspector › Timelines shows no frames over
      16 ms while scrolling.)
- [ ] Move and resize the window (mouse allowed). Quit and start it again: the same size and
      position. Quick add (⌃⌥N) still opens 22% from the top of the screen, not where it last
      was.

## 11. Light mode (1 minute)

- [ ] ⌘, › Appearance › Light. Both windows switch at once with no flash; the background behind
      the web view is white.
- [ ] Glance at Needs me, All tasks with the panel open, Settings and quick add (⌃⌥N): text is
      readable, nothing is clipped, red appears only for needs-me. Switch back to System.

## Done

- Note the macOS version and Mac model here, with the date, and file anything that failed.
- Remove the scratch data: `rm -rf /tmp/airboss-qa`, delete the QA thread and project in T3,
  and turn off "Open at login" if a throwaway run turned it on (⌘,).
