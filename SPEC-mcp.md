# Spec: mcp

Module `mcp` from [CAPABILITIES.md](CAPABILITIES.md). State: **approved 25 Nov 2026**, aligned with `core` v2 the same day (see [v2 alignment](#v2-alignment-25-nov-2026)); **built** in `crates/mcp` on 26 Nov 2026 (see [decisions made while building](#decisions-made-while-building)).

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
codex mcp add airboss -- /path/to/airboss mcp      # then env_vars = ["T3CODE_THREAD_ID"] in ~/.codex/config.toml
```

## Project structure

```
crates/mcp/
  Cargo.toml               airboss-mcp, binary "airboss"
  src/lib.rs               The database path, and the modules below as a library the tests call
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
| `list_tasks` | `repos?` (list of names), `status?` (list), `mine?` (bool), `ready?` (bool), `query?` (text in the title), `priority?` (list of `p0`–`p3`), `sort?` (`tree` or `priority`) | Reads `tree()` and filters it. `repos` keeps tasks linked to any of them. `mine` keeps tasks linked to this thread. `ready` keeps what `ready()` returns: `todo` tasks with no open blockers. `priority` keeps tasks with any of them. `sort: priority` puts P0 first and tasks with none last, ties in tree order, before the cap (SPEC-core rule 35). | `[{id, title, status, priority, parent, depth, waiting, repos}]`, at most 200, plus `truncated`; `priority` left out when none is set |
| `get_task` | `id` | `task` (with its repos by name and its `branch`), `links` (with titles), the last 20 entries of `history`, `dependencies` both ways (what it waits on and what waits on it, finished ones included), the direct subtasks, and `run_order` if it has subtasks (`waves`, `waiting_outside` with each entry's blockers, `tangled`) | One object with all of that |
| `update_task` | `id`, `status?`, `note?` | `set_status` when `status` is given (the note becomes the reason); otherwise `add_note`. At least one of the two is required. | The task after the change |
| `link` | `id`, `kind`, `ref?`, `url?`, `pr_state?`, `title?` | `link`, with `title` as the link's display name. When `kind` is `t3_thread` or `claude_session` and `ref` is left out, it links **this** thread. | The link |
| `propose_tasks` | `place?` (`sibling` or `top`), `parent?`, `tasks: [{title, body?, repos?, blocks?, priority?}]` (1–20) | `add_task` as the agent, once per item, so every one starts as `proposed`. Without `parent` it goes next to this thread's own task, under that task's parent (`sibling`, the default), or at top level (`top`); a given `parent` wins. Each waits on this thread's own task (`NewTask::waiting_on`), which the app shows as "follows #n", unless it sits under that task. Each `blocks` id waits on it (`NewTask::blocking`). Neither holds anything up until I accept it (SPEC-core rules 16 and 37). A `priority` (`p0` to `p3`) is kept (`NewTask::with_priority`, SPEC-core rule 35); left out, it has none. | The proposed tasks |
| `import_plan` | `plan_file`, `parent?`, `plan` (a tree: `key`, `title`, `body?`, `repos?`, `depends_on?`, `children?`) | Checks that `plan_file` exists, then calls `add_tasks` as `plan` and links the file to the plan's top-level tasks | `{created, existing, keys}` |

- **Repos are named, not numbered,** in tool arguments and results (`"repos": ["booking-web", "booking-api"]`). A task can have several. An unknown repo name is an error that lists the registered repos.
- **There are no tools** to delete, move, edit titles, change a task's repos or branch, add dependencies outside a plan or a proposal, accept proposals, or set PR state. Those are mine (the CLI, later the app) or belong to `sync`.
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
  - Don't widen your own task: ship your part so it's safe without the missing piece, and propose that piece, such as another repo's work, as a sibling that says what it blocks.
  - Use `import_plan` only for a plan the user settled in plan-review.

  The instructions also name the actor this process will act as.

## CLI (acts as `me`)

```
airboss add "Fix booking typo" [--under 12] [--repo booking-web]… [--body "…"]   # --repo again for more
airboss ls [--all] [--ready]          # the tree; --all includes done and dropped
airboss show 12                       # task, repos, branch, links, history, dependencies both ways, run order
airboss set 12 doing|in_review|blocked|todo [--note "…"]
airboss done 12 [--note "…"]
airboss drop 12 [--note "…"]
airboss accept 14 | airboss reject 15 [16 …] [--note "…"]   # reject skips any accepted meanwhile
airboss edit 12 [--title "…"] [--body "…"]
airboss repo link 12 booking-web | airboss repo unlink 12 booking-web
airboss branch 12 booking-flow | airboss branch 12 --clear
airboss mv 12 [--under 9 | --top] [--pos 2]
airboss wait 12 --on 9 | airboss unwait 12 --on 9
airboss link 12 pr owner/repo#42 [--url …] [--title …]
airboss repo add booking-web ~/Workspace/…/booking-web [--remote …] | airboss repo ls
airboss repo edit booking-web [--name …] [--path …] [--remote … | --no-remote]
airboss repo rm booking-web                # unlinks it from its tasks first
airboss log [--after <event id>]      # recent history, newest last
airboss backup ~/Backups/airboss.db [--force]   # a consistent copy, while everything runs
airboss mcp                           # the MCP server over stdio (agents only)
```

- Output is plain text that's easy to scan. The tree prints one task per line, indented, with a status mark and `#id`. Every command except `mcp` also takes `--json`.
- Errors print one line to stderr and exit with code 1. The message is the same one agents get.
- `airboss reject` goes through `core`'s `reject_proposals`, all ids in one change. A missing id, or a task an agent never proposed, is an error before anything is written. A proposal accepted or dropped meanwhile (by the app, or another `airboss`) is left as it is and printed as `153 was already accepted (todo); left as is` or `153 was already dropped; left as is`, and the command still succeeds. `--json` gives `{rejected: [task…], skipped: [{id, status}…]}`.

## Performance targets

On top of the targets in `CAPABILITIES.md`; measured on my Mac in release mode.

| What | Target | Measured (25 Nov 2026) |
|---|---|---|
| `airboss mcp` start to answering `initialize` | under 50 ms | 2.3 ms (p95, 50 runs) |
| A tool call (`list_tasks` on 2,000 tasks, `update_task`) | under 20 ms (95th percentile) | `list_tasks` 3.4 ms, `update_task` 0.1 ms, `get_task` 3.4 ms (p95, 300 runs each) |
| Memory per `airboss mcp` process when idle | under 15 MB | 8.6 MB resident |
| `airboss` binary size (release, stripped) | under 10 MB | 5.4 MB (6.8 MB as built) |
| A CLI command such as `airboss ls`, end to end | under 100 ms | 9.3 ms (p95, 50 runs) |

Measured on a Mac16,1 (Apple M4), macOS 27.2.1, in release mode, over real stdio against a file-backed database with 2,020 tasks (`tests/perf.rs`).

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

- [x] All six tools work over real stdio from a test client, and `tools/list` shows exactly these six with input schemas.
- [x] An agent can't act as `me` through any tool, and its proposals arrive as `proposed`.
- [x] Every `core` error reaches the agent as a one-sentence message that says what to do.
- [x] `import_plan` refuses a plan file that doesn't exist, links the file it imports, and records which thread sent it.
- [x] Every CLI command works against a temporary database and supports `--json`.
- [x] Two `airboss mcp` processes and one CLI command can write at once without errors.
- [x] Every performance target is met on my Mac, with the machine recorded.
- [ ] Installed with `claude mcp add`, a real T3 thread can list, link itself and update a task, checked by hand once.

## Not verified yet

Checked on 25 Nov 2026, before the code was written (build task T1):

- **rmcp 3.11.0, from the crate source** (`rmcp-3.11.0`, `rmcp-macros-3.11.0`) and a throwaway server:
  - `#[tool_router] impl S { #[tool(description = …, annotations(read_only_hint = true))] async fn f(&self, Parameters(args): Parameters<T>) -> CallToolResult }` builds the router. `T: Deserialize + JsonSchema`; doc comments on its fields become the schema's descriptions, and `deny_unknown_fields` gives `additionalProperties: false`. Recursive types (a plan's `children`) come out as `$defs` and `$ref`.
  - `#[tool_handler(router = self.tool_router)] impl ServerHandler for S { fn get_info(&self) -> ServerConfig }` fills in `call_tool` and `list_tools` and keeps a hand-written `get_info`, which is how the instructions can name the actor. `ServerConfig` is `InitializeResult`: `ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(…).with_instructions(…)`.
  - `S.serve(rmcp::transport::stdio()).await?.waiting().await` runs until stdin closes. It works on a current-thread runtime. The handler must be `Send + Sync`, so the `Store` goes behind a `Mutex`.
  - **Tool errors:** `CallToolResult::error(vec![ContentBlock::text(…)])` is a tool-level error (`isError: true`), which the agent sees. Arguments that don't deserialize also come back as a tool error, `failed to deserialize parameters: unknown field …`, not a JSON-RPC error. Only an unknown tool name is a JSON-RPC error (`-32602 tool not found`).
  - `initialize` negotiates the client's version, `2024-11-05` up to `2025-11-25`.
- **Claude Code 2.1.332 passes its whole environment to a stdio server, and adds `CLAUDE_CODE_SESSION_ID` itself.** A probe server under `claude -p --mcp-config …` saw `CLAUDE_CODE_SESSION_ID` equal to that run's session id (with the variable removed from the parent beforehand), plus `T3CODE_THREAD_ID` and an unrelated marker inherited from the parent. So a T3 thread running Claude Code gives `airboss mcp` both ids.
- **Codex 0.172.0 does not.** A stdio server gets only `HOME LANG LOGNAME PATH PWD SHELL TMPDIR USER __CF_USER_TEXT_ENCODING`. Listing a variable in the server's `env_vars` forwards it: with `env_vars = ["T3CODE_THREAD_ID"]` the probe saw it. `codex mcp add` has no flag for `env_vars` (`--env` sets fixed values), so it goes in `~/.codex/config.toml` by hand. Codex sets its own `CODEX_THREAD_ID` only for the shell commands it runs, not in its own environment, so `env_vars` can't forward it.
  - **Consequence:** inside T3, a Codex thread is identified through `T3CODE_THREAD_ID` once `env_vars` lists it (README). Outside T3, Codex has no per-thread id to offer, so it falls back to `AIRBOSS_THREAD` or `agent:unknown/<parent pid>`, as the identity table says.

Still open, checked by hand once installed (last success criterion):

- Whether T3 sets `T3CODE_THREAD_ID` per thread for Codex threads the way it does for Claude Code ones. The probes above set it by hand.

## Decisions made while building

Where the build differs from, or fills in, the text above:

- **A library target.** `src/lib.rs` makes the modules callable from `tests/tools.rs`; `main.rs` only parses arguments. The CLI's text helpers (status marks, columns, colour) live in `render.rs`, as the project structure says.
- **`list_tasks` returns `{tasks, total, truncated}`**, so an agent sees how many matched before the 200 cap. With no `status`, it leaves out done and dropped tasks, as `airboss ls` does; otherwise old work would crowd out current work at the cap.
- **`get_task` also returns `waiting` and `history_total`**, so an agent knows whether it can start and whether there's history beyond the last 20.
- **`import_plan` records the sender in a note, not the `created` event body.** `core`'s `add_tasks` takes no body, and `core` is closed for this module. Each task the import creates gets a note right after its `created` event, written by the agent itself, so the note's actor names the thread: "Imported from plan.md" (the file name; the plan's top-level tasks link the full path). Before 0.1.0-alpha.6 the body also repeated the actor and the full path ("Imported by agent:t3/97b5… from /path/plan.md"); the app shows those older notes the short way too. Re-sending a plan creates nothing and adds no notes.
- **`update_task`'s schema offers only `doing`, `in_review`, `blocked` and `done`**, the statuses an agent may set, so agents don't try the others. `core` still decides each change.
- **`propose_tasks` checks first, then adds.** `core` has no batch add for agents, so the count, titles, repo names and the parent are all checked before the first task is added. Only a database failure can stop it partway, and then the error lists what was already proposed.
- **This thread's own task** is the one it's linked to (`mine`), or of several, the one it was linked to last; proposals and dropped tasks don't count. With none, `sibling` means top level and nothing is followed. `blocks` naming a missing, done or dropped task is refused before anything is added; a loop or a parent-and-step pair comes back from `core` with the proposal's title in front. That the ids come from what waits on the agent's own task is advice in the tool text, not a check: an agent may know of other work its proposal holds up.
- **Error messages:** one per `core::Error` variant, picked out by what the call tried where a variant covers two situations (`NotAllowed`: a task with a PR, or a status only the user sets; `NoteRequired`: blocked or done; `InvalidTarget`: a closed parent, or a dependency on a parent or subtask; `Loop`: a move or a dependency). Arguments that don't match the schema are reported by rmcp as a tool error, `failed to deserialize parameters: …`, which names the bad field or value.
- **A database from a newer Airboss** (core rule 32) is refused with `core`'s own sentence, "This database was created by a newer Airboss. Update the `airboss` command.", on its own: at start, `airboss mcp` and every CLI command print it to stderr and exit 1, and a server that's already running returns it as the tool error for every write.
- **`AIRBOSS_THREAD`** may be written `t3/<id>` or `claude/<id>`; then `mine` and linking this thread work too. A leading `agent:` is ignored.
- **Tool annotations:** `list_tasks` and `get_task` are marked read-only; `link` and `import_plan` idempotent; none is destructive.
- **CLI details:** `airboss branch <task> --clear` clears a branch; `airboss ls --ready` shows since when each task has been ready; `airboss log` shows the last 50 events unless given `--after`, each line starting with its event id (the number `--after` takes); `ls` keeps a done or dropped parent when something under it is open, so the tree keeps its shape. Waiting is drawn as todo's ring with a "waiting" pill, and proposed as the dotted ring.

## Open questions

None. The three shape decisions above were settled with you.
