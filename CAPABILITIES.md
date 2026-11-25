# Capability map: Airboss

Approved 25 Nov 2026. This file is the index of specs: each module has its own `SPEC-<module>.md`, written and approved in build order. Module ids are fixed and are never renamed.

The direction behind it is in [docs/idea.md](docs/idea.md).

| Module id | What it does | Depends on |
|---|---|---|
| `core` | The task tree, task history and links in SQLite. Enforces the rules: done only from a merged PR or from me; agents only propose subtasks; blocked needs a reason. | none |
| `mcp` | The local MCP server agents use (`list_tasks`, `get_task`, `update_task`, `link`, `add_tasks`, `propose_tasks`). Needs a token, listens on 127.0.0.1 only. | `core` |
| `app` | The Mac app: Dock icon; main window with the tree on the left and the selected task on the right; menu-bar icon with the "waiting on you" count; hotkey quick-add; launch at login. | `core` |
| `launch` | The Do it and Plan it buttons. Writes the starting brief and runs `t3 thread start --worktree`, one thread per repo with a shared branch name. Links each thread to its task. The only module that calls the `t3` CLI. | `core`, `app` |
| `sync` | Live state from outside. Watches T3's database file and reads it read-only, only when it changes. Checks open linked PRs through `gh` every 5 minutes. A merged PR moves its task to done. | `core` |
| `attention` | The "needs me" screen at the top of the main window, and the menu-bar count: questions, approvals, PRs to review, failed runs, proposed subtasks, and work quiet for 5+ days. | `sync`, `app` |

**Build order:** `core` → `mcp` and `app` → `launch` and `sync` → `attention`.

Dependencies point one way only. A module may use only the modules it depends on.

## Stack

- **One process**, a Tauri v2 app:
  - Rust for all the logic: `rusqlite` for SQLite, `rmcp` for MCP over streamable HTTP.
  - Svelte for the UI.
  - No second runtime such as Node or Bun.
- **macOS only.** `launch` needs T3 Code installed and `sync` needs `gh` logged in. Without them, those features switch off and everything else still works.
- **Data** lives in one SQLite file, `~/Library/Application Support/Airboss/airboss.db`.

## Performance targets (every module)

A module counts as done only after these are measured on a real Mac.

| What | Target |
|---|---|
| App download size | under 15 MB |
| Memory when idle (app plus its web view) | under 80 MB |
| CPU when idle | near 0%, no busy polling |
| Cold start to usable window | under 1 s |
| Hotkey to quick-add box on screen | under 100 ms |
| An agent's MCP call | under 20 ms (95th percentile) |
| Tree with 2,000 tasks | scrolls smoothly at 60 fps |

## Specs

| Module | Spec | State |
|---|---|---|
| `core` | [SPEC-core.md](SPEC-core.md) | Draft, waiting for review |
| `mcp` | not written yet | |
| `app` | not written yet | |
| `launch` | not written yet | |
| `sync` | not written yet | |
| `attention` | not written yet | |
