# Airboss

A to-do list for working with coding agents.

Jot down a task in a couple of seconds, whether it's a one-line bug fix or a whole new product. Hand it to an agent with one key. Always see where everything stands, without opening a thread to ask.

On an aircraft carrier, the air boss launches every plane, tracks each one in the air and decides who lands next. Airboss does that for your agent threads.

> **Status:** early. `core` (task tree, history, links, dependencies, run order) and the `airboss` binary (an MCP server for agents plus a CLI for you) are built and tested. The Mac app, launching, sync and Needs me are being built. See [CAPABILITIES.md](CAPABILITIES.md) and [tasks/program.md](tasks/program.md).

## What it will do

- **Capture:** a global hotkey opens a quick-add box. Paste a Slack link into it if the task came from Slack.
- **Tasks of any size:** every task can have subtasks. A big task gets a plan, which becomes phases and then small tasks.
- **Start:** two buttons on every task. **Do it** opens an agent thread with the context filled in. **Plan it** runs a planning session first.
- **Needs me:** the home screen shows what is waiting on you: questions, approvals, PRs to review, failed runs, and work that has gone quiet.
- **Agents report back:** a local MCP server lets every agent thread update the tasks it is working on.

## Use with agents

The `airboss` binary is both an MCP server for agent threads and a small CLI for you. Each agent thread starts its own `airboss mcp` process over stdio; there's no port, token or background service. Tasks live in `~/Library/Application Support/Airboss/airboss.db` (set `AIRBOSS_DB` to use another file).

**Build** (Rust 1.100, pinned in `rust-toolchain.toml`):

```
cargo build -p airboss-mcp --release      # → target/release/airboss
```

Copy `target/release/airboss` somewhere on your `PATH`, or use its full path below.

**Claude Code**, once, for every project:

```
claude mcp add --scope user airboss -- /path/to/airboss mcp
```

Claude Code hands the server its session id, and T3 Code adds its thread id, so each thread is recognised by itself.

**Codex**:

```
codex mcp add airboss -- /path/to/airboss mcp
```

Codex passes a server only a few variables, so in `~/.codex/config.toml` add the T3 thread id to the server's `env_vars`:

```toml
[mcp_servers.airboss]
command = "/path/to/airboss"
args = ["mcp"]
env_vars = ["T3CODE_THREAD_ID"]
```

Codex outside T3 has no thread id to pass. Give it a name with `--env AIRBOSS_THREAD=codex/<name>` on `codex mcp add` (every Codex thread then shares that name), or it shows up as `agent:unknown/<pid>`.

**Agents get six tools:** `list_tasks`, `get_task`, `update_task`, `link`, `propose_tasks` and `import_plan`. They can move their tasks along and propose new ones; only you accept proposals, drop tasks, or finish a task whose PR hasn't merged.

**The CLI acts as you:**

```
airboss add "Fix booking typo" --repo hub-ui     # --under 12 to nest it
airboss ls                                        # the tree; --all, --ready
airboss show 12
airboss set 12 doing                              # todo, doing, in_review, blocked --note "…"
airboss done 12
airboss accept 14                                 # or: airboss reject 14 --note "…"
airboss repo add hub-ui ~/Workspace/hub-ui
airboss log
```

Every command takes `--json`, and `airboss --help` lists the rest.
