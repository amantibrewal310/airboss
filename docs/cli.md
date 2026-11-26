# CLI

The `airboss` command works on the same tasks as the app, from a terminal. Every command except `mcp` acts as you, with the same rights as the app. Agents reach the tasks through `airboss mcp` instead, which acts as the agent.

[Getting started](getting-started.md#install-the-airboss-cli) says how to install it. `airboss --help` and `airboss <command> --help` print the same reference.

## Basics

- **The database** is `~/Library/Application Support/Airboss/airboss.db`, created on first use. Set `AIRBOSS_DB` to use another file, for example a throwaway one to try things in.
- **Tasks are numbered.** `#12` in the output is task `12` in a command.
- **`--json`** on every command except `mcp` prints JSON instead of text. The shapes are the same ones agents get from the [MCP tools](mcp.md).
- **Errors** print one line to stderr, starting `airboss:`, and exit with code 1. The message is the same one an agent would get:

  ```console
  $ airboss set 99 doing
  airboss: There's no task #99. List the tasks to find the right id.
  ```

- **Times** in the output are UTC.
- **Colour** is used only when the output is a terminal and `NO_COLOR` is unset.

Status marks follow the app: `○` todo (and waiting), `◌` proposed, `◑` in progress, `◕` in review, `⊖` blocked, `●` done, `⊗` dropped.

The examples come from one session that started on an empty database. In it, an agent also proposed two tasks (#5 and #6) and imported a two-phase plan (#7 to #9). Long output is trimmed.

## Repos

### `airboss repo add <NAME> <PATH> [--remote <URL>]`

Registers a repo that tasks can be linked to. The name is short, and used everywhere a repo is named.

```console
$ airboss repo add booking-web ~/Workspace/booking-web --remote https://github.com/acme/booking-web
Added repo booking-web at /Users/you/Workspace/booking-web
$ airboss repo add booking-api ~/Workspace/booking-api
Added repo booking-api at /Users/you/Workspace/booking-api
```

### `airboss repo ls`

Lists the registered repos: name, path and remote.

### `airboss repo edit <NAME> [--name <NEW>] [--path <PATH>] [--remote <URL> | --no-remote]`

Renames a repo, points it at the folder it moved to, or changes or clears its remote. Its tasks keep it, so they follow the change. A relative path is taken from the current directory.

```console
$ airboss repo edit booking-web --path ~/Code/booking-web
Edited repo booking-web at /Users/you/Code/booking-web
```

### `airboss repo rm <NAME>`

Removes a repo. It's unlinked from every task that has it first, done ones too, and each of those tasks gets a `repo unlinked` entry in its history. The tasks themselves stay.

```console
$ airboss repo rm booking-api
Removed repo booking-api, and unlinked it from #1, #3
```

### `airboss repo link <TASK> <NAME>` and `airboss repo unlink <TASK> <NAME>`

Links a task to a repo, or unlinks it. A task can have several.

```console
$ airboss repo unlink 1 booking-api
○ #1 Booking flow is linked to booking-web
$ airboss repo link 1 booking-api
○ #1 Booking flow is linked to booking-api, booking-web
```

## Tasks

### `airboss add <TITLE> [--under <TASK>] [--repo <NAME>]… [--body <BODY>]`

Adds a task as todo, at the end of its siblings. Give `--repo` again for more repos.

```console
$ airboss add "Booking flow" --repo booking-web --repo booking-api --body "New booking flow, web and API."
Added ○ #1 Booking flow in booking-api, booking-web
$ airboss add "Slot picker" --under 1 --repo booking-web
Added ○ #2 Slot picker under #1 in booking-web
$ airboss add "Slots endpoint" --under 1 --repo booking-api
Added ○ #3 Slots endpoint under #1 in booking-api
```

### `airboss ls [--all | --ready]`

Shows the tree, one task per line. Done and dropped tasks are hidden unless you pass `--all`, but a done parent still shows when something under it is open. Rejected proposals never show.

```console
$ airboss ls
#1  ○ Booking flow      booking-api, booking-web
#2    ○ Slot picker     booking-web               waiting
#3    ○ Slots endpoint  booking-api
```

`--ready` lists only what can start now: todo tasks with nothing open to wait on, with when each became ready.

```console
$ airboss ls --ready
#1  ○ Booking flow    booking-api, booking-web  ready since 2026-11-25 21:25
#3  ○ Slots endpoint  booking-api               ready since 2026-11-25 21:25
```

### `airboss show <ID>`

Shows one task: status, parent, repos, branch, notes, links, subtasks, run order, what it waits on and blocks, and its history.

```console
$ airboss show 1
#1 Booking flow
Status    ○ Todo  added by you, 2026-11-25 21:25
Repos     booking-api, booking-web
Notes     New booking flow, web and API.
Subtasks  ○ #2 Web: slot picker
          ◑ #3 Slots endpoint
Run order Wave 1  #3
          Wave 2  #2
History
  2026-11-25 21:25  me  created
  2026-11-25 21:25  me  edited   branch: booking-flow
  2026-11-25 21:25  me  edited   branch cleared
```

### `airboss edit <ID> [--title <TITLE>] [--body <BODY>]`

Changes a task's title or notes. An empty `--body ""` clears the notes.

```console
$ airboss edit 2 --title "Web: slot picker"
Edited ○ #2 Web: slot picker
```

### `airboss mv <ID> [--under <TASK> | --top] [--pos <N>]`

Moves a task, with everything under it. `--pos` is its place among its new siblings, from 1; left out, it goes at the end. With neither `--under` nor `--top`, `--pos` reorders it among its current siblings.

```console
$ airboss mv 4 --under 1 --pos 1
Moved ○ #4 Write up results under #1, position 1
$ airboss mv 4 --top
Moved ○ #4 Write up results at the top level, position 2
```

### `airboss branch <ID> <NAME>` and `airboss branch <ID> --clear`

Sets the branch that every thread for the task shares, or clears it. Do it uses this branch, or makes one (`task-<id>-<slug>`) if there's none. A name uses only letters, digits, `.`, `_`, `/` and `-`.

```console
$ airboss branch 1 booking-flow
○ #1 Booking flow is on branch booking-flow
$ airboss branch 1 --clear
○ #1 Booking flow has no branch now
```

## Status

### `airboss set <ID> <todo|doing|in_review|blocked> [--note <NOTE>]`

Sets a task's status. `blocked` needs `--note` saying why. For done and dropped, use `done` and `drop`.

```console
$ airboss set 3 doing
◑ #3 Slots endpoint is now in progress
$ airboss set 3 blocked --note "Waiting on the schema"
⊖ #3 Slots endpoint is now blocked
```

### `airboss done <ID> [--note <NOTE>]`

Marks a task done. You can finish any task, even one whose PR hasn't merged; agents can't.

```console
$ airboss done 4 --note "Wrote it up"
● #4 Write up results is now done
```

### `airboss drop <ID> [--note <NOTE>]`

Drops a task, and every open task under it. Tasks already done stay done.

```console
$ airboss drop 9 --note "Folded into phase 1"
⊗ #9 Phase 2 is now dropped
```

### `airboss accept <ID>` and `airboss reject <ID> [--note <NOTE>]`

Accepts an agent's proposal, which makes it todo, or rejects it. A rejected proposal is dropped and hidden from the tree, and stays in history with your note.

```console
$ airboss ls
#1  ○ Booking flow        booking-api, booking-web
#2    ○ Web: slot picker  booking-web               waiting
#3    ◕ Slots endpoint    booking-api
#5    ◌ Add slot caching  booking-api               proposed, waiting for you
#6    ◌ Nope                                        proposed, waiting for you
$ airboss accept 5
Accepted ○ #5 Add slot caching; it's todo now
$ airboss reject 6 --note "Not now"
Rejected ⊗ #6 Nope; it's dropped and hidden from the tree
```

## Dependencies

### `airboss wait <ID> --on <TASK>` and `airboss unwait <ID> --on <TASK>`

Makes a task wait on another, or stop waiting. A waiting task isn't ready until everything it waits on is done or dropped. A loop, or a task waiting on its own parent or subtask, is refused.

```console
$ airboss wait 2 --on 3
○ #2 Slot picker now waits on ○ #3 Slots endpoint
$ airboss unwait 2 --on 3
○ #2 Web: slot picker no longer waits on ◕ #3 Slots endpoint
```

## Links

### `airboss link <ID> <KIND> <REF> [--url <URL>] [--title <TITLE>]`

Links something to a task. `KIND` is one of:

| Kind | `REF` |
|---|---|
| `pr` | `owner/repo#42`, or the PR's GitHub URL |
| `github_issue` | `owner/repo#7` |
| `slack` | the message's permalink |
| `doc` | a file path |
| `t3_thread` | a T3 Code thread id |
| `herdr_terminal` | a Herdr terminal's address, such as `t4p` |
| `claude_session` | a Claude Code session id |

A PR is stored as `owner/repo#n` in lowercase, however you give it, so the same PR is never linked twice. Linking the same thing again returns the link that's there.

```console
$ airboss link 3 pr acme/booking-api#42 --title "Slots endpoint"
Linked pr acme/booking-api#42 to ◑ #3 Slots endpoint
$ airboss link 2 pr https://github.com/Acme/Booking-Web/pull/7
Linked pr acme/booking-web#7 to ◑ #2 Web: slot picker
$ airboss link 2 slack https://acme.slack.com/archives/C01/p123 --title "#booking thread"
Linked slack https://acme.slack.com/archives/C01/p123 to ◑ #2 Web: slot picker
```

## History

### `airboss log [--after <EVENT>]`

Shows recent history across every task, newest last: the last 50 events, or every event after an event id. Each line starts with its event id, the number `--after` takes; `--json` has it as `id`.

```console
$ airboss log
1  2026-11-25 21:25  #1  me  created
2  2026-11-25 21:25  #2  me  created
3  2026-11-25 21:25  #3  me  created
4  2026-11-25 21:25  #2  me  waits on       #3
5  2026-11-25 21:25  #3  me  → in progress
6  2026-11-25 21:25  #3  me  → blocked      Waiting on the schema
7  2026-11-25 21:25  #3  me  → in progress
8  2026-11-25 21:25  #3  me  linked         pr acme/booking-api#42
$ airboss log --after 6
7  2026-11-25 21:25  #3  me  → in progress
8  2026-11-25 21:25  #3  me  linked         pr acme/booking-api#42
```

The actor column says who made each change: `me`, `agent:<thread>`, `sync:gh`, `sync:t3`, `plan` or `launch`.

## Backup

### `airboss backup <PATH> [--force]`

Writes a copy of the whole database to `PATH`: tasks, history, links, and what sync, Do it and Needs me keep. It uses SQLite's online backup, so the copy is one consistent moment even while the app and agents are writing, and nobody has to stop. A file already at `PATH` is kept unless you give `--force`, and the database itself is never overwritten.

```console
$ airboss backup ~/Backups/airboss-2026-11-26.db
Backed up the database to /Users/you/Backups/airboss-2026-11-26.db
```

To restore, quit Airboss, then copy the backup over `~/Library/Application Support/Airboss/airboss.db` and delete the `airboss.db-wal` and `airboss.db-shm` beside it. To look inside a backup without touching your tasks, point the CLI at it: `AIRBOSS_DB=~/Backups/airboss-2026-11-26.db airboss ls`.

## For agents

### `airboss mcp`

Serves the MCP tools over stdin and stdout, for one agent thread. You don't run it yourself; your agent tool starts it, one process per thread. See [Getting started](getting-started.md#connect-your-agents) and [MCP tools](mcp.md).

## JSON

`--json` prints the same shapes the MCP tools return. For example:

```console
$ airboss add "Fix booking typo" --json
{
  "id": 10,
  "title": "Fix booking typo",
  "status": "todo",
  "repos": [],
  "origin": "me",
  "created_at": "2026-11-25T21:30:42.907Z"
}
```

`airboss ls --json` prints `{tasks, total, truncated}`, like `list_tasks` but with no 200-task cap, and `airboss show --json` prints the same object as `get_task`.
