# Spec: launch

Module `launch` from [CAPABILITIES.md](CAPABILITIES.md). State: **draft 1, waiting for review**.

It builds on [`core`](SPEC-core.md), including the v2 additions, and is called by the app. It is the only module that runs the `t3` CLI. Facts about `t3` and `git` were checked on this Mac on 25 Nov 2026 (see [Checked on this Mac](#checked-on-this-mac)); the rest is under [Not verified yet](#not-verified-yet).

## Objective

`launch` is behind the two buttons on every task:

- **Do it** starts one T3 thread per linked repo. Each thread gets its own git worktree, and all of them use one branch name. Each thread's first message is a brief with everything the agent needs.
- **Plan it** starts one thread that plans the task with me through the `plan-review` skill, then imports the settled plan as subtasks with `import_plan`.

Each new thread is linked to its task, and the task moves to `doing`.

**User stories**

- As me, I press ⌘↩ on "Booking flow" (repos booking-web and booking-api) and get two threads on `task-142-booking-flow`. Each knows the other exists and which API they share.
- As me, I press ⇧⌘↩ on a big task, plan it in a thread, and its phases and steps appear as subtasks when I settle the plan.
- As me, on a task with no repo, I'm asked where to start: one of my repos, or "No repo: research only".
- As me, without T3 Code, the buttons are disabled and say why. If one repo of two fails, the other still starts, and I can retry the one that failed.

## Decisions

- **The `t3` CLI, not T3's local server.** T3's CLI source (in the app bundle) shows how `t3 thread start` works. It finds the running server, gets a short-lived session token from T3's own auth store, calls the server's `t3_thread_launch` tool, and then revokes the token. To call the server directly, Airboss would have to read T3's secrets, which is off limits. The CLI is T3's supported interface, resolves project paths, and gives clear error messages. Each call costs about 0.3 s of CPU, which is fine for a button press.
- **One thread per repo, started one after another**, all on one branch. A failure in one repo doesn't stop the others, unless it means T3 itself is unavailable.
- **The branch** is the task's own `branch` if it has one. Otherwise it is `task-<id>-<slug>`, saved with `set_branch(…, launch)` before the first thread starts, so every sibling gets the same name. Each Do it task gets its own branch; a subtask doesn't inherit its parent's.
- **The worktree base** is the repo's default branch, from `origin/HEAD`, unless `launch_repos.base_ref` overrides it. T3 fetches before it checks out.
- **Plan it** starts one thread without a worktree, because planning doesn't change code. It runs in the task's first repo, or in the research folder if the task has none. It also moves the task to `doing`, since planning is work on the task.
- **"No repo: research only"** starts a thread in `~/.t3/scratch`, without a worktree or branch. On this Mac that folder is T3's "No project" project.
- **Siblings find each other through Airboss.** Thread ids exist only after each start, so a brief can't name the other threads' ids. Instead, each brief says that the sibling threads are linked to the task and that `get_task` lists them. No follow-up message is sent.
- **Starting again** skips any repo that already has a Do it thread for this task, which makes retry after a partial failure safe. An explicit "Start again" (`again: true`) starts new threads on `<branch>-2` (the first free number).
- **Blocking calls.** `launch` keeps no thread, timer or watcher. The app calls it from a background thread.
- **`launch` doesn't read T3's database and doesn't use `sync`.** It records what it started in its own table.

## Tech stack

- Rust edition 2024, Rust 1.100, crate `airboss-launch`.
- Dependencies: `airboss-core` (path), `rusqlite` 0.43.0 with `bundled`, `serde` 1.0.229, `serde_json` 1.0.154 and `thiserror` 2.0.21. These are the latest stable on crates.io as of 25 Nov 2026. **No new crates.**
- Processes: `std::process::Command`, with a timeout enforced by a waiting thread that kills the child.
- **Needs from `core` v2:** `Task.repos`, `set_branch` (allowed for `launch`), link titles, `dependencies(id)`. It also needs `airboss_core::migrate` to be public; if it isn't, copy `db::migrate`, as in `sync`.

## Interfaces

```rust
pub type SharedStore = Arc<Mutex<airboss_core::Store>>;

pub struct LaunchConfig {
    pub db_path: PathBuf,        // airboss.db
    pub t3_path: PathBuf,        // ~/.t3/bin/t3
    pub research_dir: PathBuf,   // ~/.t3/scratch
    pub timeout: Duration,       // 30 s per t3 call
}
impl LaunchConfig { pub fn defaults(db_path: PathBuf, home: &Path) -> LaunchConfig; }

pub struct Launcher { /* config, store, own connection, tools, cached availability */ }
impl Launcher {
    pub fn new(cfg: LaunchConfig, store: SharedStore, tools: LaunchTools) -> Result<Launcher, LaunchError>;
    pub fn availability(&self) -> Availability;          // cached; no process
    pub fn check_again(&self) -> Availability;           // runs `t3 --version`
    pub fn prepare(&self, task: TaskId, mode: Mode) -> Result<Prepared, LaunchError>;
    pub fn start(&self, req: &StartRequest) -> Result<LaunchReport, LaunchError>;
    pub fn briefs(&self, req: &StartRequest) -> Result<Vec<Brief>, LaunchError>; // exactly what start would send
    pub fn add_to_t3(&self, repo: RepoId) -> Result<(), LaunchError>;            // `t3 project add`
    pub fn repair(&self) -> Result<usize, LaunchError>;  // links threads whose link didn't land
}

pub struct StartRequest { pub task: TaskId, pub mode: Mode, pub place: Place, pub again: bool }
pub enum Mode { DoIt, PlanIt }
pub enum Place { TaskRepos, Repo(RepoId), NoRepo }
pub struct Prepared {
    pub repos: Vec<Repo>,             // where Do it would start; may be empty
    pub from_subtasks: bool,          // repos are the union of open subtasks' repos
    pub needs_choice: bool,           // Do it with no repo anywhere: show "Start in …"
    pub choices: Vec<Repo>,           // every registered repo, for that menu
    pub branch: Option<String>,       // the branch Do it would use (not saved yet)
    pub started: Vec<StartedThread>,  // threads launch already started for this task
}
pub struct LaunchReport {
    pub branch: Option<String>, pub started: Vec<StartedThread>, pub skipped: Vec<RepoId>,
    pub failed: Vec<Failed>, pub moved_to_doing: bool, pub warnings: Vec<String>,
}
pub struct StartedThread { pub thread_id: String, pub mode: Mode, pub repo: Option<RepoId>,
                           pub title: String, pub project_id: String, pub linked: bool }
pub struct Failed { pub repo: Option<RepoId>, pub error: LaunchError }
pub struct Brief { pub repo: Option<RepoId>, pub argv: Vec<String>, pub stdin: String }
pub enum Availability { Unknown, Ready { version: String }, Missing { reason: String } }

pub trait T3Cli: Send { fn run(&mut self, argv: &[String], stdin: Option<&str>, timeout: Duration)
                               -> io::Result<CmdOutput>; }
pub trait Git: Send {
    fn default_branch(&mut self, repo: &Path) -> io::Result<Option<String>>;
    fn has_branch(&mut self, repo: &Path, branch: &str) -> io::Result<bool>;
    fn valid_branch_name(&mut self, name: &str) -> io::Result<bool>;
}
pub struct LaunchTools { pub t3: Box<dyn T3Cli>, pub git: Box<dyn Git> }
impl LaunchTools { pub fn real(cfg: &LaunchConfig) -> LaunchTools; }   // never called by tests
```

`LaunchError` variants: `T3Missing(String)`, `T3NotRunning`, `T3Unreachable(String)`, `T3TooOld(String)`, `Timeout`, `NotAProject { repo, path }`, `RepoMissing { repo, path }`, `NoBaseBranch { repo }`, `BadBranch(String)`, `TaskClosed(Status)`, `TaskProposed`, `NeedsChoice`, `BadAnswer(String)`, `T3Failed(String)`, `Core(core::Error)`, `Db(rusqlite::Error)`. Each has one fixed sentence for the app to show, in `src/errors.rs`.

## Data / SQL

Migration `launch` version 1, recorded in `schema_migrations` as `('launch', 1)`. Loaded into an in-memory SQLite on 25 Nov 2026; each CHECK rejected its bad row.

```sql
CREATE TABLE launch_threads (             -- one thread launch started
  thread_id     TEXT PRIMARY KEY CHECK (length(thread_id) > 0),  -- from `t3 thread start --json`
  task_id       INTEGER NOT NULL,         -- no foreign key: core owns tasks
  repo_id       INTEGER,                  -- null: research-only, or Plan it with no repo
  mode          TEXT NOT NULL CHECK (mode IN ('do','plan')),
  branch        TEXT,
  base_ref      TEXT,
  t3_project_id TEXT NOT NULL,
  title         TEXT NOT NULL,
  linked        INTEGER NOT NULL DEFAULT 0 CHECK (linked IN (0,1)),   -- 1 once core has the link
  started_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  CHECK ((branch IS NULL) = (base_ref IS NULL)),
  CHECK (branch IS NULL OR (mode = 'do' AND repo_id IS NOT NULL))
);
CREATE INDEX launch_threads_task_idx ON launch_threads(task_id, repo_id);

CREATE TABLE launch_repos (               -- per-repo launch settings
  repo_id       INTEGER PRIMARY KEY,
  base_ref      TEXT CHECK (base_ref IS NULL OR length(base_ref) > 0),  -- null: use origin/HEAD
  t3_project_id TEXT,                     -- the project T3 last resolved this repo to
  updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
```

Rows for deleted tasks are removed by `repair()` when `core` answers `NotFound`.

## Behaviour

### Availability

- `check_again` returns `Missing` if `t3_path` doesn't exist. Otherwise it runs `<t3> --version`: exit 0 with `t3 vX.Y.Z` is `Ready`, and exit 127 (the launcher's "T3 Code has moved or been removed") or anything else is `Missing`.
- The reason text is "Do it and Plan it need T3 Code. Install it, then choose Check again."
- The app calls `check_again` once at start, in the background, and again from "Check again". While the answer is `Missing`, the buttons are disabled.
- T3 not *running* is found only when starting. It isn't checked ahead, because checking costs a process.

### prepare

- Refuses `proposed` tasks (`TaskProposed`), and `done` or `dropped` ones (`TaskClosed`; the app can offer "Reopen and start" by setting `todo` as `me` first).
- `repos` is `Task.repos` in link order. If that's empty, it's the union of the open subtasks' repos, by name, with `from_subtasks` set.
- `needs_choice` is true only for Do it with no repos at all. `start` with `Place::TaskRepos` then fails with `NeedsChoice`.
- The menu's answer becomes the `place`. "Start in <repo>" is `Place::Repo(r)`: the app first links that repo to the task as `me` (`link_repo`), so the task shows it from then on. "No repo: research only" is `Place::NoRepo`.
- `branch` is computed as below but not saved.
- `prepare` runs no `t3` process, only reads.

### Branch name

- The task's `branch`, if set, after checking it with `git check-ref-format --branch <name>`. Otherwise `task-<id>-<slug>`.
- To make the slug: lowercase the title, keep ASCII letters and digits, turn every other run of characters into one `-`, and trim `-` from both ends. Cut it to 40 characters at the last `-` before the limit.
- Examples: 142 "Booking flow" gives `task-142-booking-flow`. 7 "Fix: café (EU) — VAT!" gives `task-7-fix-caf-eu-vat`. A title with no ASCII letters or digits gives `task-<id>`.
- `start` saves it with `set_branch(task, Some(name), launch)` before the first thread, only if the task had none.
- **Again:** the first `<branch>-N`, from N = 2, that has no `launch_threads` row for this task and isn't a branch in the repo (`has_branch`). Its base is `<branch>` if that branch exists in the repo, otherwise the default branch. The task's saved branch doesn't change.

### The commands

```
# Do it: once per repo, in prepare's order
<t3> thread start --json --title "<title>" --worktree <base> --branch <branch> <repo path> -
# Plan it: one thread, no worktree
<t3> thread start --json --title "Plan <id> <task title>" <first repo path | research_dir> -
# No repo: research only
<t3> thread start --json --title "<title>" <research_dir> -
```

- **Title:** `<id> <task title>`, plus ` · <repo name>` when there are several repos. Cut to 80 characters with `…`.
- **Base:** `launch_repos.base_ref`, else `git -C <path> symbolic-ref --short refs/remotes/origin/HEAD` with the remote prefix removed (`origin/main` gives `main`), else `git -C <path> branch --show-current`. If none, the repo fails with `NoBaseBranch`.
- **Git checks:** `has_branch` is `git -C <path> rev-parse --verify --quiet refs/heads/<branch>` (exit 0 yes, 1 no); `valid_branch_name` is `git check-ref-format --branch <name>`. `git` is `/usr/bin/git`, which a Finder-started app can always find.
- **The brief** goes on stdin (`-` as the message), and stdin is closed afterwards. It's never passed as an argument, so its length and quoting don't matter.
- **Process:** working directory `$HOME`. The environment is the app's, minus `T3CODE_THREAD_ID`, `CLAUDE_CODE_SESSION_ID` and `AIRBOSS_THREAD`, so a development build started inside an agent thread doesn't pass on that thread's identity. Timeout 30 s.
- **Answer:** one JSON line on stdout, `{"threadId": "…", "projectId": "…", "runId": "…" | null}`. `t3` returns once the thread exists. T3 then fetches, creates the worktree and starts the agent in the background, and records the final branch and worktree on the thread, where `sync` reads them.

### After each thread starts

1. Insert its `launch_threads` row with `linked = 0`, and upsert `launch_repos.t3_project_id`.
2. `link(task, NewLink { kind: T3Thread, ref: threadId, title: Some(<title>) }, launch)`, then set `linked = 1`. The history entry "Started thread … in booking-web" is this `linked` event.
3. After the loop, if at least one thread started and the task is `todo` or `blocked`, `set_status(task, Doing, None, launch)`. A `doing` or `in_review` task stays as it is.

If step 2 fails, the thread still exists: the report gets a warning, and `repair()` links it. `repair()` runs when `Launcher::new` is called and after every `start`.

### Partial failure

- The loop stops early on errors that mean T3 can't start anything: `T3Missing`, `T3NotRunning`, `T3Unreachable`, `T3TooOld` and `Timeout`. The remaining repos are reported as failed with the same error.
- Any other error fails only that repo, and the loop goes on.
- `start` returns `Err` only if nothing could begin (a closed task, or no repo for Do it without a choice). Everything else is in `LaunchReport`.
- Example toast: "Started in booking-web. booking-api failed: it isn't a T3 project yet. [Add to T3 and retry]". Retrying `start` with the same request then skips booking-web.
- **Not a T3 project:** `add_to_t3(repo)` runs `<t3> project add <path> --title <repo name>` and is only ever called from that button.

### The Do it brief

Placeholders in braces. Empty sections are left out. Long fields are cut: notes at 20,000 characters, parent notes at 1,500, lists at 50 items.

```
You're working on Airboss task {id}: {title}.
{One repo: "Work in {repo} ({path})." Several: "This task spans {n} repos: {names}. One thread works in each,
all on branch `{branch}`. You're the {repo} thread ({path})."}

## The task
{notes, or "No notes yet. If the title isn't enough, ask me here."}

## Where it sits
Under: {ancestor ids and titles, outermost first}
Parent {pid} {ptitle} notes: {parent notes}
Subtasks, in run order: {id, title, status, repos per line}
Waits on: {id title (status)} · Blocks: {id title (status)}

## Links
{one per line: kind, title or ref, url; PRs with their state; thread links left out}
Slack: {url}. Read the whole Slack thread with Composio's Slack tools (the `composio` CLI) before
you start. Airboss keeps only the link.

## How to work
- You're in a new git worktree on branch `{branch}`, based on `{base}`. Open your pull request from it.
{Several repos: "- Your sibling threads are linked to task {id}: `get_task` with id {id} lists them, titled
by repo. Agree the API between the repos before you build against it. If task {id}'s notes don't define
it, propose it as a note (`update_task` with `note`) and read your siblings' notes with `get_task`."}

## Report to Airboss
Use the `airboss` MCP tools; they already know which thread you are.
- `list_tasks` with `mine: true` should list task {id}. If it doesn't, call `link` with
  `id: {id}` and `kind: "t3_thread"`.
- Keep the status current with `update_task`: `doing` while you work, `in_review` once your PR is open,
  `blocked` with a reason if you're stuck.
- Link every PR you open: `link` with `id: {id}`, `kind: "pr"`, `ref: "owner/repo#number"`.
- Don't mark the task done while it has a PR; it finishes when the PR merges.
- Put new work you spot in `propose_tasks`, not in this PR.
If the airboss tools aren't available, say so in your first reply and carry on with the task.
```

Research only drops "How to work" and adds "No repo: this is research or writing. Put what you find in a note with `update_task`, and mark the task done with a note when you've finished."

### The Plan it brief

```
Plan Airboss task {id}: {title}, with me, using the plan-review skill: read the code, show me the review
page, fold in my comments, and write the implementation plan once I say the plan is settled.
Don't change any code.

## The task / ## Where it sits / ## Links       (as in Do it)
{Existing subtasks: "This task already has subtasks. Keep them, plan around them, and refer to them by id."}

## Repos
{name and path per line, or "No repo is linked yet. Ask me which repos this touches."}

## When I say the plan is settled
1. Write the implementation plan as plan-review describes, and note its absolute path.
2. Call the airboss tool `import_plan` with `plan_file` (that path), `parent: {id}`, and `plan`:
   one entry per phase (`key` "p1", "p2", …) with its steps as `children` (`key` "p1.1", "p1.2", …).
   Give each step one `repo` by name; split a step that touches two repos into one step per repo.
   Use `depends_on` with keys for order inside the plan, and with task ids for the existing tasks above.
3. Tell me what `import_plan` created. Importing again with the same keys adds only new steps.
Don't import anything before I say the plan is settled.

## Report to Airboss
- `list_tasks` with `mine: true` should list task {id}. If it doesn't, link yourself as in Do it.
- Don't change the task's status; the imported steps carry the work.
If the airboss tools aren't available, say so in your first reply; still write the plan file.
```

The templates are `const` strings in `src/brief.rs`, filled by plain string replacement.

## Errors and degraded modes

The `t3` CLI writes one line to stderr and exits 1. `launch` maps that line by its start (from T3's CLI source):

| stderr starts with, or case | Error | Message in the app | Stops the loop |
|---|---|---|---|
| no binary, or exit 127 | `T3Missing` | "Do it and Plan it need T3 Code. Install it, then choose Check again." | yes |
| `No running T3 Code server uses` | `T3NotRunning` | "T3 Code isn't open. Open it, then try again." | yes |
| `Could not reach the T3 Code server` | `T3Unreachable` | "Couldn't reach T3 Code: …" | yes |
| `This T3 Code server has no` | `T3TooOld` | "Update T3 Code, then try again." | yes |
| no answer in 30 s | `Timeout` | "T3 didn't answer in 30 s. The thread may still appear; check before retrying." | yes |
| `No T3 Code project matches` | `NotAProject` | "booking-api isn't a T3 project yet." [Add to T3 and retry] | no |
| repo folder missing | `RepoMissing` | "booking-api isn't at <path> any more." | no |
| no base branch | `NoBaseBranch` | "Couldn't find booking-api's main branch. Set one in the repo's settings." | no |
| stdout isn't the JSON above | `BadAnswer` | "T3 answered in a way Airboss doesn't understand." | no |
| anything else | `T3Failed` | the first stderr line | no |

If worktree setup fails after `t3` has returned (for example, the base ref is missing on the server), the thread ends with a failed run. `sync` reports that, and it shows in Needs me as a failed run.

## Performance targets

| What | Target |
|---|---|
| `prepare` | under 20 ms (core reads, and at most one `git` call per repo, cached for the app's run) |
| Do it, one repo: button to linked thread | under 1.5 s |
| Do it, three repos | under 4 s |
| CPU and memory between presses | none: no threads, timers or watchers |

## Testing

Tests never run the real `t3` or `git`: they use `FakeT3Cli` and `FakeGit`, never `LaunchTools::real`.

- **Fakes:** `FakeT3Cli` records each argv and stdin, and answers from a script (JSON, stderr and exit code). `FakeGit` holds default branches and existing branches per path. Every test uses `Store::open_in_memory()` and a temporary `airboss.db` for `launch`'s own tables.
- **The process runner** is tested against `tests/fixtures/fake-t3`, a small shell script (not T3) that echoes argv and stdin, sleeps, or exits as told. It covers stdin passing, exit codes, the removed variables and the timeout kill.
- **`tests/commands.rs`:** exact argv for one repo, several repos, Plan it, research only and again; titles over 80 characters; the base from `origin/HEAD`, from the current branch, and from the override.
- **`tests/branch.rs`:** the slug cases above; an existing task branch kept and checked; `set_branch` written once, before the first start; `-2` and `-3` on again.
- **`tests/briefs.rs`:** golden files in `tests/golden/` for Do it with one repo, two repos, research only, Plan it with and without subtasks, and with a Slack link; cut limits.
- **`tests/failures.rs`:** each stderr row maps to its error; a global error stops the loop; repo 2 of 3 failing still links 1 and 3 and moves the task to `doing`; retry starts only repo 2; a link failure is repaired; closed and proposed tasks are refused; `add_to_t3` runs `project add`.
- **`tests/migrations.rs`:** each CHECK rejects its bad row; opening twice applies once.

## Project structure

```
crates/launch/
  Cargo.toml                 airboss-launch
  src/lib.rs                 Launcher, requests, reports
  src/db.rs                  own connection, migration (src/migrations/001_launch.sql), both tables
  src/prepare.rs  src/branch.rs   repos, choices; slug and "again" naming
  src/brief.rs               the two templates and how they're filled
  src/t3cli.rs  src/git.rs   argv, running t3 and parsing its answer; base branch and branch checks
  src/errors.rs  src/fake.rs LaunchError and its sentences; FakeT3Cli, FakeGit
  tests/…                    as listed above, plus fixtures/fake-t3 and golden/
```

## Boundaries

- **Always:** act as `launch`; save the branch before starting; link every thread it starts; send the brief on stdin; keep T3 CLI details in `src/t3cli.rs`.
- **Ask first:** changing the brief templates' instructions to agents; any new `t3` subcommand; a new crate.
- **Never:** read or write T3's files or database; call T3's server directly; start anything the user didn't press for; change status beyond `todo`/`blocked` to `doing`; add a T3 project without the user's click.

## Success criteria

- [ ] Do it on a two-repo task starts two threads with the exact commands above on one branch, links both as `launch`, saves the branch and moves the task to `doing`.
- [ ] Plan it starts one thread whose brief ends with `import_plan` for this task, and a settled plan arrives as subtasks (checked by hand once, after `mcp` ships).
- [ ] A task with no repo returns `needs_choice`, and research only starts in `~/.t3/scratch`.
- [ ] Without T3, `availability()` is `Missing` with the design's sentence.
- [ ] One failing repo doesn't stop the others; retrying starts only the failed one.
- [ ] Every brief matches its golden file.
- [ ] One real Do it on this Mac, checked by hand: the thread appears in T3 on the right branch and worktree, and `sync` shows it running.

## Checked on this Mac

25 Nov 2026:

- `t3` v0.1.12. `~/.t3/bin/t3` is a shell launcher for the app's bundled CLI; it exits 127 with "T3 Code has moved or been removed" when the app is gone. `t3 thread start --help` and `t3 thread show --help` list the flags used above.
- T3's CLI source in the app bundle (read, never changed): `--json` prints `{threadId, projectId, runId}`; `--branch` without `--worktree` is refused; a project is an id, else the deepest project folder containing the path, else found through the repo's other git worktrees; `-` reads stdin and an empty message is refused; the error sentences in the table above, with exit code 1; worktree setup (fetch, checkout, submodules, setup script) runs on the server after the command returns.
- `t3 thread list --project <folder in no project>` prints the "No T3 Code project matches" sentence and exits 1. `t3 thread show --json` costs 0.33 s of CPU. There are 29 T3 projects; `~/.t3/scratch` is the one called "No project".
- `git` 2.54: `symbolic-ref` gives `origin/main` in two repos; `rev-parse --verify --quiet` exits 0 or 1; `check-ref-format --branch` accepts `task-142-booking-flow` and rejects `bad..name`.
- A T3 thread's `T3CODE_THREAD_ID` equals its id in T3's database, so `launch`'s link ref and `mcp`'s identity match.
- `plan-review` is in `~/.claude/skills/plan-review` and has no import step yet. The `composio` CLI 0.5.0 and its skill are installed.

## Not verified yet

- **`t3 thread start` itself** wasn't run, because it would create a real thread. Its output shape comes from T3's source.
- **T3 desktop closed:** the "No running T3 Code server" sentence comes from the source, not from a run.
- **A `--branch` that already exists** in the repo. Setup runs after `start` returns, so a clash would show as a failed run, which `sync` reports.
- **Opening one thread in the T3 app.** There's no deep link to a thread. `t3 thread show --json` gives a web `link`, and `t3 app <path>` opens a project. The app spec decides.
- **Whether T3 keeps `--title`** or later renames the thread (it can regenerate titles); `sync` shows the live title either way.
- **The Composio Slack connection** being live, and Codex threads seeing the `airboss` tools (see SPEC-mcp).
