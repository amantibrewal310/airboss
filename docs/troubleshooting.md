# Troubleshooting

Start with the footer at the bottom of the sidebar. It reads "All connected", or in orange names the problem ("GitHub not signed in"), or says "2 connections need setup". Click it to see one row per connection, with what it does and how to fix it. **Check again** re-checks them all once you've fixed something.

When a source is off, Airboss keeps its last known state and marks it "May be out of date". Nothing else stops working.

## GitHub

Airboss reads PRs with the GitHub CLI. It looks for `gh` at `/opt/homebrew/bin/gh`, then `/usr/local/bin/gh`, then on the app's `PATH`. An app opened from Finder has a short `PATH` without Homebrew, so install `gh` in one of the first two places.

| The GitHub row says | What to do |
|---|---|
| Not installed | Install the GitHub CLI, then choose Check again. |
| Not signed in | Run `gh auth login` in a terminal, then choose Check again. |
| Not working | `gh` failed, often because the network is down. It's tried again after 1, 2, 4 and up to 30 minutes; Check again tries at once. |
| &lt;login&gt;: … | One account has a problem. The popover says which, for example "&lt;login&gt; needs `gh auth login` again", or that it's rate-limited until a given time. Only that account's PRs wait. |

Check what `gh` sees with:

```sh
gh auth status
```

**Several accounts.** Airboss uses every github.com account that `gh auth status` shows as signed in, the active one first. It never switches your active account. For each other account it asks `gh auth token --user <login>` for a token, keeps it in memory, and passes it only to the `gh` process it starts. A PR is read with the first account that can see its repo, and that account is tried first next time. So a PR in a private repo that only your second account can see still syncs, as long as that account is signed in. To add one, run `gh auth login` again and sign in with the other account.

If you link a PR that none of your accounts can see, the link stays, and looking it up says "None of your GitHub accounts can see owner/repo#n."

**A merged PR didn't finish its task.** PRs are read about every 5 minutes, and when you open the task. A task finishes when all its PRs have merged or closed, with at least one merged. It also waits while an agent on the task is still running or waiting on you. A task you reopened after it finished stays open.

## T3 Code

Do it and Plan it run T3 Code's CLI at `~/.t3/bin/t3`. Live thread state comes from T3's database, `~/.t3/userdata/statev2.sqlite`, which Airboss only ever opens read-only.

| You see | What to do |
|---|---|
| The T3 Code row says Not found or Not installed, and the task panel says "Do it and Plan it need T3 Code. Install it, then choose Check again." | Install T3 Code. Check that `~/.t3/bin/t3 --version` prints a version, then choose Check again. |
| "T3 Code isn't open. Open it, then try again." | Open T3 Code, then press Do it again. |
| "&lt;repo&gt; isn't a T3 project yet." | Choose **Add to T3 and retry** on the toast. |
| "&lt;repo&gt; is still starting. Check T3, or choose Start again." | A start didn't answer. Airboss links the thread when T3 shows it. If no thread appears, choose Start again. |
| The T3 Code row says Can't open threads | T3's `environment-id` file is missing or isn't a UUID, so Open thread can't open T3. Opening T3 once writes it again. |

**The T3 Code row says "Live state broken since 14:02".** T3 Code changed its database, and Airboss found something it doesn't know: a missing table or column, an unknown run status or request kind, or a missing JSON key. Rather than guess, it stops reading T3, keeps the last known thread state marked "May be out of date", and says what it found in the popover, for example `unknown T3 run status "paused"`. It tries again on T3's next write and every hour, so it recovers by itself if the change was temporary. If it doesn't, Airboss needs updating for the new T3 version. To see exactly which check fails, run from the repo:

```sh
cargo test -p airboss-sync --release --test real_t3 -- --ignored --nocapture
```

That test reads your real T3 database, read-only.

## The quick-add hotkey

The default is ⌃⌥N. To change it, open Settings (⌘,), press the hotkey button and type a new one.

