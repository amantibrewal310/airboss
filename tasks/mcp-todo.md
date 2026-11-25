# Tasks: mcp

The plan is in [mcp-plan.md](mcp-plan.md), and the spec is [SPEC-mcp.md](../SPEC-mcp.md).

Every task meets the Definition of Done:

- tests are written first and pass
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean
- no `unwrap` or `expect` outside tests
- no test touches the real database: each sets `AIRBOSS_DB` to a temp dir

The focused test command is `cargo test -p airboss-mcp`.

Sizes: XS = 1 file, S = 1–2 files, M = 3–5 files.

## Phase 1: Foundation

- [x] **T1. Crate, and the rmcp and identity checks** (S)
  - **Description:** Add `crates/mcp` (`airboss-mcp`, binary `airboss`) to the workspace with exactly the spec's dependencies at their latest stable versions. Read rmcp 3.11's source for `#[tool_router]`, `#[tool]`, `ServerHandler`, `serve(stdio())` and tool errors. Probe what environment Claude Code and Codex give a stdio server.
  - **Acceptance:**
    - `airboss --help` and `airboss --version` work.
    - The spec's "Not verified yet" section says what was confirmed, with versions.
  - **Verify:** `cargo build -p airboss-mcp`; a CLI test for `--help`.
  - **Files:** `Cargo.toml`, `crates/mcp/Cargo.toml`, `src/lib.rs`, `src/main.rs`, `tests/cli.rs`, `SPEC-mcp.md`
  - **Depends on:** none

- [x] **T2. Identity from the environment** (S)
  - **Description:** `identity::from_env(lookup, parent_pid)` returns the actor, this thread's links (T3 thread, Claude session) and a warning when unknown.
  - **Acceptance:** every row of the spec's identity table; blank values count as missing; `AIRBOSS_THREAD=t3/…` or `claude/…` also knows its thread link.
  - **Verify:** unit tests in `src/identity.rs`.
  - **Files:** `src/identity.rs`
  - **Depends on:** T1

### Checkpoint 1

- [x] All tests pass; clippy and fmt are clean.

## Phase 2: Tools

- [x] **T3. Error messages and the read tools** (M)
  - **Description:** `ToolError` and one message per `core::Error` variant in `render.rs`. The JSON views. `list_tasks` (repos, status, mine, ready, query, 200 cap, `truncated`) and `get_task` (task with repos and branch, links, last 20 history, dependencies both ways, subtasks, run order).
  - **Acceptance:** each filter alone and combined; done and dropped are left out unless `status` asks for them; an unknown repo lists the registered ones; a missing task says so with its id.
  - **Verify:** `tests/tools.rs`.
  - **Files:** `src/render.rs`, `src/tools.rs`, `tests/tools.rs`, `tests/common/mod.rs`
  - **Depends on:** T2

- [x] **T4. `update_task` and `link`** (S)
  - **Description:** status and/or note; link with title, and "this thread" when `ref` is left out.
  - **Acceptance:** `NotAllowed` (done with a PR, and other statuses), `InvalidTransition` naming the status, `NoteRequired` for done and for blocked, nothing to do; linking this thread for T3 and Claude, and a clear error when the process has no such id; `pr_state` refused on a non-PR link.
  - **Verify:** `tests/tools.rs`.
  - **Files:** `src/tools.rs`, `src/render.rs`, `tests/tools.rs`
  - **Depends on:** T3

- [x] **T5. `propose_tasks` and `import_plan`** (S)
  - **Description:** proposals as the agent, 1–20, checked before the first write. A plan as `plan`, with `plan_file` required and linked as a `doc` on the top-level tasks, and a note on each created task naming the thread that sent it.
  - **Acceptance:** proposals are always `proposed`; a closed parent, a blank title, an unknown repo or 21 items write nothing; a missing plan file is refused; running the same plan twice creates nothing and adds no notes; `DuplicateKey`, `Loop` and `InvalidTarget` messages.
  - **Verify:** `tests/tools.rs`.
  - **Files:** `src/tools.rs`, `tests/tools.rs`
  - **Depends on:** T4

