# MCP tools

`airboss mcp` is an MCP server over stdio, one process per agent thread. It gives agents six tools to read their tasks and report on them. This page is the reference; [Getting started](getting-started.md#connect-your-agents) says how to connect a client.

The tools are kept few on purpose: fewer tools means agents pick the right one more often. Each returns compact JSON as one text block. A refusal comes back as a tool error (`isError: true`) with one sentence that says what to do next, so the agent can recover by itself.

## Who the agent is

The server works out who it's acting for once, at start-up, from its environment. Tool arguments never name the actor.

| Variable present | Actor | "This thread" means |
|---|---|---|
| `T3CODE_THREAD_ID` | `agent:t3/<id>` | that T3 thread, plus the Claude Code session if `CLAUDE_CODE_SESSION_ID` is set too |
| otherwise `HC_TERMINAL_ID` (set by Herdr in its terminals) | `agent:herdr/<address>` | that Herdr terminal, plus the Claude Code session if `CLAUDE_CODE_SESSION_ID` is set too |
| otherwise `CLAUDE_CODE_SESSION_ID` | `agent:claude/<id>` | that Claude Code session |
| otherwise `AIRBOSS_THREAD` | `agent:<value>` | the thread it names, if it's written `t3/<id>`, `herdr/<address>` or `claude/<id>`; otherwise nothing |
| none of these | `agent:unknown/<parent pid>` | nothing |

- A thread host (T3 Code, Herdr) names the thread before the agent it runs, so an agent's own link is the one Do it made, and nothing shows twice. The server's instructions name the one kind to link with.
- Values are trimmed, and a blank value counts as unset. A leading `agent:` in `AIRBOSS_THREAD` is ignored.
- Over MCP, the caller is always an agent. Nothing an agent sends can make it act as you. The one exception is `import_plan`, which creates tasks as `plan`, the actor for a plan you settled.
- When no thread can be identified, `mine: true` and linking "this thread" are refused, and the server's instructions end with a warning:

  > This server couldn't tell which thread it serves: none of T3CODE_THREAD_ID, HC_TERMINAL_ID, CLAUDE_CODE_SESSION_ID or AIRBOSS_THREAD is set. It acts as agent:unknown/73060, so `mine` and linking this thread without a `ref` won't work. Set AIRBOSS_THREAD in this client's MCP config to fix it.

### What the actor proves

The actor records who did something; it doesn't prove it. The server believes its environment, and the CLI acts as `me` for whoever runs it. So any process running as your macOS user can act as you through the CLI, or start `airboss mcp` with another thread's id and report as that thread. Its changes still obey that actor's rules, and history shows them under the name it claimed.

That's acceptable for a single-user tool that runs only on your Mac: anything that can run as your user can already read and change the database file directly. Airboss's rules keep agents from making the changes you reserve for yourself by mistake, not from software set on getting round them.

## Server instructions

Sent with the `initialize` result, naming the actor:

> Airboss is the user's to-do list, and these tools report on the tasks you work on. You act as agent:t3/97b5…. When you start, find your task with list_tasks (`mine: true`, or your repos), or use the task id in your brief, and read it with get_task. Link this thread to it (link, kind t3_thread or claude_session, no ref). Report status changes with update_task as they happen, and link every PR you open. Propose new work with propose_tasks; don't create it. Don't widen your own task: ship your part so it's safe without a missing piece, and propose that piece, such as another repo's work, as a sibling that says what it blocks. Use import_plan only for a plan the user settled in plan-review.

## What agents may do

The rules live in the task store, not in the server, so the CLI and the app follow the same ones.

- **Status:** an agent can move a task that is todo, in progress, in review or blocked to in progress, in review or blocked. Blocked needs a note saying why.
- **Done:** an agent can finish a task only if it has **no** PR link, and only with a note saying what it did. A task with a PR finishes when GitHub says its PRs merged.
- **Parents:** a task with subtasks has no status of its own; it shows what its subtasks add up to. Changing its status, or linking a PR to it, is refused with a message pointing at its subtasks. Notes still go on it.
- **New work** arrives as proposals, which wait for you to accept or reject them. A proposal lands next to the agent's task, follows it, and may say what it blocks; it holds nothing up until you accept it.
- **There are no tools** to delete, move or rename tasks, change a task's repos or branch, add dependencies outside a plan or a proposal, accept proposals, drop tasks or set a PR's state. Those are yours, or belong to PR sync.

Every change an agent makes is recorded in the task's history under its actor.

## The tools

Repos are always named, never numbered, in arguments and results. An unknown repo name is an error that lists the registered ones. Ids are numbers: `12` for task `#12`.

### `list_tasks`

> List tasks in tree order. Start here: `mine: true` finds the tasks linked to this thread; otherwise filter by `repos`, `status`, `ready`, `query` or `priority` (p0 most urgent to p3). `sort: "priority"` puts the most urgent first. Done and dropped tasks are left out unless `status` names them. Returns at most 200 (`truncated` says there were more). Next: get_task on the task you'll work on.

Read-only. Every filter is optional, and they combine.

| Argument | Type | Meaning |
|---|---|---|
| `repos` | list of names | Tasks linked to any of these repos |
| `status` | list of statuses | Tasks in these statuses: `proposed`, `todo`, `doing`, `in_review`, `blocked`, `done`, `dropped`. Left out, every status but done and dropped. |
| `mine` | bool | Tasks linked to this thread |
| `ready` | bool | Tasks that can start now: todo, with nothing open to wait on |
| `query` | text | Tasks whose title contains this, ignoring case |
| `priority` | list of `p0`–`p3` | Tasks with any of these priorities; tasks with none are left out |
| `sort` | `tree` or `priority` | `tree` (the default) keeps tree order. `priority` puts P0 first and tasks with no priority last; ties keep tree order. The cap applies after sorting. |

Returns `{tasks, total, truncated}`. `total` is how many matched before the cap of 200.

```json
{"tasks":[{"id":3,"title":"Slots endpoint","status":"doing","parent":1,"depth":1,"waiting":false,"repos":["booking-api"]}],"total":1,"truncated":false}
```

`waiting` is true while the task waits on something that isn't done or dropped. `parent` is left out for a top-level task, and `priority` (`"p0"` to `"p3"`) when none is set; `get_task`'s task does the same.

### `get_task`

> Read one task before working on it: notes, repos, branch, links, the last 20 history entries, what it waits on and what waits on it, subtasks and their run order. Next: link this thread to it, then update_task as you go.

Read-only.

| Argument | Type | Meaning |
|---|---|---|
| `id` | number | The task |

Returns one object:

| Field | What |
|---|---|
| `task` | The task: `id`, `title`, `status`, `blocked_reason`, `parent`, `repos`, `branch`, `body` (its notes), `origin` (`me`, `agent` or `plan`), `plan_key`, `created_at`. Empty fields are left out. |
| `waiting` | True while anything it waits on is open |
| `links` | Every link: `id`, `task`, `kind`, `ref`, `title`, `url`, `pr_state`, `linked_by`, `linked_at` |
| `history` | The last 20 events, oldest first: `id`, `task`, `at`, `actor`, `kind`, `to_status`, `body` |
| `history_total` | How many events its whole history has |
| `dependencies` | `waits_on` and `blocks`, each a list of `{id, title, status}`, finished ones included |
| `subtasks` | Its direct subtasks, in order, as `{id, title, status}` |
| `run_order` | Only when it has subtasks: `waves` (lists of subtasks), `waiting_outside` (each `{task, blockers}`) and `tangled` |

Threads that Do it started for the task show up in `links` as `t3_thread` links, with the repo's name at the end of their title when the task has several repos. That's how sibling threads find each other.

### `update_task`

> Report progress on a task: set `status`, add a `note`, or both. Set doing when you start, in_review when your PR is open, blocked with a `note` saying why. Set done with a `note` saying what you did, but only on a task with no PR: a task with a PR finishes by itself when the PR merges. A note alone records progress. A task with subtasks has no status of its own: it shows what they add up to, so update the subtask you're on. When the user gives a deadline ("by Friday", "in 2 days"), set `due` to that day as YYYY-MM-DD, worked out from today's date in their time zone; "" clears it. The task shows in their Needs me from that day.

| Argument | Type | Meaning |
|---|---|---|
| `id` | number | The task |
| `status` | `doing`, `in_review`, `blocked` or `done` | The new status. The schema offers only the statuses an agent may set. |
| `note` | text | With `status`: why it's blocked, or what was done. Alone: a progress note. |
| `due` | text | The day it's due, `YYYY-MM-DD` in the user's time zone; `""` clears it. Set first, so a bad date writes nothing. |

Give `status`, `note`, `due`, or several. Returns the task after the change, in the same shape as `get_task`'s `task`.

### `link`

> Attach something to a task. With kind t3_thread, herdr_terminal or claude_session and no `ref`, it links this thread: do that when you start on a task, with the kind the server's instructions name. Link every PR you open (kind pr, ref owner/repo#42, plus `url`), to the task it does: on a task with subtasks, to the subtask. Linking the same thing twice returns the existing link.

Idempotent.

| Argument | Type | Meaning |
|---|---|---|
| `id` | number | The task |
| `kind` | `t3_thread`, `herdr_terminal`, `claude_session`, `pr`, `slack`, `doc` or `github_issue` | What it points at |
| `ref` | text | The thread id, terminal address (`t4p`), `owner/repo#n`, permalink or file path. Leave it out for `t3_thread`, `herdr_terminal` or `claude_session` to link this thread. |
| `url` | text | The web page it lives at, if any |
| `pr_state` | `open`, `merged` or `closed` | For a `pr` link only: its state, if known. Left out, it starts open. |
| `title` | text | A short name to show instead of the ref |

A PR ref can be `owner/repo#42` or the PR's GitHub URL. It's stored as `owner/repo#42` in lowercase. Linking an open PR to a done task moves the task back to in review.

Returns the link:

```json
{"id":2,"task":3,"kind":"t3_thread","ref":"demo-thread-1","linked_by":"agent:t3/demo-thread-1","linked_at":"2026-11-25T21:25:23.267Z"}
```

### `propose_tasks`

> Propose 1 to 20 new tasks you spotted. They land next to your own task, under its parent (`place: "sibling"`, the default), or at top level with `place: "top"`; a `parent` you give wins over both. Each follows your own task: Airboss records that itself. Give each the ids it `blocks`, picked from what waits on your own task (get_task's `blocks`); they wait on it only once it's accepted. They arrive as proposed and wait for the user to accept or reject them, so don't start on one until it's accepted.

| Argument | Type | Meaning |
|---|---|---|
| `place` | `sibling` or `top` | Where they go without `parent`: next to your own task, under its parent (the default), or at top level. |
| `parent` | number | The task to put them under. Given, it wins over `place`. |
| `tasks` | list of 1 to 20 `{title, body?, repos?, blocks?}` | `title` says what to do; `body` says why, and anything the user needs to decide; `repos` are the repos it touches, by name; `blocks` are the ids of tasks that can't finish without it |

Your own task is the one this thread is linked to, or of several, the one it was linked to last. Each proposal waits on it, which the app shows as "follows #10", unless the proposal sits under it; with no linked task, `sibling` means top level and nothing is followed. Each `blocks` id waits on the proposal. A proposal holds nothing up until you accept it, so a rejected one never did.

Everything is checked before the first task is added, so a bad title, repo, parent or `blocks` id adds nothing. Returns the proposed tasks, each with `status: "proposed"` and `origin: "agent"`.

### `import_plan`

> Import a plan the user settled in plan-review as a tree of tasks, under `parent` or at top level. Only for a settled plan: `plan_file` must be that plan's file, and it gets linked to the top-level tasks. Give each step a stable `key`; sending the plan again adds only steps with new keys.

Idempotent. The tasks are created as `plan`, so they start as todo, not proposed.

| Argument | Type | Meaning |
|---|---|---|
| `plan_file` | text | The settled plan's file, absolute or relative to the thread's folder. It must exist. |
| `parent` | number | The task to put the plan under. Left out, top level. |
| `plan` | list of steps | The plan's top-level steps |

Each step:

| Field | Type | Meaning |
|---|---|---|
| `key` | text | The plan's own id for the step, unique in the plan. Sending the plan again matches steps by key, so keep keys stable. |
| `title` | text | What to do |
| `body` | text | Notes for whoever picks it up |
| `repos` | list of names | The repos it touches |
| `depends_on` | list | What it waits on: other steps' keys (text), or existing task ids (numbers) |
| `children` | list of steps | Its own steps |

The whole plan is checked before anything is written; if any step fails, nothing is created. The plan file is linked as a `doc` to each top-level task, and each new task gets a note from the thread that sent it, such as "Imported from plan.md". Sending the same plan again creates nothing and adds no notes.

Returns which tasks were created, which existed already, and the task each key became:

```json
{"created":[7,8,9],"existing":[],"keys":{"p1":7,"p1.1":8,"p2":9}}
```

## Errors

Every refusal is one of these sentences, filled in with the task, status or name the call was about.

**Status and notes**

- "Agents can't finish a task that has a PR; it finishes when the PR merges. Set it to in_review instead."
- "Say what you did in `note` when finishing a task."
- "Say why it's blocked in `note`."
- "Agents can set doing, in_review, blocked, or done (on a task with no PR); only the user can set {status}."
- "Task #6 is dropped, so its status can't change from here. Ask the user to reopen it if there's more to do." For a proposed task the last part is "Wait for the user to accept it."
- "Give `status`, `note`, `due`, or several."
- "A due date is a real day written YYYY-MM-DD, e.g. 2026-12-04. Work out \"Friday\" or \"in 2 days\" from today's date in the user's time zone first."

**Ids, names and files**

- "There's no task #99. List the tasks to find the right id."
- "There's no repo named \"nope\". Registered repos: booking-api, booking-web." With no repos registered: "No repos are registered yet; leave `repos` out."
- "There's no file at \"/nope.md\". Give the path of the plan file the user settled in plan-review."
- "A PR is owner/repo#42 or its GitHub URL, like https://github.com/owner/repo/pull/42."
- "Give every task a title that isn't blank."

**This thread**

- "This server doesn't know which thread it serves, so `mine` can't work. Use `repos` or `query` instead. It acts as agent:unknown/…."
- "This server doesn't know its T3 thread id, so give `ref`." (or "its Claude Code session id")
- "Give `ref` for a pr link, e.g. owner/repo#42." and the same for the other kinds

**Tree, plans and limits**

- "Task #4 is done, so nothing can go under it. Pick an open parent or none."
- "A task can't wait on its own parent or subtask. Remove that dependency."
- "That would make a loop of tasks waiting on each other. Remove one of the dependencies."
- "The plan uses the key \"p1\" twice. Give every step its own key."
- "Give `plan` at least one step."
- "Propose between 1 and 20 tasks at a time."
- "`pr_state` only goes with a `pr` link; leave it out."

**The database**

- "This database was created by a newer Airboss. Update the `airboss` command." A newer app has migrated the file, so this older binary refuses to write to it. A server that was running already says this for every write; a new one doesn't start.
- "The Airboss database is busy. Try again in a moment."
- "The Airboss database failed (…). Try again; if it keeps failing, tell the user."

**Malformed calls.** Arguments that don't match a tool's schema come back as a tool error from the MCP library, naming the field, such as "failed to deserialize parameters: unknown variant \`todo\`, expected one of \`doing\`, \`in_review\`, \`blocked\`, \`done\`". Unknown fields are refused. Only an unknown tool name is a protocol error (`-32602`, "tool not found").

## Performance

Measured in release mode on an Apple M4 against 2,020 tasks: start-up to answering `initialize` 2.3 ms, `list_tasks` and `get_task` 3.4 ms and `update_task` 0.1 ms at the 95th percentile, 8.6 MB of memory per idle process. The figures and how they were taken are in [SPEC-mcp.md](../SPEC-mcp.md#performance-targets).
