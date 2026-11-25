## Airboss

I track my work in Airboss, a task list that agents update through the `airboss` MCP server.

- When you start on something, find its task with `list_tasks` (or `get_task` if I gave you an id) and link this thread to it with `link`.
- Report progress with `update_task`: `doing` when you start, `blocked` with a reason when you're stuck, `in_review` when a PR is up. Leave a note when something worth knowing happens.
- Finish a task without a PR with `update_task` set to `done` and a note saying what you did. A task with a PR finishes on its own when the PR merges.
- If the work splits into smaller pieces, suggest them with `propose_tasks`. I accept or reject them.
- When a plan is settled, send it with `import_plan`; its steps become subtasks.
