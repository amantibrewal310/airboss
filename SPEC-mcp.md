# Spec: mcp

Module `mcp` from [CAPABILITIES.md](CAPABILITIES.md). State: **draft 1, waiting for review**.

It builds on [`core`](SPEC-core.md), which holds every rule about who may change what. `mcp` adds no rules of its own. It decides **who is calling** and passes that on to `core`.

## Objective

Give every agent thread, in T3 Code, Claude Code or Codex, a way to read and report on Airboss tasks. Before the Mac app exists, give me a small command line to do my part.

**User stories**

- As an agent starting work, I find my task (`list_tasks`, or the task id in my brief), read its context (`get_task`), and link my own thread to it.
- As an agent working, I report progress: `doing`, `in_review`, `blocked` with a reason, or notes. I also link the PRs I open.
- As an agent finishing a task that has no PR, I mark it `done` with a note saying what I did. A task with a PR finishes when its PR merges, not when I say so.
- As an agent that spots more work, I propose subtasks. They wait for me (the human) to accept them.
- As plan-review, once a plan is settled, I import it as a tree of tasks, linked to its plan file.
- As me, from the terminal before the app exists, I add tasks, see the tree, tick things off, and accept or reject proposals.

## Decisions (settled 25 Nov 2026)

- **Each thread gets its own process over stdio, not one shared HTTP server.**
  - Each agent thread starts its own `airboss mcp` process, which talks MCP over stdin and stdout and opens the same SQLite file.
  - There's no port, no token and no background service, and it works before the Mac app exists.
  - `core` already handles several processes writing at once (IMMEDIATE transactions; the concurrency test runs 4 writers).
  - **Cost:** the app can't rely on `Store::subscribe()` to see writes made by these processes. The `app` spec must watch the database file and catch up with `events_after(cursor)`. This replaces "listens on 127.0.0.1 with a token" in the capability map.
