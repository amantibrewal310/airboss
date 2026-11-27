# Changelog

What changed in each release of Airboss. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html): alphas are `0.1.0-alpha.N`, with macOS build number `N`. Each release's page on GitHub has the install steps and the .dmg.

## [Unreleased]

### Changed

- **A task with subtasks has no status of its own.** It shows what its subtasks add up to: done when every one is done or dropped, dropped when all are, blocked when any is (naming which), in progress when any is under way or some are finished, otherwise todo. A plan with 8 of 38 steps done now shows in progress, not done. Its old status is ignored, not changed.
- **A parent's panel leads with what needs you.** One line says where it stands ("6 of 19 done · 2 ready · 4 to review"), then **Needs you** (each item with its own button), **Ready now** with Start ready, **In progress**, **Next** and **Done**, folded. Finished steps no longer sit above the ones that can start. Proposals show one by one with Accept and Reject, what they'd follow or block when known, and Accept all only for the ones one agent proposed together. The notes come next, with links folded under them; there's no Status or Branch row.
- **No more "waves".** The full page's switch is List / Map, the map's columns read 1st, 2nd, 3rd, and plan cards drop their Wave row.
- **Nothing finishes a parent by hand any more.** The status menu, Space and ⌘B offer only Drop on a parent, which drops its open subtasks. A parent takes no branch or PR of its own, so a merged PR linked to it long ago can't finish it; its PRs go on its subtasks.

### Removed

- **"Finish it?" in Needs me**, and on plan cards. A parent is done when its subtasks are.

## [0.1.0-alpha.7] - 2026-11-26

### Added

- **What's left for you, on the task.** Once an agent has finished its turn, its card under **Threads** shows what its last reply left for you: the part after "Next on your side:", "To bring to the meeting:", "For you to decide" and the like, or its closing paragraph. **Full reply** shows the whole reply, with its links. While the agent is working the card shows nothing, since its last reply is about the turn before. For T3 Code threads.

### Changed

- **Clearer briefs for Do it and Plan it.** A brief now opens with what to do and the task itself, then where to work, what done means (a pull request, the checks run, asking before a fix that needs your decision) and the few Airboss rules for that task. Agents are asked to end with **Next on your side:**, which is what the Threads card shows. The general rules, such as when to set In review and never marking done a task that has a PR, now come from the Airboss tools themselves, so agents you start yourself get them too.

## [0.1.0-alpha.6] - 2026-11-26

### Added

- **Start ready.** On a task with subtasks, such as a settled plan or one of its phases, Do it becomes **Start ready (n)**: it starts an agent on each step that can start now, each with its own thread, worktree and branch, and says what started in one toast. A step under a phase that still waits on another isn't started. A step with no repo is left for you to start. **Do it as one thread** (right-click menu and ⌘K) still hands the whole task to one agent, which is now told to report on each step rather than on the parent.
- **Add a repo from Do it.** When a task has no repo, the Start in menu offers to add a T3 project the task's title or notes name and start there (**Add hub-ui and start** for `hub-ui: fix 7 tests`), and **Add another repo…**. Agents can't add repos, so they're now told to name one that isn't registered this way.

### Changed

- A thread that has finished its turn and waits for your reply shows **Your turn** instead of **Stopped**, and its line reads "replied 2m ago". Stopped now means it failed or was settled.
- The avatar for Do it and Plan it says where the agent runs: **T** for T3 Code, **H** for Herdr, in place of "L" for Launch. The activity list reads "T3 moved this to In progress".
- The activity list folds the same event on several subtasks into one line, so importing a plan reads "33 subtasks · Plan created" rather than 33 lines. A plan import's note now reads "Imported from implementation-plan.md", and older notes show the same way.
- An agent whose thread Airboss doesn't know reads "An agent", never a raw thread id.

## [0.1.0-alpha.5] - 2026-11-26

### Added

- **Search in Add repos.** With more than seven folders, a search field at the top filters them by name or path as you type. It opens with the cursor in it; ↓ goes into the list, ticks are kept while you search, and Esc clears it.

### Changed

