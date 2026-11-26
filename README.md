# Airboss

A to-do list where agents do the work. Drop in any task, from a one-line fix to a whole product, hand it to a coding agent with one key, and see where everything stands without opening a thread to ask.

On an aircraft carrier, the air boss launches every plane, tracks each one in the air and decides who lands next. Airboss does that for your agent threads.

![Needs me, from the design: questions, reviews, failed runs, proposals and ready tasks in one inbox](docs/design/screens/02-needs-me.png)

*A design screenshot of Needs me, with made-up sample data. The built screen follows it closely.*

## Why

Running many agent threads at once has a cost. In seven weeks of one engineer's work, 35 of 853 threads were check-ins like "where are we?" or "find thread X". A stack of 10 PRs sat idle for 19 days before anyone noticed. Plan docs drifted out of date, and tasks were spread across memory, saved chat messages and a tracker.

To-do apps don't know agent threads exist. Agent tools know threads, but not tasks or plans. Airboss joins the task, its threads, its PRs and its plan, and gets status from the agents, from GitHub and from T3 Code, so you don't type it.

## What it does

- **Capture from anywhere.** ⌃⌥N opens a quick-add box over any app. Paste a PR, issue, Slack or `t3code://` link into it and it's attached.
- **Tasks of any size.** Every task can have subtasks, at any depth, linked to one or more repos. Subtasks can wait on each other, and a parent shows its run order in waves.
- **Do it and Plan it.** ⌘↩ starts a T3 Code thread for the task, one per linked repo, each in its own worktree on a shared branch, with the task's notes as the brief. ⇧⌘↩ starts a planning thread, and the settled plan comes back as subtasks.
- **Needs me first.** The home screen lists what is waiting on you: agent questions and approvals, PRs to review, failed runs, agents that stopped without a PR, proposed subtasks, tasks that just became ready, parents whose subtasks are all done, and work quiet for 5 days. The menu-bar icon shows the count.
- **Status is read, not typed.** Agents report through an MCP server. A merged PR finishes its task. T3 Code's threads show whether an agent is working, waiting on you or stopped.
- **Agents propose, you decide.** Agents can move their tasks along and propose new ones. Only you accept proposals, drop tasks, or finish a task whose PR hasn't merged.
- **Keyboard first, and a real Mac app.** Native menus, a menu-bar item, open at login, dark and light, ⌘K to jump anywhere, ⌘Z to undo a slip, and a shortcut for every main action.
- **A CLI too.** The `airboss` command adds, moves, links and finishes tasks from a terminal, and accepts or rejects proposals.

## Requirements

- macOS 13 or later, on Apple silicon.
- Rust 1.100. `rust-toolchain.toml` pins it, so rustup installs it on the first build.
- Node 24 and pnpm, to build the app's interface.
- Optional: T3 Code for Do it, Plan it and live thread state, and the GitHub CLI (`gh`, signed in) for PR sync. Without them, those features switch off and everything else works.

## Quick start

```sh
git clone https://github.com/amantibrewal310/airboss.git
cd airboss/app
pnpm install
pnpm tauri build --config src-tauri/tauri.bundle.conf.json --bundles app
cp -R ../target/release/bundle/macos/Airboss.app /Applications/
open /Applications/Airboss.app
```

Then, in the app:

1. On first run, keep **Open Airboss at login** ticked and add your repos.
2. Choose **Airboss › Install Command-Line Tool**. It links `~/.local/bin/airboss` to the CLI inside the app.
3. Connect your agents. For Claude Code, once:

   ```sh
   claude mcp add --scope user airboss -- ~/.local/bin/airboss mcp
   ```

4. Paste the agent note (Help › Copy Agent Note) into each repo's `AGENTS.md` or `CLAUDE.md`.

Codex needs one more line of config. [Getting started](docs/getting-started.md) has the details.

## Docs

- [Getting started](docs/getting-started.md): build, install, first run, connecting agents
- [Using Airboss](docs/using-airboss.md): the daily flow, and every keyboard shortcut
- [CLI](docs/cli.md): every `airboss` command
- [MCP tools](docs/mcp.md): what agents can call, and what they get back
- [Architecture](docs/architecture.md): processes, the database and the data model
- [Troubleshooting](docs/troubleshooting.md)
- [Contributing](CONTRIBUTING.md)

The design is in [DESIGN.md](DESIGN.md) and [docs/design](docs/design/README.md). Each module has a spec, listed in [CAPABILITIES.md](CAPABILITIES.md).

## Status

Version 0.1.0. Every module is built, tested and merged: the task store, the MCP server and CLI, the Mac app, Do it and Plan it, sync with GitHub and T3 Code, and Needs me. Every Rust crate is tested (`cargo test --workspace`), and the interface has unit, component and end-to-end tests.

What isn't done yet:

- End-to-end QA as a real user and a polish pass are under way.
- There's no release download. Build it from source, as above.
- It's Apple silicon only, and signed ad hoc.
- It's built for one person's setup (T3 Code, Claude Code, Codex and `gh`). There's no onboarding for other setups.

## Privacy

Airboss is local only. There's no account, no server and no telemetry.

- Tasks live in one SQLite file on your Mac, `~/Library/Application Support/Airboss/airboss.db`.
- The app opens no network port, and its windows load nothing from the network.
- The only network traffic is the `gh` CLI reading the PRs you've linked, and your own open PRs, from GitHub, about every 5 minutes, with your own `gh` sign-in.
- It reads T3 Code's database read-only, and Claude Code's session files in `~/.claude/sessions`, to see what your agents are doing. It never writes to either.
- Do it and Plan it run the `t3` CLI, which starts threads in T3 Code on your Mac.

## License

License: not chosen yet.