- **Identity comes from the thread's environment, never from tool arguments.** The process works out its actor once, at startup:

  | Variable present | Actor |
  |---|---|
  | `T3CODE_THREAD_ID` | `agent:t3/<id>` |
  | otherwise `CLAUDE_CODE_SESSION_ID` | `agent:claude/<id>` |
  | otherwise `AIRBOSS_THREAD` (set by hand in a client's MCP config, e.g. for Codex) | `agent:<value>` |
  | none of these | `agent:unknown/<parent pid>`, plus a warning in the server's instructions |

  Over MCP, an agent is always an agent. There is no way to act as `me`. The one exception is `import_plan`, which acts as `plan` (see below).
- **Settled plans are trusted, but must name their plan file.**
  - `import_plan` creates tasks straight away as actor `plan`, as `core` rules 8 and 19 intend.
  - It requires `plan_file`, a path to an existing file, and links that file as a `doc` on the plan's top-level tasks.
  - Every task it creates records which thread sent it, in the `created` event body (e.g. "imported by agent:t3/97b5…"). A misused import is visible in history and easy to undo: drop the parent.
- **I use a small CLI until the app exists.** The same `airboss` binary has commands that act as `me`. They only ever run in my own terminal, never through MCP.

## Tech stack

- **A new crate, `crates/mcp`**, called `airboss-mcp`. It builds one binary, `airboss`. Rust edition 2024, pinned to Rust 1.100 as in `core`.
- **Dependencies** (latest stable as of 25 Nov 2026):
  - `airboss-core` (path)
  - `rmcp` 3.11 with the features `server`, `macros` and `transport-io`
  - `tokio` 1.54 with `rt`, `macros` and `io-std`, using a current-thread runtime with no worker pool
  - `serde` 1 and `serde_json` 1.0
  - `schemars` 1.2, matching what `rmcp` uses
  - `clap` 4.6 with `derive`

  Nothing else without asking.
- **The database** is `~/Library/Application Support/Airboss/airboss.db`, created on first use. The `AIRBOSS_DB` environment variable overrides the path; tests use this.

## Commands

```
cargo build -p airboss-mcp --release                       # → target/release/airboss
cargo test -p airboss-mcp
cargo test -p airboss-mcp --release -- --ignored perf
cargo clippy -p airboss-mcp --all-targets -- -D warnings
cargo fmt --check
```

**Installing it into the agent tools** (done once, documented in the README):

```
claude mcp add --scope user airboss -- /path/to/airboss mcp
codex mcp add airboss -- /path/to/airboss mcp      # plus AIRBOSS_THREAD if Codex hides the thread id
```

## Project structure

```
crates/mcp/
  Cargo.toml               airboss-mcp, binary "airboss"
  src/main.rs              Command-line parsing; `mcp` starts the server, everything else is a CLI command
  src/identity.rs          Actor from the environment
  src/server.rs            MCP server: tool list, server instructions, error mapping
  src/tools.rs             One function per tool: arguments in, JSON out, calls core
  src/cli.rs               The `me` commands and how they print
  src/render.rs            Shared text and JSON shapes for tasks, trees and errors
  tests/tools.rs           Each tool called in-process against an in-memory Store
  tests/stdio.rs           The real binary over stdin/stdout: initialize, tools/list, tools/call
  tests/cli.rs             The real binary as a CLI, against a temporary AIRBOSS_DB
  tests/perf.rs            #[ignore] start-up and call-time checks
```

## MCP tools

Six tools, kept few on purpose: fewer tools means agents pick the right one more often. Every tool returns compact JSON as text content.

| Tool | Arguments | What it does (`core` call) | Returns |
|---|---|---|---|
| `list_tasks` | `repo?`, `status?` (list), `mine?` (bool), `query?` (text in the title) | Reads `tree()` and filters it. `mine` keeps tasks linked to this thread. | `[{id, title, status, parent, depth, waiting, repo}]`, at most 200, plus `truncated` |
| `get_task` | `id` | `task`, `links`, the last 20 entries of `history`, `waits_on`, the direct subtasks, and `run_order` if it has subtasks | One object with all of that |
| `update_task` | `id`, `status?`, `note?` | `set_status` when `status` is given (the note becomes the reason); otherwise `add_note`. At least one of the two is required. | The task after the change |
| `link` | `id`, `kind`, `ref?`, `url?`, `pr_state?` | `link`. When `kind` is `t3_thread` or `claude_session` and `ref` is left out, it links **this** thread. | The link |
| `propose_tasks` | `parent?`, `tasks: [{title, body?, repo?}]` (1–20) | `add_task` as the agent, once per item, so every one starts as `proposed` | The proposed tasks |
| `import_plan` | `plan_file`, `parent?`, `plan` (a tree: `key`, `title`, `body?`, `repo?`, `depends_on?`, `children?`) | Checks that `plan_file` exists, then calls `add_tasks` as `plan` and links the file to the plan's top-level tasks | `{created, existing, keys}` |

- **Repos are named, not numbered,** in tool arguments (`"repo": "hub-ui"`). An unknown repo name is an error that lists the registered repos.
- **There are no tools** to delete, move, edit titles, add dependencies outside a plan, accept proposals, or set PR state. Those are mine (the CLI, later the app) or belong to `sync`.
- **Errors** come back as MCP tool errors with one sentence that tells the agent what to do next. For example:
  - `NotAllowed`: "Agents can't finish a task that has a PR; it finishes when the PR merges. Set it to in_review instead."
  - `NoteRequired`: "Say what you did in `note` when finishing a task."
  - `InvalidTransition`: names the task's current status.

  Each `core::Error` variant has one fixed message, in `render.rs`.
- **The server instructions** (sent at `initialize`) are a short paragraph:
  - Call `list_tasks` with `mine` or your repo when you start.
  - Link your thread.
  - Report status changes as they happen.
  - Propose, don't create, new work.
  - Use `import_plan` only for a plan the user settled in plan-review.

  The instructions also name the actor this process will act as.

## CLI (acts as `me`)

```
airboss add "Fix booking typo" [--under 12] [--repo hub-ui] [--body "…"]
airboss ls [--all]                    # the tree; --all includes done and dropped
airboss show 12                       # task, links, history, what it waits on, run order
airboss set 12 doing|in_review|blocked|todo [--note "…"]
airboss done 12 [--note "…"]
airboss drop 12 [--note "…"]
airboss accept 14 | airboss reject 15 [--note "…"]
airboss edit 12 [--title "…"] [--body "…"] [--repo hub-ui]
airboss mv 12 [--under 9 | --top] [--pos 2]
airboss wait 12 --on 9 | airboss unwait 12 --on 9
airboss link 12 pr owner/repo#42 [--url …]
airboss repo add hub-ui ~/Workspace/…/hub-ui [--remote …] | airboss repo ls
airboss log [--after <event id>]      # recent history, newest last
airboss mcp                           # the MCP server over stdio (agents only)
```

- Output is plain text that's easy to scan. The tree prints one task per line, indented, with a status mark and `#id`. Every command except `mcp` also takes `--json`.
- Errors print one line to stderr and exit with code 1. The message is the same one agents get.

## Performance targets

On top of the targets in `CAPABILITIES.md`; measured on my Mac in release mode.

| What | Target |
|---|---|
| `airboss mcp` start to answering `initialize` | under 50 ms |
| A tool call (`list_tasks` on 2,000 tasks, `update_task`) | under 20 ms (95th percentile) |
| Memory per `airboss mcp` process when idle | under 15 MB |
| `airboss` binary size (release, stripped) | under 10 MB |
| A CLI command such as `airboss ls`, end to end | under 100 ms |

## Code style

The same as `core`: rustfmt, clippy with `-D warnings`, and no `unwrap` or `expect` outside tests. `mcp` never touches SQL. Everything goes through `airboss_core::Store`. Each tool function is a plain function, unit-tested without MCP:

```rust
/// Agents report progress; core decides whether they may.
pub fn update_task(store: &mut Store, actor: &Actor, args: UpdateTask) -> Result<Task, ToolError> {
    match (args.status, args.note.as_deref()) {
        (Some(to), note) => Ok(store.set_status(args.id, to, note, actor)?),
        (None, Some(note)) => {
            store.add_note(args.id, note, actor)?;
            Ok(store.task(args.id)?)
        }
        (None, None) => Err(ToolError::nothing_to_do("give `status`, `note`, or both")),
    }
}
```

## Testing strategy

- **Tests come first**, as in `core`.
- **Tool tests** (`tests/tools.rs`) call each tool function against `Store::open_in_memory()`:
  - every argument path
  - every `core` error, with its message
  - `mine` filtering
  - linking this thread when `ref` is left out
  - `propose_tasks` always producing `proposed` tasks
  - `import_plan` rejecting a missing file, linking the file, and recording which thread sent it
- **Identity tests:** each row of the identity table, as a pure function over a map of variables.
- **Stdio tests** (`tests/stdio.rs`) start the real binary with a temporary `AIRBOSS_DB` and a fake `T3CODE_THREAD_ID`, then:
  - send `initialize`, `tools/list` (exactly six tools, each with an input schema) and a few `tools/call` requests
  - check that a tool error comes back as an MCP error rather than a crash
  - check that two `airboss mcp` processes writing at once both succeed
- **CLI tests** (`tests/cli.rs`) run every command once against a temporary database, checking the exit code and output, plus `--json` round-trips.
- **Performance checks** (`#[ignore]`) cover start-up, calls and memory, printed next to the machine as in `core`.

## Boundaries

- **Always:**
  - Work out the actor once, from the environment.
  - Go through `Store` for everything.
  - Map every `core::Error` to its fixed message.
  - Keep tool argument names the same as in the table above.
- **Ask first:**
  - Adding a tool, changing a tool's arguments, or adding a dependency.
  - Letting anything over MCP act as `me`.
- **Never:**
  - Accept an actor from tool arguments.
  - Open a network port.
  - Write SQL.
  - Print anything but MCP messages on stdout in `mcp` mode. Logs go to stderr.

## Success criteria

- [ ] All six tools work over real stdio from a test client, and `tools/list` shows exactly these six with input schemas.
- [ ] An agent can't act as `me` through any tool, and its proposals arrive as `proposed`.
- [ ] Every `core` error reaches the agent as a one-sentence message that says what to do.
- [ ] `import_plan` refuses a plan file that doesn't exist, links the file it imports, and records which thread sent it.
- [ ] Every CLI command works against a temporary database and supports `--json`.
- [ ] Two `airboss mcp` processes and one CLI command can write at once without errors.
- [ ] Every performance target is met on my Mac, with the machine recorded.
- [ ] Installed with `claude mcp add`, a real T3 thread can list, link itself and update a task, checked by hand once.

## Not verified yet

- **Codex threads:** whether Codex passes `T3CODE_THREAD_ID` through to a stdio server. Its `mcp add` takes explicit `--env` variables, and the help doesn't say whether it inherits the thread's own environment. This gets checked in the first build task. If it doesn't, Codex threads set `AIRBOSS_THREAD` in their MCP config, and identity falls back as the table above says.
- **Exact `rmcp` 3.11 calls** for a stdio server with `#[tool]` macros. Confirmed so far: the features exist (`server`, `macros`, `transport-io`). The rest gets checked against the crate's source and docs before the code is written.

## Open questions

None. The three shape decisions above were settled with you.