- The connections list (click the line at the foot of the sidebar) now says what it is: "Connections: the tools Airboss works with. Green ones work; orange ones need setting up." T3 Code and Herdr read "Connected" instead of "Live state", and a T3 Code problem reads "Not updating since 14:02".

## [0.1.0-alpha.4] - 2026-11-26

### Added

- Updates inside the app. When a new version is out, the foot of the sidebar says so, and **Update and restart** downloads it, checks its signature, puts it in place and restarts, with the window showing. **Airboss › Check for Updates…** looks right away, and **Settings › Check for updates automatically** (on by default) looks every 6 hours. Each check reads one small file from GitHub.

### Fixed

- Add repos with many T3 Code projects no longer runs past the bottom of the window: the list scrolls inside it, and **Add another folder…** and **Add repos** stay in view. A folder's path shows as a tooltip only when it's cut off, so it no longer covers the next row.

## [0.1.0-alpha.3] - 2026-11-26

### Fixed

- "Claude Code sessions can't be read" no longer appears when a Claude Code session has a background shell task running. Claude Code reports that as the session status `shell`, which now shows as Working. A status Airboss doesn't know yet shows that session as idle, and every other session keeps working.

### Changed

- Plainer wording. Connections, Settings, the task panel, the Plans view and the New task window say what happens instead of assuming you know the Do it and Plan it buttons: "Agents you start on a task open here, each in its own tab", or "Starting agents needs T3 Code". Hovering Do it or Plan it says what each one does.
- Finder's Get Info shows the version Apple's way, `0.1.0`, with the build number. About Airboss shows the full version: `0.1.0-alpha.3 (3)`.

## [0.1.0-alpha.2] - 2026-11-26

### Added

- Herdr. **Settings › Agent setup › Start agents in** picks T3 Code or Herdr; **Automatic** uses T3 Code when it's there, else Herdr. **Agent in Herdr** picks Claude Code or Codex.
  - Do it opens a tab in the repo's Herdr space, in a git worktree under `~/.herdr/worktrees`, with the task's brief as the agent's first message. Plan it opens a planning tab.
  - The task's chip follows the agent: Working, Waiting on you, Stopped. Needs me says what it asks for, including Claude Code's "trust this folder" prompt.
  - Open thread brings Herdr forward on that tab.
  - On first run without T3 Code, Airboss offers your Herdr spaces' folders as repos.

### Changed

- The Codex setup Airboss copies also passes `HC_TERMINAL_ID`. With an older setup, the Agents row says so.

### Fixed

- A Claude Code session waiting on you is saved and shown. Before, it was dropped.

## [0.1.0-alpha.1] - 2026-11-26

### Added

- The first alpha: tasks and subtasks, quick add from any app (⌃⌥N), Needs me, Do it and Plan it in T3 Code, the `airboss` MCP server and CLI for agents to report through, GitHub PR state, a menu-bar icon and macOS notifications.
- A one-line installer that checks the .dmg's SHA-256 and signature.
- Help › Report a Problem…, and an alpha feedback form.

[Unreleased]: https://github.com/amantibrewal310/airboss/compare/v0.1.0-alpha.7...HEAD
[0.1.0-alpha.7]: https://github.com/amantibrewal310/airboss/compare/v0.1.0-alpha.6...v0.1.0-alpha.7
[0.1.0-alpha.6]: https://github.com/amantibrewal310/airboss/compare/v0.1.0-alpha.5...v0.1.0-alpha.6
[0.1.0-alpha.5]: https://github.com/amantibrewal310/airboss/compare/v0.1.0-alpha.4...v0.1.0-alpha.5
[0.1.0-alpha.4]: https://github.com/amantibrewal310/airboss/compare/v0.1.0-alpha.3...v0.1.0-alpha.4
[0.1.0-alpha.3]: https://github.com/amantibrewal310/airboss/compare/v0.1.0-alpha.2...v0.1.0-alpha.3
[0.1.0-alpha.2]: https://github.com/amantibrewal310/airboss/compare/v0.1.0-alpha.1...v0.1.0-alpha.2
[0.1.0-alpha.1]: https://github.com/amantibrewal310/airboss/releases/tag/v0.1.0-alpha.1