### Checkpoint 2

- [x] Every tool and every `core` error message is tested in-process.

## Phase 3: Server

- [x] **T6. The stdio server** (M)
  - **Description:** `server.rs` with the six tools, their descriptions, annotations and the server instructions naming the actor. `airboss mcp` opens the database (from `AIRBOSS_DB` or the default path) and serves on stdin/stdout, logging only to stderr.
  - **Acceptance:** `initialize` returns the instructions; `tools/list` returns exactly six tools, each with an input schema; `tools/call` works; a refused call is an MCP tool error (`isError: true`) and the server keeps running; bad arguments are a tool error too; two `airboss mcp` processes and one CLI command write at once without errors.
  - **Verify:** `tests/stdio.rs` against the real binary.
  - **Files:** `src/server.rs`, `src/main.rs`, `tests/stdio.rs`, `tests/common/mod.rs`
  - **Depends on:** T5

## Phase 4: CLI

- [x] **T7. CLI reads** (M)
  - **Description:** `ls [--all] [--ready]`, `show`, `log [--after]`, `repo ls`, each with `--json`. Aligned columns, DESIGN.md's status marks, colour only on a TTY and never with `NO_COLOR`.
  - **Acceptance:** output checked line by line; `--json` parses back; no colour codes when piped.
  - **Verify:** `tests/cli.rs` and unit tests in `src/cli.rs` for the colour switch.
  - **Files:** `src/cli.rs`, `src/main.rs`, `src/render.rs`, `tests/cli.rs`
  - **Depends on:** T3

- [x] **T8. CLI writes** (M)
  - **Description:** `add`, `set`, `done`, `drop`, `accept`, `reject`, `edit`, `mv`, `wait`, `unwait`, `link`, `repo add|link|unlink`, `branch`.
  - **Acceptance:** every command once against a temp database, with exit code and output; errors are one line on stderr with exit code 1 and the agents' message; `accept` and `reject` refuse a task that isn't proposed.
  - **Verify:** `tests/cli.rs`.
  - **Files:** `src/cli.rs`, `src/main.rs`, `tests/cli.rs`
  - **Depends on:** T7

### Checkpoint 3

- [x] Every CLI command works and supports `--json`.

## Phase 5: Finish

- [x] **T9. Performance checks** (S)
  - **Description:** `#[ignore]` checks named `perf_*`: start-up to `initialize`, `list_tasks` on 2,000 tasks and `update_task` over stdio, idle memory, stripped binary size, and `airboss ls` end to end. Each prints the machine.
  - **Acceptance:** every target in the spec is met, and the numbers are recorded in the spec.
  - **Verify:** `cargo test -p airboss-mcp --release -- --ignored perf --nocapture`.
  - **Files:** `tests/perf.rs`, `SPEC-mcp.md`
  - **Depends on:** T6, T8

- [x] **T10. Polish and README** (S)
  - **Description:** tool descriptions and server instructions written for agents; `--help` text; a "Use with agents" section in the README.
  - **Acceptance:** each description says when to call the tool and what to do next; the README has build, `claude mcp add`, the Codex equivalent and the CLI basics.
  - **Verify:** read `tools/list` and `airboss --help` output; tests still pass.
  - **Files:** `src/server.rs`, `src/main.rs`, `README.md`
  - **Depends on:** T9

- [x] **T11. Self-review** (S)
  - **Description:** re-read the whole diff against the spec (code-review-and-quality), fix what's found, tick the spec's success criteria.
  - **Acceptance:** every finding fixed or written down with a reason.
  - **Verify:** the full check suite.
  - **Files:** any in `crates/mcp`, `SPEC-mcp.md`
  - **Depends on:** T10

### Checkpoint 4

- [x] The spec's success criteria are checked off, except the one done by hand (a real T3 thread, once installed).
