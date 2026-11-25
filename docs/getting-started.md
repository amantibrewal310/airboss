# Getting started

This page takes you from a fresh clone to agents reporting on your tasks.

## What you need

- A Mac with Apple silicon, on macOS 13 or later.
- [rustup](https://rustup.rs). The repo pins Rust 1.100 in `rust-toolchain.toml`, so rustup installs the right toolchain, with clippy and rustfmt, the first time you build.
- Node 24 and pnpm, to build the app's interface. Nothing Node runs in the finished app.

Optional, and each one turns on more:

| Tool | What it turns on | Without it |
|---|---|---|
| T3 Code, with its `t3` CLI at `~/.t3/bin/t3` | Do it and Plan it, live thread state, the repo list on first run | Do it and Plan it are off and say why. Everything else works. |
| GitHub CLI (`gh`), signed in | PR state, reviews and checks; a merged PR finishes its task | PRs stay as links. Everything else works. |
| Claude Code, Codex, or any MCP client | Agents report on their tasks | You update tasks yourself, in the app or with the CLI. |

## Build the app

```sh
git clone https://github.com/amantibrewal310/airboss.git
cd airboss/app
pnpm install
pnpm tauri build --config src-tauri/tauri.bundle.conf.json --bundles app
```

The build compiles the interface, the `airboss` CLI and the app in release mode. The first build takes a few minutes. The app lands in `target/release/bundle/macos/Airboss.app`, at the repo root.

Move it to Applications before you open it, because the CLI link in the next step points inside the app:

```sh
cp -R ../target/release/bundle/macos/Airboss.app /Applications/
open /Applications/Airboss.app
```

The app is signed ad hoc, not with a developer ID, so build it on the Mac you'll run it on.

## Install the `airboss` CLI

The app carries the CLI inside it. Install it from any of these:

- **Airboss › Install Command-Line Tool**
- **Settings (⌘,) › Command-line tool › Install**
- the **Agents** row on the first-run screen

Each one makes a link at `~/.local/bin/airboss` that points to `Airboss.app/Contents/MacOS/airboss`. If you move the app later, install it again. Settings shows whether `~/.local/bin` is on your `PATH`. If it isn't, add this to `~/.zshrc`:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Then check it:

```sh
airboss --version
airboss ls
```

**Without the app,** you can build the CLI on its own. It works by itself, against the same database:

```sh
cargo build -p airboss-mcp --release      # → target/release/airboss
```

Copy `target/release/airboss` somewhere on your `PATH`. If you copy it to `~/.local/bin/airboss`, the app's Install button leaves it alone and tells you the file is already there.

## First run

The first time it opens, Airboss turns on open at login. While there are no tasks, it opens on All tasks with the first-run screen:

- **Open Airboss at login** is ticked. Untick it to turn it off at once. At login the app starts in the menu bar, without a window. Settings changes it later. (A development build from `pnpm tauri dev` never turns it on.)
- **Repos** lists T3 Code's projects. Tick the ones you work in and choose **Add N repos**. Each repo is named after its folder, and its remote comes from the folder's `.git/config`. Folders that aren't git repos, or are already added, are greyed out with the reason. **Add another folder…** picks any folder. Without T3 Code, only the folder picker shows.
- **Connections** shows Agents, T3 Code and GitHub, with a button to fix each one that's off.

You can add repos later from the **+** next to Repos in the sidebar, or from the terminal:

```sh
airboss repo add booking-web ~/Workspace/booking-web --remote https://github.com/acme/booking-web
```

The quick-add hotkey is **⌃⌥N**. Press it in any app to add a task. "Change hotkey…" on the first-run screen, or Settings, picks another.

## Connect your agents

Every agent thread starts its own `airboss mcp` process and talks to it over stdin and stdout. There's no port, token or background service. The process works out which thread it serves from its environment, once, when it starts:

| Variable | The agent shows up as |
|---|---|
| `T3CODE_THREAD_ID` (set by T3 Code) | `agent:t3/<id>` |
| otherwise `CLAUDE_CODE_SESSION_ID` (set by Claude Code) | `agent:claude/<id>` |
| otherwise `AIRBOSS_THREAD` (set by you, in the client's MCP config) | `agent:<value>` |
| none of them | `agent:unknown/<parent pid>`, with a warning in the server's instructions |

### Claude Code

Once, for every project:

```sh
claude mcp add --scope user airboss -- ~/.local/bin/airboss mcp
```

**Settings › Agent setup › Copy setup command** copies this exact line. Claude Code passes its session id to the server, and T3 Code adds its thread id, so each thread is recognised as itself.

### Codex

```sh
codex mcp add airboss -- ~/.local/bin/airboss mcp
```

Codex passes a server only a few variables, so the T3 thread id doesn't reach it unless you ask. Open `~/.codex/config.toml` and add `env_vars` to the server that `codex mcp add` wrote:

```toml
[mcp_servers.airboss]
command = "/Users/you/.local/bin/airboss"
args = ["mcp"]
env_vars = ["T3CODE_THREAD_ID"]
```

`codex mcp add` has no flag for `env_vars`, which is why it's a hand edit.

Codex outside T3 Code has no thread id to pass. Give it a name instead, with `--env AIRBOSS_THREAD=codex/<name>` on `codex mcp add`. Every Codex thread then shares that name. Without one, it shows up as `agent:unknown/<pid>`, and `mine` and linking "this thread" don't work.

### Any other MCP client

Run `airboss mcp` as a stdio server. If the client doesn't pass `T3CODE_THREAD_ID` or `CLAUDE_CODE_SESSION_ID` through, set `AIRBOSS_THREAD` in its server config. The value may be `t3/<id>` or `claude/<id>`; then `mine` and linking this thread work too.

### Check it

Ask an agent to call `list_tasks`. It should return your tasks. In the agent's tool list you should see six tools: `list_tasks`, `get_task`, `update_task`, `link`, `propose_tasks` and `import_plan`. They're described in [MCP tools](mcp.md).

## Tell agents to report

Agents report more reliably when the repo tells them to. Put this in each repo's `AGENTS.md` (Codex) or `CLAUDE.md` (Claude Code). **Help › Copy Agent Note** and **Settings › Agent setup › Copy agent note** copy the same text.

```md
## Airboss

I track my work in Airboss, a task list that agents update through the `airboss` MCP server.

- When you start on something, find its task with `list_tasks` (or `get_task` if I gave you an id) and link this thread to it with `link`.
- Report progress with `update_task`: `doing` when you start, `blocked` with a reason when you're stuck, `in_review` when a PR is up. Leave a note when something worth knowing happens.
- Finish a task without a PR with `update_task` set to `done` and a note saying what you did. A task with a PR finishes on its own when the PR merges.
- If the work splits into smaller pieces, suggest them with `propose_tasks`. I accept or reject them.
- When a plan is settled, send it with `import_plan`; its steps become subtasks.
```

The server also sends instructions when a session starts. An agent connected from a T3 thread gets this paragraph, with its own actor:

> Airboss is the user's to-do list, and these tools report on the tasks you work on. You act as agent:t3/97b5…. When you start, find your task with list_tasks (`mine: true`, or your repos), or use the task id in your brief, and read it with get_task. Link this thread to it (link, kind t3_thread or claude_session, no ref). Report status changes with update_task as they happen, and link every PR you open. Propose new work with propose_tasks; don't create it. Use import_plan only for a plan the user settled in plan-review.

Threads started with Do it or Plan it get a brief that repeats these steps for their task, so the note matters most for threads you start yourself.

**Plan it** asks the agent to plan with a `plan-review` skill and then call `import_plan`. Install that skill for the agent you plan with.

## Uninstall

If you might want your tasks later, back them up first, while the CLI is still there:

```sh
airboss backup ~/airboss-backup.db
```

Then, in this order:

1. **Quit Airboss:** ⌘Q, or **Quit Airboss** in the menu-bar menu. Closing the window leaves it running in the menu bar.
2. **Remove the login item.** Open at login is a LaunchAgent. Turn it off in Settings before you quit, or remove it:

   ```sh
   rm -f ~/Library/LaunchAgents/Airboss.plist
   ```

3. **Remove the app:**

   ```sh
   rm -rf /Applications/Airboss.app
   ```

4. **Remove the CLI link**, if it's still the one the app made:

   ```sh
   [ -L ~/.local/bin/airboss ] && rm ~/.local/bin/airboss
   ```

   If you built the CLI yourself and copied it there, remove that file instead. Take the `export PATH="$HOME/.local/bin:$PATH"` line out of `~/.zshrc` too, unless something else lives in `~/.local/bin`.
5. **Remove `airboss mcp` from each agent client:**

   ```sh
   claude mcp remove --scope user airboss
   codex mcp remove airboss
   ```

   For any other client, delete the `airboss` server from its MCP config. Remove the [agent note](#tell-agents-to-report) from your repos' `AGENTS.md` and `CLAUDE.md` as well.
6. **Optionally, remove your data.** This deletes every task and its history, and can't be undone:

   ```sh
   rm -rf ~/Library/Application\ Support/Airboss     # airboss.db, its -wal and -shm, app-settings.json
   rm -rf ~/Library/Logs/Airboss                    # app.log and the two before it
   ```

   macOS may also keep the web view's caches in `~/Library/WebKit/io.github.amantibrewal310.airboss` and `~/Library/Caches/io.github.amantibrewal310.airboss`; remove them if they're there. A database you pointed `AIRBOSS_DB` at stays where you put it, with its own `app-settings.json` beside it.

Airboss never writes to T3 Code's or Claude Code's files, so there's nothing to undo there.

## Next

- [Using Airboss](using-airboss.md): the daily flow and every shortcut
- [Troubleshooting](troubleshooting.md), if a connection shows orange
