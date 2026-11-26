# Updates inside the app

**Status:** building (alpha 4). **Owner's ask:** "provide the update button in the app" so a tester updates with one click instead of re-running the install line.

## Objective

Airboss notices when a newer release is out and installs it in one click: download, check its signature, swap the app in place, restart. There's still no account and no server of ours: the release list is a file in the public repo, and the download is the GitHub release asset.

Success means a tester on alpha 4 sees "Airboss 0.1.0-alpha.5 is available" and, after one click, is running alpha 5 with the same window, tasks and settings, and no Gatekeeper warning. Alpha 3 and earlier have no updater: their users run the install line once more.

## Decisions

1. **Tauri's updater plugin** (`tauri-plugin-updater` 2.14.1), driven from Rust only. The web views get Airboss's own three commands and one event; the plugin's JavaScript API and permissions aren't used.
2. **The manifest is `updates/latest.json` on `main`**, read from `https://raw.githubusercontent.com/amantibrewal310/airboss/main/updates/latest.json`. GitHub's `releases/latest` skips pre-releases, and every release so far is one. The format is the plugin's static JSON: `version`, `notes`, `pub_date`, and `platforms["darwin-aarch64"]` with `url` (the release's `.app.tar.gz`) and `signature`. A release uploads its assets first and commits the manifest last, so the manifest never names a file that isn't there.
3. **Updates are signed with Airboss's own updater key** (minisign, from `tauri signer generate`), separate from Apple code signing. The public key is in `tauri.conf.json`. The private key is `~/.tauri/airboss-updater.key` on the owner's Mac, outside the repo, and its password is the login keychain item `airboss-updater-key`. The plugin refuses a download whose signature doesn't match, so neither a changed manifest nor a changed asset can install other code. If the key is lost, installed apps can't take signed updates from a new key: their users run the install line once, and the next app carries the new public key.
4. **When it checks:** 30 seconds after start, then whenever 6 hours of wall-clock time have passed. A 5-minute tick looks at the clock, so sleep doesn't stretch the wait. It checks only when **Settings › General › Check for updates automatically** is on (the default) and Airboss is an installed release build: inside an `.app`, not a debug build. **Airboss › Check for Updates…** checks at any time; in a development build it says updates work only in the installed app.
5. **What you see:** a card at the foot of the sidebar, above the connections line, only while there's something to say. "Later" hides it until Airboss starts again, a newer version appears, or you choose Check for Updates….

   | State | Card |
   |---|---|
   | available | **Airboss 0.1.0-alpha.5 is available** · Update and restart · What's new · Later |
   | downloading | **Downloading Airboss 0.1.0-alpha.5…** · a progress bar with the percent, when the size is known |
   | installing | **Installing Airboss 0.1.0-alpha.5…** |
   | restarting | **Restarting…** |
   | failed | **Couldn't update to 0.1.0-alpha.5.** · the reason · Try again · Later |

   "What's new" opens the release's GitHub page. Check for Updates… toasts "You have the newest Airboss, 0.1.0-alpha.4." when up to date, "Couldn't check for updates: <reason>" when it fails, and "Airboss 0.1.0-alpha.5 is available." with an **Update and restart** action when there is one. Automatic checks only log failures: being offline isn't news.
6. **Update and restart:** downloads with progress, verifies, and replaces the app where it is, keeping the old one until the new one is in place (the plugin's swap). Then Airboss saves and stops its services as on quit, releases its single-instance lock, and opens the new app through LaunchServices **without `--hidden`**, so an Airboss opened at login comes back with its window showing. If anything fails before the swap, the old app is untouched and the card says why.
7. **Privacy:** one HTTPS GET of the manifest from `raw.githubusercontent.com` per check, and the download from `github.com` when you update. Neither sends anything about you or your tasks. README, getting started and Settings say so.

## Interfaces

- `UpdateStatus` (Rust `dto.rs`, exported to TypeScript), tagged by `state`: `idle` · `available {version, notes_url}` · `downloading {version, percent: number | null}` · `installing {version}` · `restarting {version}` · `failed {version, message}`. `idle` means nothing to show: not checked yet, up to date, or checks off.
- Commands (Appendix A of SPEC-app):
  - `update_status` → `UpdateStatus`. The UI reads it at start, then follows the event.
  - `check_for_updates` → `UpdateStatus`. A manual check, with the toasts above.
  - `install_update` → `()`. Downloads, installs and restarts; progress arrives as events. On failure the status becomes `failed`, and the command still returns `Ok`, so the card is the only place that says so. It returns `not_found` when there's nothing to install, and does nothing while an install already runs.
- Event `update:changed` with the `UpdateStatus`, on every change.
- `Settings.check_for_updates: bool`, default `true`.
- Native: `Shell::updater() -> Arc<dyn Updater>` and `Shell::restart()`. `Updater` has `available()` (an installed release build), `check() -> Result<Option<Release>, String>` and `install(version, progress)`. Tests use `FakeUpdater`; `RecordingShell` records `restart`.
- Menu: **Airboss › Check for Updates…**, right under About Airboss. The command palette has "Check for updates".

## Releasing

`tauri.bundle.conf.json` (the release build only) sets `createUpdaterArtifacts`, so `pnpm bundle` also writes `Airboss.app.tar.gz` and its `.sig`. It needs `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, which `scripts/release-build.sh` reads from the key file and the keychain. After the GitHub release is published with the `.dmg`, its `.sha256` and `Airboss_<version>_aarch64.app.tar.gz`, `scripts/update-manifest.mjs` writes `updates/latest.json`, and that commit on `main` is what installed apps see.

## Testing

- **Rust:** with `FakeUpdater` and `RecordingShell`: the schedule (`due`), automatic checks respecting the setting and `available()`, each manual check outcome and its toast, the install's status sequence and the restart, a failed install, and a second install while one runs. `version.rs` checks the updater's public key and endpoint.
- **UI:** the card in every state, Later, What's new, the progress bar's accessible value, the Settings switch, and the palette command. The mock backend implements all three commands, and an end-to-end test runs available → downloading → restarting.
- **Real, by hand before the release:** a release build at `0.1.0-alpha.4`, installed in a temporary folder and built with a QA-only config that points at a loopback manifest, updates itself to a locally built `0.1.0-alpha.4.1`. Check that the bundle is replaced, the signature is verified (a wrong signature is refused), the app restarts with its window showing, and `airboss --version` reports the new version. This needs the owner's Airboss quit while it runs (single instance).

## Boundaries

- **Always:** verify the signature (the plugin does) and never install over a running update. Keep the old app until the new one is in place.
- **Never:** put the private key or its password in the repo, a log or an issue. Never let an endpoint come from anything a user or agent can write at runtime: it's compiled in.
- **Not now:** release notes inside the app (What's new links to GitHub), delta updates, update channels, and Intel Macs.
