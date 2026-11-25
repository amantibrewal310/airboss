# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

The UI is Svelte 5 in a Tauri 2 web view, shipped as a macOS app. It must look and behave like a native Mac app (see Brand Commitments), not like a website in a window.

## Users

One engineer running many T3 Code and Claude Code agent threads in parallel, across several repos. They visit the app in short bursts, about 20 times a day: open it from the menu bar, the Dock or a hotkey, answer what needs them, start the next task, close it. The window is a normal laptop size (about 1100 × 720). Later the project is open source, but v1 has no onboarding for other setups.

## Product Purpose

A to-do list where agents do the work. Any task, from a one-line bug fix to a whole product, goes into one tree. One keystroke hands a task to an agent (Do it) or to a planning thread (Plan it). The app always shows where every task stands, so nobody opens a thread to ask "where are we?".

Success after four weeks: no "where are we" threads; most new threads start from a task; nothing stalls unseen (work quiet for 5+ days surfaces on its own).

## Positioning

To-do apps and trackers don't know agent threads exist. T3 knows threads but not tasks or plans. Airboss joins the task, its threads, its PRs and its plan, and gets status from agents (MCP), GitHub (`gh`) and T3, not from the user typing it.

## Operating Context

- Opened from a menu-bar icon (with a count of what's waiting), the Dock, or a global hotkey for quick-add.
- Main window: a navigation sidebar ("Needs me" first, then Ready, Plans, In progress, All tasks, parents and repos), the task tree as a list in the middle, and the selected task in a panel on the right (or as a full page).
- Glance-and-close use: the design favours fast scanning over showing everything at once.
- Works next to T3 Code, GitHub PRs, Slack links and plan docs. The app links out to them; it doesn't replace them.

## Capabilities and Constraints

- Task tree of any depth. Statuses: proposed, todo, doing, in_review, blocked, done, dropped. "Waiting" (on other tasks) is worked out, not stored. A parent's subtasks have a run order in waves.
- Each task has notes, links (T3 threads, Claude sessions, PRs, Slack, docs, GitHub issues), dependencies and a full history with actors (me, an agent, sync, plan, launch).
- Agents only propose subtasks; the user accepts, edits or rejects them. Code is done only when its PR merges.
- Needs me: agent questions and approvals, PRs to review, failed runs, proposed subtasks, tasks that just became ready, parents whose subtasks are all done, work quiet 5+ days.
- T3 Code and `gh` are optional: without them, Do it / Plan it and PR sync switch off and everything else still works.
- Stack: Tauri 2.12, Rust backend, Svelte 5 with plain CSS. No heavy component library.
- Performance: download under 15 MB, idle memory under 80 MB, cold start under 1 s, hotkey to quick-add under 100 ms, tree of 2,000 tasks scrolls at 60 fps (virtualised list).
- macOS only. Local only, no login.

## Brand Commitments

- Working name: Airboss (may change).
- Must feel like a real Mac app, not a website in a window: traffic lights, native menus and menu-bar item, standard Mac shortcuts.
- Look: like Linear and Notion. Dark first; light mode is supported and secondary. Dense, calm, hairline borders, one accent.
- Keyboard first: every main action has a shortcut. Mac arrows and keys (↑↓ move, →← open and fold, Return rename, Space done, ⌘K jump, ⌘↩ Do it), plus a few Linear-style single letters when no field has focus (C new task, S status, G then N / L / R / A to switch views). No vim-style movement keys.
- Open source: no company names anywhere in the product or its docs.

## Evidence on Hand

- `docs/idea.md`: the problem and its evidence (35 of 853 threads were status check-ins; a stack of 10 PRs sat idle for 19 days).
- `SPEC-core.md`: the data model the UI shows.
- No screenshots, users, testimonials or logos exist yet. Don't invent any.

## Product Principles

1. **Status is read, not typed.** The UI shows where status came from (agent, PR, sync) and asks the user only for decisions.
2. **What needs me comes first.** Every visit starts with the short list of decisions waiting on the user.
3. **One keystroke to hand off.** Capturing a task and starting an agent on it are each one shortcut away.
4. **Quiet when nothing's happening.** No badges, motion or colour without a state behind them.
5. **Honest about the plan.** Run order and waiting are shown as they are; nothing starts by itself.

## Accessibility & Inclusion

Follow macOS conventions: full keyboard operation, visible focus, respect Reduce Motion, Increase Contrast and the system accent colour where it fits. Status must never rely on colour alone.
