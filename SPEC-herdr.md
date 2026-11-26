# SPEC: Herdr

Status: draft 1, 26 Nov 2026. For alpha 2.

## Objective

Airboss starts and follows agents in T3 Code. Some people run their agents in **Herdr** instead: a desktop app whose window shows spaces, tabs and panes, and whose core (`hc`) runs the terminals and agents in them. For them, Do it and Plan it should open Herdr tabs, the task's live chip should follow those agents, and Open thread should show the pane, just as they do for T3 threads.

Out of scope: Herdr on another machine (`--machine`), cleaning up worktrees, reading an agent's question text from its screen, and Herdr's own TUI (`herdr`), which is a different product.

## Decisions

1. **One launcher per Do it, chosen in Settings.** "Start agents in": **Automatic** (default), **T3 Code** or **Herdr**. Automatic is T3 Code when it's Ready, else Herdr when it's Ready, else T3 Code (whose error then says what's missing). "Agent in Herdr": **Claude Code** (default) or **Codex**. Both are app settings, passed to `launch` in each `StartRequest`.
2. **A Herdr thread is a terminal.** Its key is `ThreadKey { source: Herdr, ref: <address> }`, such as `t4p`, and the link is a new kind, `herdr_terminal`. Addresses count up and aren't reused in practice. To be safe, every terminal Airboss starts carries the labels `airboss=1`, `airboss.task=<task id>` and `airboss.launch=<launch row id>`, and settling trusts a row's address only while its label matches.
3. **Agent identity: T3 Code, then Herdr, then Claude Code.** Inside Herdr every process has `HC_TERMINAL_ID`, so an agent there acts as `agent:herdr/<address>` and "link this thread" links its terminal. A Claude Code agent in a Herdr pane also has `CLAUDE_CODE_SESSION_ID`: it's kept as the identity's `claude_session`, as it already is under T3. So the link the agent makes and the one Do it made are the same row (`UNIQUE (task_id, kind, ref)`), and nothing shows twice.
4. **Do it makes its own worktree.** T3 makes worktrees itself; Herdr doesn't have a command for it. So `launch` runs `git worktree add` at Herdr's own place for them: `worktrees.directory` in `~/.config/herdr-app/settings.json` (default `~/.herdr/worktrees`), then `<repo folder name>/<branch with / as ->`. An existing worktree there, on that branch, is reused.
5. **One Herdr space per repo, one tab per thread.** The space for a repo is the one named after the repo (its Airboss name), else the one whose folder is the repo's path. If there's none, `space.new` makes it. Tabs open without taking focus (`focus: false`), as T3 threads do. Open thread shows them.
6. **Status comes from Herdr's own reports**, never from screen text: `hc events --json` gives a snapshot and then each change, and that's cheap (one line per fact change, no screen output). It maps as follows:

   | Herdr | Airboss `ThreadState` |
   |---|---|
   | `state: live`, status `working` | `Running` |
   | `live`, `blocked` (a person is needed) | `NeedsMe` |
   | `live`, `error` | `Errored` |
   | `live`, `done`, `idle` or no status | `Idle` |
   | `finished`, or not listed | `Gone` |

## Interfaces

### `core`

- `LinkKind::HerdrTerminal`, stored as `herdr_terminal`. Migration 005 rebuilds `links` with the wider `CHECK`, as earlier migrations rebuilt tables. It's a thread kind, like `t3_thread` and `claude_session`, for permissions and for "this thread". Its `ref` is 1–64 printable ASCII characters with no spaces (`t4p`, `vps/t7`), and it has no URL.

### `services`

```rust
pub enum Source { T3, Claude, Herdr }          // sync.rs; ThreadKey's source

pub enum Launcher { T3, Herdr { agent: HerdrAgent } }   // launch.rs
pub enum HerdrAgent { Claude, Codex }
pub struct StartRequest { pub task, pub mode, pub place, pub again,
                          #[serde(default)] pub launcher: Launcher }   // default T3

pub trait LaunchService: Service {
    fn availability(&self) -> Availability;          // T3 Code, unchanged
    fn herdr_availability(&self) -> Availability;    // Herdr: hc and herdr-app present
    …
}
```

`LinkKind::HerdrTerminal` ↔ `Source::Herdr` wherever the kinds and sources map today.

### `mcp`

- **Done in the groundwork commit.** `identity.rs`: `HERDR_TERMINAL = "HC_TERMINAL_ID"`, tried after `T3CODE_THREAD_ID` and before `CLAUDE_CODE_SESSION_ID`. `Identity.herdr_terminal: Option<String>`. `AIRBOSS_THREAD=herdr/<address>` works too. "This thread" links `herdr_terminal` when that's the identity, and the server's instructions name the agent's own kind (`Identity::own_kind`), so a Claude agent in Herdr links its terminal, not its session.
- The Codex setup the app copies adds `HC_TERMINAL_ID` to `env_vars`, after `T3CODE_THREAD_ID`.

### `launch`: the Herdr side

A `HerdrCli` trait next to `T3Cli`, with a fake for tests and a real one that runs `~/.local/bin/herdr-app` and `~/.local/bin/hc` (then `PATH`), with no shell. Arguments go as one JSON object, so the brief is never parsed by a shell:

```
herdr-app ls                                   # spaces (id, name, folder, tabs), recent folders
herdr-app do space.new '{"name","folder","tab","argv","task","focus":false}'   # → {"pane","terminal"}
herdr-app do tab.new   '{"space","name","folder","argv","task","focus":false}' # → {"pane","terminal"}
hc label <address> airboss=1 airboss.task=<id> airboss.launch=<row>
hc get <address> --json                        # labels, to settle a pending row
hc --version, herdr-app --version              # availability
```

- **argv:** Claude Code is `["claude"]`. Codex is `["codex", "--no-daemon"]`, because without it Codex's commands run outside hc.
- **Not open:** if `herdr-app do` can't reach the window, open Herdr (`open -b dev.herdr.app`), wait up to 10 s for `herdr-app status` to answer, and try once more. Then fail with `HerdrNotRunning`, "Herdr didn't open. Open it and try again."
- **Migration 3:** `launch_threads.launcher TEXT NOT NULL DEFAULT 't3' CHECK (launcher IN ('t3','herdr'))` and `launch_threads.worktree_path TEXT`.
- **Start**, per place, keeping the T3 path's rules (pending row first, `skipped`, `AlreadyStarting`, again-branches, blockers, `moved_to_doing`):
  - **Do it:** make or reuse the worktree, then open a tab in it, titled as for T3 (`<id> <task title>`, plus ` · <repo>` with several repos).
  - **Plan it:** one tab in the first repo's folder (no worktree), titled `Plan <id> <task title>`.
  - **No repo, research only:** one tab in that folder.
  - **Then:** label the terminal, mark the row `started` with the address, and link `herdr_terminal`.
- **The brief** is the same text T3 gets, sent as the tab's `task`, which is the agent's first message.
- **Settling:** a `herdr` row left `pending` (Airboss quit mid-start) becomes `started` if a live terminal carries its `airboss.launch` label, else `lost` after 2 minutes.
- **Errors:**

  | Error | Message |
  |---|---|
  | `HerdrMissing` | "Herdr isn't installed." |
  | `HerdrNotRunning` | "Herdr didn't open. Open it and try again." |
  | `WorktreeFailed` | git's own message, in one sentence |
  | `HerdrFailed` | herdr-app's `{error}` |

  `HerdrMissing` and `HerdrNotRunning` stop the loop, as `T3Missing` does. `WorktreeFailed` and `HerdrFailed` fail only their place.

### `sync`: the Herdr side

- A `HerdrSource` trait, with a fake and a real one. The real one follows `hc events --json` as a child process and treats each line's `terminals` as upserts by address. A line can be a snapshot or a change; handle both. A terminal's `status` is `null` or `{app, state}`. If the child exits, it's restarted with backoff (1 s, doubling to 60 s), and the source reads again on wake.
- The T3-and-Claude worker reports a `ThreadView` for every `herdr_terminal` link: the state as in Decision 6, its `title`, `project_path` (cwd) and `last_activity_at` (`status_ms`). A link whose terminal isn't listed is `Gone`. Untracked Herdr terminals aren't reported.
- **Migration 2:** `observed_threads.source` and the health table's `source` allow `'herdr'`. SQLite can't widen a CHECK in place, so rebuild them as core's migration 5 rebuilt `links`.
- **Health:** a "Herdr" connection. It's off with no row when `hc` isn't installed. It's ok as "Connected" while the stream runs, and broken as "Herdr's status can't be read" after three failed restarts.

### `app`

- **Settings:** `start_agents_in: auto | t3 | herdr` and `herdr_agent: claude | codex`, in the Agent setup section. The app picks the launcher (Decision 1) and passes it in `StartRequest`. Do it and Plan it are enabled by the chosen launcher's availability. The disabled reason names it ("Herdr isn't installed").
- **Connections:** a Herdr row: "Ready" with its version, or "Not installed". It's shown when Herdr is installed or chosen. The T3 Code row is hidden when T3 isn't installed and Herdr is Ready.
- **Open thread** on a `herdr_terminal` link runs `herdr-app do agent.focus '{"terminal":"<ref>"}'`, then brings Herdr forward (`open -b dev.herdr.app`). That's an app constant, not a stored link, so it doesn't go through the link allowlist. Its failure is a toast: "Couldn't show that terminal in Herdr."
- **First run:** without T3 Code, the Repos step lists Herdr's space folders and recent folders, under "From Herdr", with the same rules as T3 projects (git repos only, "Added" when added).
- **Copy:** wherever text says T3 Code for launching, it names the launcher actually in use. "Add to T3 and retry" stays T3-only.
- **DTOs and the mock:** a `herdr_terminal` link and a `herdr` thread source in the fixtures, so the UI tests cover them.

## Testing

- `core`: the migration on a database with every old kind; insert and `UNIQUE` for `herdr_terminal`; the ref rule.
- `mcp`: identity order (T3 > Herdr > Claude), `AIRBOSS_THREAD=herdr/…`, and "this thread" linking `herdr_terminal`.
- `launch` (fake `HerdrCli` and `Git`): Do it with one and with two repos (worktree path, reuse, again-branch); Plan it; research only; Herdr not open, then open; `herdr-app` errors; labels; pending rows settling; migration 3. There's an `#[ignore]`d test against the real Herdr, in a space named `airboss-test`, which closes that space afterwards.
- `sync` (fake source): each mapping row; snapshot then change lines; a terminal that disappears; restart and backoff; health. There's an `#[ignore]`d test against the real `hc`, with a terminal it starts and removes itself.
- `app`: picking the launcher; settings round-trip; Open thread's commands (recording shell); connection rows; first run with Herdr folders (component test); the axe and screenshot suites still pass.

## Boundaries

- **Never** touch terminals, tabs or spaces Airboss didn't start. Tests and QA use their own space, `airboss-test`, open it with `focus: false`, and close only it. Never run `hc server stop` or `restart`, `hc kill` on someone else's terminal, or `hc send` to an agent Airboss didn't start.
- Don't write to `~/.config/herdr-app` or `~/.config/herdr-core`. Settings are read only.
- **Ask first:** a new dependency, or a second database table.

## Success criteria

On this Mac, with Herdr open:

1. Choose Herdr and Claude Code. Do it on a task linked to a scratch repo opens a tab named `<id> <title>` in that repo's Herdr space, in `~/.herdr/worktrees/<repo>/task-<id>-…`. Claude gets the brief as its first message, and the task shows "Working" within a few seconds.
2. When the agent asks something, the chip turns to "Waiting on you" and Needs me has the item. When it finishes, the chip is "Stopped", and Agent stopped follows after 10 minutes with no PR.
3. Open thread brings Herdr forward on that pane.
4. The agent's own `link` (this thread) makes no second link.
5. With Herdr quit, Do it opens it and still starts the tab.
6. T3 Code users see no change: Automatic picks T3 when it's Ready, and every existing test passes.

## Checked on this Mac (26 Nov 2026)

- `hc` 0.1.22 and `herdr-app` 0.1.18 are in `~/.local/bin`, and Herdr.app's bundle id is `dev.herdr.app`.
- `hc ls --json` takes about 10 ms and gives `{terminals:[{address, agent, cwd, state, status, turn, where}]}`, where `status` is a string.
- `hc get <t> --json` and `hc events --json` add `labels`, `status` as `{app, state}` (Claude Code reports `app: "claude-code"`), `status_ms`, `title`, `created_ms` and `finished_ms`. Idle terminals produced one snapshot line in 8 s.
- `herdr-app ls` gives `{spaces:[{id, name, folder, tabs:[{id, name, terminals}]}], recent:[{name, folder}]}`.
- `herdr-app commands` lists:
  - `space.new {name, folder, tab, argv, task, machine, focus}`
  - `tab.new {name, space, folder, argv, task, mod, props, focus}`
  - `agent.focus {terminal}`
- `codex --help` lists `--no-daemon`.
