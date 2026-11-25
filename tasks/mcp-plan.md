# Implementation plan: mcp

Module `mcp` from [CAPABILITIES.md](../CAPABILITIES.md). It builds what's in [SPEC-mcp.md](../SPEC-mcp.md), aligned with `core` v2 on 25 Nov 2026. The tasks and their checkpoints are in [mcp-todo.md](mcp-todo.md).

State: **built 26 Nov 2026**. All 11 tasks and 4 checkpoints done.

## Overview

`mcp` is one Rust crate, `airboss-mcp`, that builds one binary, `airboss`. `airboss mcp` is a per-thread MCP server over stdio with six tools. Every other subcommand is a small CLI that acts as `me`. Both go through `airboss_core::Store` and never touch SQL.

The work is cut into thin slices. Each one adds a behaviour end to end with its tests written first: identity, then the tools as plain functions, then the stdio server around them, then the CLI, then the performance checks and polish.

## Architecture decisions

- **A library plus a thin binary.** `src/lib.rs` holds the modules (`identity`, `render`, `tools`, `server`, `cli`). `src/main.rs` only parses arguments and dispatches. The tool tests call `tools::*` in-process against `Store::open_in_memory()`, which needs a library target.
- **Tools are plain functions.** Each takes `&mut Store`, the `Identity` and typed arguments, and returns a typed view or a `ToolError`. `server.rs` is only glue: rmcp's `#[tool_router]` methods lock the store, call the function and turn the result into JSON text.
- **Identity is a pure function** over a variable lookup and the parent pid, so every row of the identity table is a unit test. The process works it out once, at start-up.
- **One message per `core::Error` variant, in `render.rs`.** `core` errors carry no detail, so the tool gathers the facts the message needs (the task's current status, whether it has a PR, the parent's status) only after a call fails. The happy path pays nothing.
- **Shared shapes.** `render.rs` defines the JSON views (task, node, link, event, task detail) that the tools return and the CLI prints with `--json`, so an agent and I see the same shape.
- **The server runs on a current-thread Tokio runtime**, built only for `airboss mcp`. The CLI never starts a runtime. The `Store` sits behind a `std::sync::Mutex`, held only for the length of one synchronous `core` call.
- **Repos by name.** Each call that takes or shows repos reads `repos()` once and maps names to ids and back.

## Dependency graph

```
T1 crate + rmcp/identity checks
 └─ T2 identity
     └─ T3 errors + read tools (list_tasks, get_task)
         ├─ T4 update_task + link
         │   └─ T5 propose_tasks + import_plan
         │       └─ T6 stdio server
         └─ T7 CLI reads (ls, show, log, repo ls)
             └─ T8 CLI writes
T9 performance checks   (after T6 and T8)
T10 polish + README     (after T9)
T11 self-review         (last)
```

## Phases

1. **Foundation (T1–T2):** the crate, the confirmed rmcp API, and identity.
2. **Tools (T3–T5):** all six tools as plain functions, with every `core` error's message.
3. **Server (T6):** the tools over real stdio.
4. **CLI (T7–T8):** every command, with `--json`.
5. **Finish (T9–T11):** performance, polish, README, review.

## Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| rmcp 3.11's macros differ from older examples | Medium | T1 reads the crate source first and records the exact calls in the spec. |
| A Claude or Codex thread doesn't pass its thread id to the server | High | T1 checks it with a probe server under the real clients. Identity falls back to `AIRBOSS_THREAD`, then to `agent:unknown/<ppid>` with a warning. |
| `propose_tasks` half-succeeds (core has no batch add for agents) | Medium | Check every title, repo name and the parent before the first write, so only a database failure can stop it partway. |
| `import_plan` can't put "imported by" in the `created` event body (core's `add_tasks` takes no body) | Low | Record it as a note by the agent right after the import, on each task it created. Noted in the spec. |
| The 15 MB idle-memory target with a Tokio runtime | Low | Current-thread runtime, no worker pool; measured in T9. |
| Tests touching the real database | High | Every test sets `AIRBOSS_DB` to a temp dir; the binary reads only that path when it's set. |

## Parallel work

Built in order in one thread: the crate is small and T3–T8 all touch `render.rs`.

## Open questions

None.