- "That key is in use by another app. Pick another." Another app already holds it.
- "macOS uses that key for Spotlight or input sources. Pick another." ⌃Space, ⌃⌥Space, ⌘Space, ⌥⌘Space and ⌃⌘Space are refused.
- "A hotkey needs ⌃, ⌥ or ⌘ and one other key."

If another app takes the hotkey before Airboss starts, Airboss can't register it, and only its log says so: `the hotkey Control+Alt+N is taken`. Quick add then doesn't open from other apps. Pick another key in Settings, or quit the other app and restart Airboss.

## Agents

**An agent shows up as `agent:unknown/<pid>`,** or `list_tasks` with `mine: true` says "This server doesn't know which thread it serves". The `airboss mcp` process didn't get a thread id from its client.

- **Codex** passes a server only a few variables. Add `env_vars = ["T3CODE_THREAD_ID"]` to `[mcp_servers.airboss]` in `~/.codex/config.toml`, then start a new thread. `codex mcp add` can't set this; it's a hand edit. See [Getting started](getting-started.md#codex).
- **Codex outside T3 Code** has no thread id to pass. Set a name with `AIRBOSS_THREAD`, for example `--env AIRBOSS_THREAD=codex/<name>` on `codex mcp add`.
- **Any other client:** set `AIRBOSS_THREAD` in the server's environment in its MCP config.

The server's instructions name the actor it acts as, so asking the agent "who does Airboss say you are?" is a quick check.

**Agents don't report.** Check the tools are connected: the agent should list six `airboss` tools. Then add the [agent note](getting-started.md#tell-agents-to-report) to the repo's `AGENTS.md` or `CLAUDE.md`.

**An agent's update doesn't show in the window.** They should show within 300 ms. If the Live updates row says "Not watching", changes from agents show when the window gains focus; restart Airboss. Also check the agent and the app use the same database (see below).

## The command-line tool

- `airboss: command not found`: `~/.local/bin` isn't on your `PATH`. Add `export PATH="$HOME/.local/bin:$PATH"` to `~/.zshrc`.
- "~/.local/bin/airboss already exists and isn't Airboss's. Remove it, then try again." You put another file there, perhaps a CLI you built yourself. Remove it, or keep using it.
- "The command-line tool comes with the app. Open Airboss from Applications, then try again." The app isn't running from its bundle, for example in `pnpm tauri dev`.
- The link breaks if you move the app. Install it again from the new place.

## The database

Tasks live in `~/Library/Application Support/Airboss/airboss.db`, with SQLite's `-wal` and `-shm` files beside it. The app's settings are in `app-settings.json` in the same folder.

`AIRBOSS_DB` points the app, the CLI and `airboss mcp` at another file. The settings file follows it, so a throwaway database gets throwaway settings too. Everything that should see the same tasks needs the same value: for agents, set it in the MCP server's environment. To run the app on another database, quit it first (a second copy only brings the first one forward), then:

```sh
AIRBOSS_DB=/tmp/airboss-try.db /Applications/Airboss.app/Contents/MacOS/airboss-app
```

A new database comes with new settings, so it's a first run, and that turns open at login back on. Turn it off again in Settings if you don't want it.

- **"Airboss can't open its database"** fills the window, with the path and the reason, **Show in Finder** and **Try again**.
- **"Database moved"** in the footer: the file was moved or replaced while the app was open. Quit Airboss and open it again.

## Logs

- **The app** logs to `~/Library/Logs/Airboss/app.log`: start-up, connections, refused hotkeys, background errors and panics, plus every line from its services, tagged `airboss-sync`, `airboss-launch` or `airboss-attention` (for example "Claude sync broken: ps failed"). That holds when Airboss opens from Finder or at login, where nothing reads stderr. The file is rotated at 1 MB and the last two are kept, as `app_<date>.log`.
- **The CLI** prints errors to stderr, starting `airboss:`, and exits with code 1.
- **`airboss mcp`** writes only MCP messages to stdout. Its own errors go to stderr; where that ends up depends on the agent client.
- **Task history** is in the app's activity list, in `airboss show <id>`, and across all tasks in `airboss log`.
