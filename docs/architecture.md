# Architecture

Airboss is a Rust workspace with a Svelte interface. Everything shares one SQLite file. This page is the map; each module's spec has the detail.

## Modules

| Path | Crate | What it does | Spec |
|---|---|---|---|
| `crates/core` | `airboss-core` | The task tree, dependencies, links, repos and history in SQLite. It enforces every rule about who may change what. Every other module goes through its `Store`; none touches its tables. | [SPEC-core.md](../SPEC-core.md) |
| `crates/mcp` | `airboss-mcp` | The `airboss` binary: `airboss mcp`, the per-thread MCP server, and the CLI that acts as you | [SPEC-mcp.md](../SPEC-mcp.md) |
| `crates/services` | `airboss-services` | The contract between the app and the three services: shared types and traits, `std` only | [README](../crates/services/README.md) |
| `crates/sync` | `airboss-sync` | Live state from outside: GitHub PRs through `gh`, T3 Code threads from T3's database, Claude Code sessions from their files | [SPEC-sync.md](../SPEC-sync.md) |
| `crates/launch` | `airboss-launch` | Do it and Plan it: writes the brief and runs `t3 thread start` | [SPEC-launch.md](../SPEC-launch.md) |
| `crates/attention` | `airboss-attention` | Needs me: decides what is waiting on you | [SPEC-attention.md](../SPEC-attention.md) |
| `app/src-tauri` | `airboss-app` | The Mac app: windows, menus, menu-bar item, hotkey, the view model, commands, and the host for the three services | [SPEC-app.md](../SPEC-app.md) |
| `app/src` | | The Svelte 5 interface: the main window and the quick-add panel | [SPEC-app.md](../SPEC-app.md), [DESIGN.md](../DESIGN.md) |

Dependencies point one way:

```
core ◀── mcp
core ◀── services ◀── sync, launch, attention ◀── app
```

`attention` and `launch` don't depend on the `sync` crate. They use `sync` only through the `SyncService` trait in `services`, and the app wires them together.

## Processes

There are two kinds of process, and they share nothing but the database file.

```
                 ┌──────────────────────── Airboss.app (one process) ────────────────────────┐
  you ──────────▶│  main window (web view)      quick-add panel (web view)    menu-bar item  │
                 │        │ commands ▲ patches          │                                    │
                 │        ▼          │                  ▼                                    │
                 │  view model ◀── catch_up() ◀── kqueue on airboss.db-wal                   │
                 │        │                                                                  │
                 │   core Store    sync ──▶ gh, T3 database (read-only), Claude sessions     │
                 │                 launch ──▶ t3 thread start                                │
                 │                 attention                                                 │
                 └───────────────────────────────┬───────────────────────────────────────────┘
                                                 │ reads and writes
                                                 ▼
          ~/Library/Application Support/Airboss/airboss.db   (SQLite, WAL)
                                                 ▲
                         ┌───────────────────────┼───────────────────────┐
                         │                       │                       │
                  airboss mcp              airboss mcp              airboss (CLI)
                  (agent thread 1)         (agent thread 2)         (you, in a terminal)
                         ▲                       ▲
                  stdio  │                stdio  │
                    agent client            agent client
```

**The app** is one process, `Airboss.app`. It holds one connection to the database and runs the three services as libraries, each with its own small worker threads:

- `sync` has a T3-and-Claude worker and a GitHub worker, plus a watcher thread.
- `launch` has one worker that checks whether T3 Code is there and settles starts that didn't answer.
- `attention` has a worker and a thread that waits for `sync` to change.

The app adds a catch-up worker, the database watcher, and one listener per service. They're all plain threads that block until something happens. There's no timer polling the database. The only timer is `sync` reading GitHub every 5 minutes.

**Each agent thread** starts its own `airboss mcp` process, which talks MCP over stdin and stdout and opens the same database file. There's no port, no token and no shared server, and it works whether the app is running or not. The CLI is the same binary, run once per command.

**Writing at once** is safe: `core` uses SQLite's WAL mode and IMMEDIATE transactions, and its tests run several writers together.

**Versions.** Every module records its migrations in `schema_migrations`. A binary that finds a version newer than it knows refuses the file, and an open connection checks before every write, so an `airboss mcp` left running after an update stops writing instead of breaking rules it doesn't know.

**Who did what.** Every change carries its actor (`me`, `agent:t3/<id>`, `sync:gh` and so on), and `core` enforces each actor's rights. The actor records who did something but doesn't prove it: `airboss mcp` takes its identity from its environment, and the CLI acts as `me`. So any process running as your user can act as you through the CLI, or claim to be another thread. That's acceptable for a single-user local tool, since such a process could open the database file itself anyway. The rights stop an agent doing your part by mistake; they aren't a security boundary between processes you run.

## How a change reaches the window

Say an agent calls `update_task`. Its `airboss mcp` process commits to the database, which writes to `airboss.db-wal`.

