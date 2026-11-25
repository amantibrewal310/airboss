# Airboss: idea one-pager

Agreed 25 Nov 2026 using idea-refine. This sets the direction only. The plan and phases come next, through plan-review.

## Problem statement

How might one engineer drop any task, from a one-line bug fix to a whole new product, into a single list, hand it to agents in one keystroke, and always see where every task stands without opening a thread to ask?

Evidence from 5 Oct to 25 Nov 2026:

- 35 of 853 threads were "where are we?", "what's remaining?" or "find thread X" check-ins.
- A stack of 10 PRs sat idle from 4 Nov to 23 Nov without anyone noticing.
- Plan docs drifted out of date. One plan's status section was last updated 3 Nov.
- Tasks live in my head, in Slack saved items and in Linear or Jira.

## Who it's for

Me first: one engineer running many T3 Code and Claude Code threads in parallel. It will be open source, so others can use it, but there's no onboarding for other setups in v1.

## Success after four weeks

- **No "where are we" threads.** I check status in Airboss.
- **Work starts from the list.** Most new threads come from a task's Start button.
- **Nothing stalls unseen.** Anything quiet for 5+ days shows up on its own.

## Recommended direction

A **menu-bar app** (Tauri) with a global hotkey and a **task tree** of any depth. The home view is **what's waiting on me**.

Every task has two buttons:

- **Do it** opens a T3 thread with the task, its notes and its Slack link as the first message. The thread is linked to the task.
- **Plan it** runs plan-review. Once I settle the plan, its phases and steps become the task's subtasks, so the plan and its status can't drift apart.

Status comes from three sources:

- **Agents** report through a local MCP server.
- **GitHub PRs** are checked with `gh`. A merged PR moves its task to done.
- **T3 thread state** is read for linked threads by watching T3's database read-only. It feeds "needs me" (questions, failed runs) and history, but never changes a task's status (SPEC-core rule 6).

Why it's different: to-do apps and trackers don't know agent threads exist. T3 knows threads, but not tasks or plans. Airboss joins the task, its threads, its PRs and its plan.

## Settled rules

- **Done** comes from me ticking it, or from a merged PR for tasks with a PR. An agent can mark done only a task with no PR (research, a doc, a config change), and must say what it did. Code is done only when its PR merges.
- **Agents propose subtasks** (`propose_tasks`), and I accept, edit or reject them in "needs me". The exception is a plan I settle in plan-review: its phases and steps are created straight away.
- **Each Do it thread runs in its own worktree,** created by T3 with `--worktree`.
- **Work across repos means one thread per repo.** Plan it splits work so each subtask sits in one repo. Do it on a parent opens one thread per repo, all on the same branch name (e.g. `task-142-booking-flow`). Each brief names its sibling thread and the API the two sides agree on.
- **Slack:** Airboss stores only the link. The agent reads the thread through Composio's Slack connection, so Airboss needs no Slack token.
- **Local only:** the server listens on 127.0.0.1, MCP calls need a local token, and there is no login.

## Key assumptions to test

- [ ] **I will actually capture tasks.** In week 1, compare tasks added with threads opened. If fewer than half of new threads have a task, quick-add is too slow.
- [ ] **Agents report through MCP when the repo's agent note tells them to.** Run 5 threads and count the updates.
- [ ] **"Waiting on me" can be read from outside T3.** `t3 thread show` hasn't shown it yet. T3's database has pending requests in `orchestration_v2_projection_runtime_requests`.
- [ ] **A weekend is enough for capture, the tree and Start.** If day 1 doesn't end with a usable quick-add box and tree, cut the polish, not the features.

## MVP scope

- **Day 1:**
  - Tauri menu-bar app with a global hotkey (not ⌥Space, which Raycast uses)
  - SQLite task tree
  - MCP tools: `list_tasks`, `get_task`, `update_task`, `link`, `add_tasks`, `propose_tasks`
- **Day 2:**
  - Do it and Plan it, through `t3 thread start`
  - A last step in plan-review that calls `add_tasks`
  - PR sync through `gh`
  - The "needs me" home screen
- **Weeks 2–3:**
  - Threads not linked to any task, listed as "untracked"
  - Quiet-for-5-days detection
  - Claude CLI sessions

## Not doing (and why)

- **`migrate` for old repo backlogs:** new work going through the list matters more.
- **Linear/Jira sync and a status update for my manager:** neither is one of the success measures.
- **A Slack app, or a Slack token in Airboss:** Composio already reads Slack for the agent.
- **Starting the next wave of tasks automatically:** v1 shows the run order (a topological sort into waves), and I start each task myself.
- **An overnight queue of unattended agents:** wait until the core has proven itself.
- **Start for other agent tools:** T3 already runs Claude and Codex.
- **Model calls inside the app:** they would need an API key and billing.
- **One thread across several repos:** T3 tracks branches and PRs per thread, and per repo.
- **Team or shared use:** it's for me.

## Later: a lifecycle through grimoire skills

[grimoire](https://github.com/amantibrewal310/grimoire) holds my agent skills (plan-review, schema-review, setup-repo). Later, Airboss and grimoire work together, but stay **two separate repos** joined only by Airboss's MCP tools:

- **Every task gets a lifecycle,** for example: shape the idea, plan-review, build, review, ship. A small bug fix skips straight to build. A new product goes through every stage.
- **Each skill reports its own stage.** It sends its evidence to Airboss over MCP, for example "plan reviewed" with the plan file. A stage counts as done only with evidence.
- **Start launches the next stage with the right skill.** Example: on a task that already has a settled plan, Start opens a thread that builds Phase 1.
- **setup-repo adds the agent note** that tells agents to report to Airboss.
- **The skills still work without Airboss.** If Airboss isn't running, they skip the reporting step. That keeps them usable by any agent through the skills CLI.

## Prior art to read

- [sortie-ai/sortie](https://github.com/sortie-ai/sortie): "Turn tracker tickets into autonomous agent sessions."

## Open questions

- **Final name.** Airboss is the working name.
- **Stack details inside Tauri:** whether the MCP server runs in Rust, or in a bundled Node or Bun process.
- **Whether `t3 thread list` can return more than 10 threads per project.**
