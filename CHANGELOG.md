# Changelog

What changed in each release of Airboss. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html): alphas are `0.1.0-alpha.N`, with macOS build number `N`. Each release's page on GitHub has the install steps and the .dmg.

## [Unreleased]

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

[Unreleased]: https://github.com/amantibrewal310/airboss/compare/v0.1.0-alpha.4...HEAD
[0.1.0-alpha.4]: https://github.com/amantibrewal310/airboss/compare/v0.1.0-alpha.3...v0.1.0-alpha.4
[0.1.0-alpha.3]: https://github.com/amantibrewal310/airboss/compare/v0.1.0-alpha.2...v0.1.0-alpha.3
[0.1.0-alpha.2]: https://github.com/amantibrewal310/airboss/compare/v0.1.0-alpha.1...v0.1.0-alpha.2
[0.1.0-alpha.1]: https://github.com/amantibrewal310/airboss/releases/tag/v0.1.0-alpha.1