1. **kqueue.** The app watches the `-wal` file with kqueue, and the database's folder too, so it re-arms when the WAL is deleted and created again. FSEvents isn't used: it doesn't report writes to a WAL file that a connection keeps open.
2. **Debounce.** A burst of events is gathered for 30 ms after the last one, and never more than 150 ms after the first.
3. **Catch up.** The catch-up worker reads the events after its cursor. If nothing changed, it stops. Otherwise it takes one consistent `snapshot()` of the tree, the dependency edges and the ready list, hands the change to `sync`, `launch` and `attention`, and rebuilds the view model.
4. **Patch.** It diffs the new rows against the old and sends the window a patch: the rows that changed, the ids removed, and the order only if it changed. Each patch has its own increasing `seq`, and the interface applies a patch only if its `seq` is newer than the last one. Svelte never works out tree facts itself.

Writes from the app go through the same `catch_up()` inline, and each write command returns its patch. A service that changes, such as `sync` seeing a new PR state, triggers a rebuild from the last snapshot without reading the database again. When the window gains focus or the Mac wakes, the app checks SQLite's `data_version` as a safety net.

The target is an agent's write showing in an open window within 300 ms.

## Who talks to what outside

| Module | Talks to | How |
|---|---|---|
| `sync` | GitHub | `gh api graphql`, one batched call per signed-in account per pass, every 5 minutes and on demand. Each account's token is held in memory and passed only in the child's environment. |
| `sync` | T3 Code | Its database `~/.t3/userdata/statev2.sqlite`, opened read-only whenever its WAL changes |
| `sync` | Claude Code | The session files in `~/.claude/sessions`, watched with kqueue, and `ps` to check each session's process is alive |
| `launch` | T3 Code | `~/.t3/bin/t3 thread start --json …`, with the brief on stdin |
| `launch` | git | `/usr/bin/git`, to find a repo's default branch and check branch names |
| `app` | your browser, T3 Code, Finder | Opens `http`, `https`, `file` and `t3code://` links |

The windows load nothing from the network. Their content security policy is `default-src 'self'`. The app's only own network use is the updater, in Rust (`updates.rs`): the manifest on GitHub and a signed release download ([SPEC-updates](../SPEC-updates.md)).

## Data model

Everything is in one SQLite file. `core` owns six tables:

| Table | One row is |
|---|---|
| `tasks` | A task, at any depth: `parent_id`, `title`, `body`, `status`, `blocked_reason`, `position` among its siblings, `origin` (`me`, `agent` or `plan`), `plan_key`, `branch` |
| `repos` | A git repo tasks can point at: `name`, `path`, `remote_url` |
| `task_repos` | One repo a task is linked to. A task can have any number. |
| `task_deps` | One task waiting on another |
| `links` | One outside thing on a task: a T3 thread, a Claude Code session, a PR (with its state), a Slack message, a doc or a GitHub issue |
| `task_events` | One change to one task, with its actor and time. Event ids only grow and are never reused, so an id works as a cursor. |

**Statuses** are `proposed`, `todo`, `doing`, `in_review`, `blocked`, `done` and `dropped`. "Waiting" isn't stored: a task is waiting while anything it waits on is open. Last activity, ready-since, a parent's repos and "all PRs merged" are worked out too, not stored.

**Actors** are `me`, `agent:<thread>`, `sync:gh`, `sync:t3`, `plan` and `launch`. `core` decides what each may do. For example, only `me` accepts proposals and drops tasks, an agent finishes only a task with no PR, and `sync:gh` finishes a task only when its PRs merged.

Each service keeps its own tables in the same file, through its own connection, and touches `core`'s tables only through `Store`:

| Module | Tables |
|---|---|
| `sync` | `observed_threads`, `observed_prs`, `sync_health`, `sync_finish_held` |
| `launch` | `launch_threads`, a record written before every `t3` call so a retry never starts a second thread; `launch_repos` |
| `attention` | `attention_hidden`, the items you dismissed or snoozed |

Migrations are numbered files under each crate's `src/migrations/`, recorded per module in `schema_migrations`, and append-only.

The app keeps its settings in `app-settings.json` next to the database, and logs to `~/Library/Logs/Airboss/app.log`.

## The interface

The Rust side owns the view model, and the interface draws it.

- Every command is a plain Rust function in `app/src-tauri/src/commands/`, run off the main thread. All `core` calls from the app act as `me`.
- The types the interface sees are generated from Rust with `ts-rs`, into `app/src/lib/types/generated/types.ts`.
- The command registry in `app/src/lib/logic/commands.ts` builds the keymap, the native Task and View menus, the context menu, ⌘K's command list and the shortcut sheet.
- The task list is our own virtual list, so 2,000 rows scroll at 60 fps.
- In the browser, a mock backend replays responses recorded from Rust (`pnpm dev:mock`), so the interface can be worked on and tested without the app.
- Undo lives in Rust: a stack of inverse calls, 20 deep.
