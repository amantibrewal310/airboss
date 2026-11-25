# Capability map: Airboss

Approved 25 Nov 2026. This file is the index of specs: each module has its own `SPEC-<module>.md`, written and approved in build order. Module ids are fixed and are never renamed.

The direction behind it is in [docs/idea.md](docs/idea.md).

| Module id | What it does | Depends on |
|---|---|---|
| `core` | The task tree, which task waits on which, task history and links in SQLite. Enforces the rules: code is done only when its PR merges, and agents can finish only PR-less tasks; agents only propose subtasks; blocked needs a reason. | none |
| `mcp` | How agents and I talk to Airboss before (and alongside) the app. One `airboss` binary: `airboss mcp` is a per-thread MCP server over stdio (six tools: `list_tasks`, `get_task`, `update_task`, `link`, `propose_tasks`, `import_plan`), with identity taken from the thread's environment and no port or token. The other subcommands are a small CLI that acts as me. | `core` |
| `app` | The Mac app: Dock icon; main window with the tree on the left and the selected task on the right; menu-bar icon with the "waiting on you" count; hotkey quick-add; launch at login. Watches the database file to pick up writes from `airboss mcp` and CLI processes. | `core` |
| `launch` | The Do it and Plan it buttons. Writes the starting brief and runs `t3 thread start --worktree`, one thread per repo with a shared branch name. Links each thread to its task. The only module that calls the `t3` CLI. | `core`, `app` |
| `sync` | Live state from outside. Watches T3's database file and reads it read-only, only when it changes. Checks open linked PRs through `gh` every 5 minutes. A merged PR moves its task to done. | `core` |
| `attention` | The "needs me" screen at the top of the main window, and the menu-bar count: questions, approvals, PRs to review, failed runs, proposed subtasks, tasks whose blockers just finished ("ready to start"), and work quiet for 5+ days. | `sync`, `app` |

**Build order:** `core` → `mcp` and `app` → `launch` and `sync` → `attention`.

Dependencies point one way only. A module may use only the modules it depends on.

## Stack

- **One process**, a Tauri 2.12 app:
  - Rust, edition 2024, on stable Rust 1.100, pinned in `rust-toolchain.toml`.
  - Rust for all the logic: `rusqlite` 0.43 for SQLite, `rmcp` 3.11 (the official MCP Rust SDK) for MCP over streamable HTTP.
  - Svelte 5 for the UI.
  - No second runtime such as Node or Bun.
- **Versions** were checked on crates.io and npm on 25 Nov 2026: the latest stable release of each. Tauri 3 exists only as an alpha (3.0.0-alpha.6), so we stay on Tauri 2 and move when 3 is stable.
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
| `core` | [SPEC-core.md](SPEC-core.md) | **Built 25 Nov 2026** (`crates/core`, 218 tests, all performance targets met); plan in [tasks/plan.md](tasks/plan.md) |
| `mcp` | [SPEC-mcp.md](SPEC-mcp.md) | Draft 1, waiting for review |
| `app` | not written yet | |
| `launch` | not written yet | |
| `sync` | not written yet | |
| `attention` | not written yet | |
