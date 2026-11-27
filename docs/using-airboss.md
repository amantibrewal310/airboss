# Using Airboss

Airboss is built for short visits: open it, answer what needs you, start the next task, close it. This page walks through that flow, then lists every shortcut.

## The window

The sidebar lists the views, then your parent tasks, then your repos:

| View | What it shows |
|---|---|
| **Needs me** | Decisions waiting on you. The app opens here. |
| **Ready to start** | Todo tasks with nothing open to wait on, with when each became ready |
| **Plans** | One card per open top-level parent: progress, the current wave and what's next |
| **In progress** | Tasks that are in progress or in review |
| **All tasks** | The whole tree, as a tree or grouped by status |
| a parent | That parent's subtasks, as a tree |
| a repo | Every task linked to that repo, or under a parent linked to it |

Select a task to see its panel on the right: what needs you, its status, repos, branch, parent, what it waits on and blocks, links, notes, subtasks, threads and activity. ⌘↓ opens it as a full page. ⌘W or the red button hides the window; ⌘Q quits.

The **+** beside Repos opens **Add repos**: tick folders from T3 Code's projects (or Herdr's), or **Add another folder…**. With more than seven, a search field at the top filters them by name or path; ↓ goes into the list, Space ticks, ⌘↩ adds, and Esc clears the search.

The footer at the bottom of the sidebar says "All connected", or names the first connection that's off. Click it for the **Connections** list, which says what each tool is for and what to fix, with a **Check again** button.

## Capture

Press **⌃⌥N** in any app. The quick-add box opens over whatever you're doing, with the cursor in the title.

- Type a title. **↓** moves to the notes, and **↑** on the first line of the notes moves back.
- **Tab** opens the repo list and **Shift-Tab** the parent list.
- **↩** creates the task. **⌘↩** creates it and starts Do it.
- **Esc** hides the box and clears it. Clicking elsewhere hides it and keeps what you typed.

Paste a link into the title or notes and it becomes a link pill:

| You paste | It becomes |
|---|---|
| `github.com/<owner>/<repo>/pull/<n>` | a PR link |
| `github.com/<owner>/<repo>/issues/<n>` | a GitHub issue link |
| a `*.slack.com/archives/…` link | a Slack link |
| `t3code://threads/<environment>/<thread>` | a T3 thread link |
| an absolute or `~` path | a doc link |
| any other web address | a doc link with that address |

Inside the app, **C**, **⌘N** or **⌥N** opens the same form. In a parent's or a repo's view, it starts with that parent or repo filled in.

## Needs me

Needs me is the list of decisions waiting on you, in a fixed order. The menu-bar icon shows how many there are, and its menu lists the first six.

| Group | When it shows | Main action (↩) | Others |
|---|---|---|---|
| Questions | An agent on one of your tasks asked a question or wants an approval | Open thread | Dismiss |
| Reviews | A linked PR is open and isn't a draft, and no agent on its task is still running or waiting on you | Open PR | Dismiss |
| Failed runs | A linked T3 thread's last run failed | Open thread | Dismiss |
| Stopped | A task in progress whose agents have all stopped, with no open PR and nothing new for 10 minutes | Open thread | Mark done (Space), Set in review, Snooze 3 days, Dismiss |
| Due | A task's due date has come (today or past), until it's done | Open | Mark done (Space), Snooze 3 days, Dismiss |
| Proposed | An agent proposed subtasks | Accept all | Accept, Edit or Reject each; Reject all |
| Ready | A task's last blocker finished, in the last 7 days | Do it | Dismiss |
| Quiet | A task in progress, in review or blocked has had no activity for 5 days | Open | Snooze 3 days |

↑↓ move through the list, ↩ or ⌘↩ runs the main action, and ⌘⌫ dismisses. A dismissed item stays hidden, even after a restart, until something new happens: a new question, a new quiet stretch. When an item's source is offline, it's dimmed and says "May be out of date".

**Due dates.** Tell an agent "by Friday" or "in 2 days" and it sets the date on the task; or set it yourself on the **Due** line of the task's panel, or with `airboss due`. The day it comes, the task shows under Due. There's no notification for it yet.

When nothing needs you, the screen says so, with how many tasks are in progress and ready, and a button to show the ready ones.

