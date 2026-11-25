---
version: 1
slug: "docs-design-prototype-html"
primary_target: "docs/design/prototype.html"
related_targets: []
---

## Scope

The Airboss Mac app: main window (navigation sidebar, task list, detail panel and full task page), Needs me inbox, Plans overview, run order, new task (hotkey), Jump (⌘K), status and right-click menus, menu-bar dropdown, shortcuts, and empty and first-run states. Visitor mode: Operate. Glance-and-close use, about 20 visits a day.

## Direction contract

THESIS: A dark, Linear/Notion-grade tool whose one job is to show where agent work stands and what needs me. It refuses the dashboard-in-a-window and the chatty to-do app: dense rows, hairline borders, one accent, and detail one keystroke away.

OWN-WORLD: Dark first. Near-black surfaces (#0A0B0C sidebar, #0F1012 main, #16171A raised), hairline white borders at 7–11%, Inter at 13px, indigo accent #7B7FF5 (#5E63E6 fills). Status icons are Linear-style rings: grey todo, amber half for in progress, green three-quarters for in review, orange bar for blocked, an indigo check for done, dashed for waiting. Red #F2555A means "needs me" and nothing else. Each repo has its own colour dot.

STORY: I open the app on Needs me, clear each decision with ↩ from a two-pane inbox, glance at Plans to see each parent's % done, start the next ready task with ⌘↩, and close the window.

FIRST VIEWPORT: A 232px nav sidebar (Needs me with a red count, Ready, Plans, In progress, All tasks, Parents with progress rings, Repos, and "All connected" at the foot). A 44px header. 38px task rows: status, number, title, progress ring, live agent chip, needs-me dot, pills, repo, actor, time. A 480px detail panel laid out like a Notion page: properties, notes, threads, subtasks, activity.

FORM: The user steered away from the rolled directions ("Drawing set", then "Soft native") to a pinned Linear/Notion-like dark world, keeping the line-diagram run order and repo colours from the earlier rounds. seed:fbc0f0def17ba667

SIGNATURE: Run order as a map. Repos are lanes, waves are columns, tasks are stations, and lines run from blocker to task: solid when the blocker is done, dashed while waiting. Parent progress (% of all subtasks done) shows on the sidebar, rows, Plans cards and the task page.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

## Open decisions

- Answering an agent's question or approving a request inside Airboss needs a way to write back to a T3 thread. v1 opens the thread instead.