## Do it and Plan it

Every task has two buttons in its panel.

**Do it** (⌘↩) hands the task to an agent. For each repo linked to the task, it starts an agent in a new git worktree: a T3 Code thread, or a tab in Herdr (see [Where agents start](#where-agents-start)). All the threads share one branch: the task's own, or `task-<id>-<slug>`, such as `task-142-booking-flow`. Each thread's first message is a brief: the task's title, notes, parent, subtasks, dependencies and links, plus how to report back. With several repos, each brief says how to find the sibling threads through Airboss. The task moves to in progress.

- While it starts, the button reads "Starting…". A toast says what started, with **Open thread**.
- A task with no repo opens a menu: **Start in &lt;repo&gt;**, which links that repo and starts; **Add another repo…**; or **No repo: research only**, which starts one thread with no worktree. If the task's title or notes name a T3 project that isn't a repo here yet, such as `hub-ui: fix 7 tests`, the menu leads with **Add hub-ui and start**. Agents can't add repos, so they name one that isn't registered this way.
- A task that still waits on others isn't started. The toast names what it waits on.
- On a done or dropped task, the toast offers **Reopen and start**.
- If one repo fails and another starts, the toast offers **Retry**, which starts only the failed one. A repo that isn't a T3 project yet gets **Add to T3 and retry**.
- A repo that already has a thread from this task is skipped. **Start again** starts a fresh one, on a new branch such as `task-142-booking-flow-2`.

**Start ready** takes Do it's place on a task with subtasks, such as a settled plan or one of its phases. The button counts the steps that can start now: open subtasks with no subtasks of their own, nothing open to wait on, and no waiting phase above them. It runs Do it on each of those steps, so every step gets its own thread, worktree and branch, and moves to in progress by itself. One toast says what started. A step with no repo isn't started; the toast names it and offers to start it, and Do it then asks where. Once those steps finish, the next ones become ready, and Start ready shows the new count. To hand the whole task to one agent anyway, use **Do it as one thread** from the right-click menu or ⌘K. That agent is told to report on each step, not on the parent, and links each PR to the step it does. The parent takes no branch of its own and doesn't move to in progress: its status follows its steps.

**Plan it** (⇧⌘↩) starts one thread that plans the task with you, with no worktree, because planning changes no code. When you tell it the plan is settled, it calls `import_plan`, and the plan's phases and steps appear as subtasks, with their dependencies, and the plan file linked. Sending the plan again adds only new steps.

Both need T3 Code or Herdr. Without either, they read as disabled, and pressing them explains why.

### Where agents start

**Settings › Agent setup › Start agents in** picks **T3 Code** or **Herdr**. **Automatic**, the default, uses T3 Code when it's there, else Herdr. **Agent in Herdr** picks Claude Code or Codex.

In Herdr, each repo gets a space named after it, and each thread is a tab, named like a T3 thread (`142 Booking flow`). Do it makes the tab's git worktree where Herdr keeps its own: `~/.herdr/worktrees/<repo>/<branch>`, or the folder `worktrees.directory` names in Herdr's settings. The agent gets the same brief as its first message. Tabs open without taking focus. **Open thread** brings Herdr forward on that tab. If Herdr isn't open, Do it opens it first.

Claude Code asks whether you trust a folder the first time it runs there, and each Do it worktree is a new folder. Needs me shows the question as **Allow: trust this folder**, with Open thread to answer it in Herdr. Airboss never answers it for you.

## Where status comes from

You rarely set a status by hand. It comes from four places, and the activity list shows which one each change came from.

A task with subtasks has no status of its own: it shows what its subtasks add up to. It's done when every subtask is done or dropped (with at least one done), dropped when all are dropped, blocked when any is (its reason names which), in progress when any is in progress or in review or some are finished, and otherwise todo. So a plan with 8 of 38 steps done shows in progress, and nothing has to finish it. Proposals don't count until you accept them. A parent takes no branch or PR of its own; its PRs go on its subtasks.

- **Agents**, through the MCP tools: in progress when they start, in review when their PR is up, blocked with a reason, and notes along the way. An agent can mark a task done only if it has no PR, and must say what it did.
- **GitHub**, through `gh`: every linked open PR is read about every 5 minutes, and again when you open its task. When all of a task's PRs have merged or closed, with at least one merged, the task is done. If an agent on it is still running or waiting on you, Airboss waits until it stops. A PR reopened on GitHub moves its done task back to in review. PRs that T3 Code lists on a linked thread are linked to the task too.
- **T3 Code**, read from its database: each linked thread shows as Working, Waiting on you (it asked a question or wants approval), Your turn (it finished and waits for your next message) or Stopped (it failed or was settled), with its question or error. A failed run adds a note to the task. T3 state never changes a task's status by itself.
- **Herdr**, from `hc events`: each linked terminal shows as Working, Waiting on you (Herdr says it's blocked on you), Your turn (it finished) or Stopped. When the agent asks for something, Needs me shows what Herdr reports, such as "Allow: trust this folder".
- **Claude Code** sessions run outside T3 Code and Herdr: linked sessions show as working or stopped.

The rest is yours: accepting proposals, dropping tasks, and finishing a task whose PR hasn't merged. Space marks the selected task done, or not done. S opens the status menu: 1 Todo, 2 In progress, 3 In review, 4 Blocked (asks why), 5 Done, 6 Dropped (asks first, because it drops every open subtask too). On a parent only Dropped is offered: to close a phase that still has open steps, drop those steps, or drop the phase, which drops them for you.

## Run order and waves

A task can wait on others (⌘D). A waiting task shows a "waiting" mark and isn't ready until everything it waits on is done or dropped.

A parent's subtasks are sorted into waves. Wave 1 is every subtask that waits on nothing under the parent, wave 2 is what waits only on wave 1, and so on. A dependency between steps deep down counts for the subtasks they sit under. Done subtasks keep their place in the picture. A subtask that waits on a task outside the parent is shown with its outside blockers, and subtasks that wait on each other in a loop are listed apart.

The full page of a parent shows the waves as a list or as a map. On the map, hover a station to trace its lines, click to select it, and ⌘↩ to start it. Arrow keys move between and within waves. Above 40 subtasks or 8 waves the page opens on the list.

Nothing starts by itself. Select a ready task and press ⌘↩.

## Proposals

Agents don't create tasks. They propose them, and proposals wait for you, under their parent, in Needs me. A proposal lands next to the task the agent was working on, follows it, and may say what it would block ("follows #12 · would block #20"). Until you accept it, it holds nothing up.

- **Accept** makes a proposal a todo task. **Accept all** does that for the group.
- **Edit** lets you rename it first, then accepts it.
- **Reject** drops it and hides it from the tree. It stays in history, with your reason if you gave one.

## Undo

⌘Z undoes your last change, and so does **Undo** on its toast. Airboss keeps your last 20 changes until you quit.

- **Undoable:** dismiss, snooze, status changes, drops (with the subtasks they took), moves and reorders, and rejects.
- **Rejects** are written 5 seconds later, so they can be taken back. Quitting writes them at once.
- **Not undoable:** accepting a proposal, creating, renaming, notes, links, dependencies, repos, Do it and Plan it. Text fields keep their own ⌘Z.
- If someone else changed the task since, such as an agent, the undo is refused with "144 changed since; nothing was undone."

## The menu bar

The menu-bar icon shows the Needs me count, or nothing at 0. Its menu lists up to six items, with clock times; choosing one opens the window on that item. Below them are **Open Airboss** (⌘0), **Quick Add…** and **Quit Airboss**.

## Updates

When a newer Airboss is out, a card at the foot of the sidebar says "Airboss 0.1.0-alpha.5 is available".

- **Update and restart** downloads it, with a progress bar, checks its signature, puts it in place of the app you're running, and restarts. The window comes back where it was, with your tasks and settings.
- **What's new** opens the release's page on GitHub. **Later** hides the card until Airboss starts again.
- If an update fails, the card says why and offers **Try again**. The app you're running is untouched.

Airboss looks for a new version 30 seconds after it starts and then every 6 hours, unless you turn off **Check for updates automatically** in Settings. **Airboss › Check for Updates…** (or "Check for updates" in ⌘K) looks right away, whatever the setting. Each check reads one small file from GitHub; nothing about you is sent.

Each update is signed with Airboss's own key, and the app refuses a download whose signature doesn't match.

## Settings

⌘, opens Settings:

- **Quick-add hotkey:** press the button and type a new one. It needs ⌃, ⌥ or ⌘ and one other key. Keys macOS uses (⌃Space, ⌃⌥Space, ⌘Space, ⌥⌘Space, ⌃⌘Space) are refused, and so is a key another app holds.
- **Open Airboss at login**
- **Appearance:** System (the default), Dark or Light
- **Check for updates automatically:** on by default, with **Check now** ([Updates](#updates))
- **Command-line tool:** Install, with its path and whether its folder is on your `PATH`
- **Agent setup:** Copy agent note, and Copy setup command

Settings live in `app-settings.json`, next to the database.

## Keyboard reference

This list is taken from the app's command registry, `app/src/lib/logic/commands.ts`, which also builds the menus and the shortcut sheet. Press **?** in the app to see the sheet.

Single letters, Space and arrows act only when no text field has focus. In a text field, only ⌘K, ⌘N, ⌘,, ⌘W and F6 reach the app, and Esc cancels the edit. Every other key belongs to the field: ⌘↩ saves it and ⌘Z undoes typing.

### Anywhere

| Keys | Action |
|---|---|
| ⌃⌥N | Quick add, from any app (change it in Settings) |
| ⌘K | Jump to a task, or run a command |
| ⌘N, C, ⌥N | New task |
| ⌘, | Settings |
| ? | Keyboard shortcuts |
| ⌘Z | Undo |
| F6 | Go to notifications (toasts), and back |
| ⌘0 | Open Airboss on Needs me |
| Esc | Close the top overlay, the panel or the page |
| ⌘W | Close the window |
| ⌘Q | Quit |

### Views

| Keys | Action |
|---|---|
| G then N | Needs me |
| G then R | Ready to start |
| G then L | Plans |
| G then P | In progress |
| G then A | All tasks |

**As Tree** and **By Status** (All tasks only) are in the View menu and in ⌘K.

### Lists

| Keys | Action |
|---|---|
| ↑ ↓ | Move up and down |
| Home or ⌥↑ | First row |
| End or ⌥↓ | Last row |
| Page Up, Page Down | Page up and down |
| → | Open, or go to the first subtask |
| ← | Fold, or go to the parent |
| ↩ | Rename |
| Tab | Switch between the list and the panel |

### The selected task

These work in list views. The ones marked "also on the page" work on a task's full page too.

| Keys | Action | |
|---|---|---|
| Space | Mark done, or not done | |
| S | Status: then 1–6, or ↑↓ and ↩ | |
| R | Link repos | |
| ⌘↩ | Do it | also on the page |
| ⇧⌘↩ | Plan it | also on the page |
| ⌘↓ | Open the full page | |
| ⌘↑ | Back to the list | on the page |
| ⌥⌘↑, ⌥⌘↓ | Move up or down among its siblings (tree layouts) | |
| ⌘D | Add waits-on | also on the page |
| ⌘L | Add link | also on the page |
| ⌘B | Mark blocked | also on the page |
| ⌘O | Open the latest thread or PR | also on the page |
| ⇧⌘C | Copy the task number | also on the page |
| ⌘⌫ | Drop (asks first) | also on the page |

**Move under…** has no key. It's in the Task menu and in ⌘K.

### Needs me

| Keys | Action |
|---|---|
| ↑ ↓ | Previous and next item |
| ↩ or ⌘↩ | The item's main action |
| ⌘⌫ | Dismiss |

### Plans

| Keys | Action |
|---|---|
| ← → | Previous and next plan |
| ↑ ↓ | The plan above and below |
| ↩ | Open the plan's page |

### The run-order map

| Keys | Action |
|---|---|
| ← → | Move between waves |
| ↑ ↓ | Move within a wave |
| ↩ | Open the station's task |
| ⌘↩ | Start the selected station's task |
| Esc | Clear the selection |

### Quick add and New task

| Keys | Action |
|---|---|
| ↓ in the title | Go to the notes |
| ↑ on the notes' first line | Back to the title |
| Tab | Repos |
| Shift-Tab | Parent |
| ↩ | Create the task |
| ⌘↩ | Create it and start Do it |
| Esc | Close it and clear it |
