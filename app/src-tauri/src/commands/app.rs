//! App commands: the opener, the clipboard, the CLI, settings, focus, and the windows.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use airboss_core::{LinkKind, TaskId};
use airboss_services::ThreadKey;
use tauri_plugin_log::log;

use crate::classify::{expand_home, file_url};
use crate::dto::{
    CliStatus, CopyWhat, InstallCliResult, NavOpen, OpenTarget, QuickaddReset, path_text,
};
use crate::errors::{AppError, AppResult, Ctx, ErrorCode, MSG_HOTKEY_IN_USE, MSG_LINK_GONE};
use crate::handlers::{Kind, verified_app};
use crate::menu::{FocusContext, WindowName};
use crate::notify::Permission;
use crate::opener::{self, Opening};
use crate::settings::{Settings, check_hotkey, hotkey_label};
use crate::shell::UiEvent;
use crate::state::{AppState, Caller, lock};

/// The text "Copy agent note" copies.
pub const AGENT_NOTE: &str = include_str!("../../resources/agent-note.md");
/// The command "Copy setup command" copies.
pub const SETUP_COMMAND: &str = "claude mcp add --scope user airboss -- ~/.local/bin/airboss mcp";
/// What "Copy Codex setup" copies: `codex mcp add` can't set `env_vars`, which Codex needs to
/// pass T3's thread id and Herdr's terminal id to the server, so it's a table for
/// `~/.codex/config.toml` (`{bin}` is the CLI's absolute path).
pub const CODEX_SETUP: &str = "[mcp_servers.airboss]\ncommand = \"{bin}\"\nargs = [\"mcp\"]\nenv_vars = [\"T3CODE_THREAD_ID\", \"HC_TERMINAL_ID\"]\n";
/// How long Connect Claude Code waits for `claude mcp add`.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// Where Claude Code's `claude` usually is, under the home folder and then the system's,
/// before the app's `PATH` and the user's login shell's.
const CLAUDE_PLACES: &[&str] = &[".local/bin/claude", ".claude/local/claude"];
const CLAUDE_SYSTEM_PLACES: &[&str] = &["/opt/homebrew/bin/claude", "/usr/local/bin/claude"];

/// Opens a thread in T3 Code through `sync.thread_url` (`t3code://threads/…`).
pub fn open_thread(s: &AppState, _c: Caller, key: ThreadKey) -> AppResult<()> {
    let url = s.services.sync.thread_url(&key).ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "That thread can't be opened from here.",
        )
    })?;
    open_checked(s, &url)
}

/// Opens a URL, or a link by its id, through the allowlist in `opener.rs`.
pub fn open_url(s: &AppState, _c: Caller, target: OpenTarget) -> AppResult<()> {
    let url = match target {
        OpenTarget::Url(u) => u,
        OpenTarget::Link(id) => {
            // A link is found by kind; there are six, each one small read.
            let kinds = [
                LinkKind::Pr,
                LinkKind::T3Thread,
                LinkKind::ClaudeSession,
                LinkKind::Doc,
                LinkKind::Slack,
                LinkKind::GithubIssue,
            ];
            let mut found = None;
            for kind in kinds {
                let links = s.core_as(Ctx::Link, |st| st.links_of_kind(kind))?;
                if let Some(l) = links.into_iter().find(|l| l.id == id) {
                    found = Some(l);
                    break;
                }
            }
            let found = found.ok_or_else(|| AppError::new(ErrorCode::NotFound, MSG_LINK_GONE))?;
            match found.url {
                Some(u) => u,
                None if found.r#ref.starts_with('/') || found.r#ref.starts_with('~') => {
                    file_url(&expand_home(&found.r#ref, &s.config.home))
                }
                None => {
                    return Err(AppError::new(
                        ErrorCode::NotFound,
                        "That link has nothing to open.",
                    ));
                }
            }
        }
    };
    open_checked(s, &url)
}

/// Opens a link that passes the allowlist (`opener::decide`): `https` in the browser and a T3
/// thread in T3 Code, each only once that app's signature checks out (`handlers.rs`), and a
/// document in TextEdit or Preview. Any other file (an app, a script, a folder,
/// anything without a known document type) is shown in Finder instead, because agents can add
/// links and opening it could run it.
fn open_checked(s: &AppState, url: &str) -> AppResult<()> {
    let check = s.shell.app_check();
    let opened = match opener::decide(url)? {
        // Opened with exactly the app whose signature was checked (`handlers.rs`).
        Opening::Browser(u) => {
            let app = verified_app(Kind::Browser, &u, &*check, &s.config.home)?;
            s.shell.open_url(&u, &app)
        }
        Opening::Thread(u) => {
            let app = verified_app(Kind::Thread, &u, &*check, &s.config.home)?;
            s.shell.open_url(&u, &app)
        }
        Opening::Open(path, viewer) => s.shell.open_file(&path, viewer.app()),
        Opening::Reveal(path) => s.shell.reveal(&path),
    };
    opened.map_err(|e| AppError::internal(format!("couldn't open a link: {e}")))
}

/// Help › Report a Problem…: opens the repo's bug form in the browser, with this version and
/// the macOS version already filled in.
pub fn report_problem(s: &AppState, c: Caller) -> AppResult<()> {
    let macos = std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    let url = report_problem_url(env!("CARGO_PKG_VERSION"), &macos);
    open_url(s, c, OpenTarget::Url(url))
}

/// The bug form's address. GitHub fills a form's fields from query parameters named by their
/// ids (`.github/ISSUE_TEMPLATE/bug.yml`).
pub fn report_problem_url(version: &str, macos: &str) -> String {
    /// Keeps the characters a query value may hold as they are, and escapes the rest.
    fn encode(v: &str) -> String {
        v.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect()
    }
    format!(
        "{}/issues/new?template=bug.yml&version={}&macos={}",
        env!("CARGO_PKG_REPOSITORY"),
        encode(version),
        encode(macos)
    )
}

/// "Check again": every service rechecks its connections.
pub fn recheck_connections(s: &AppState, _c: Caller) -> AppResult<()> {
    for svc in s.services.all() {
        svc.recheck();
    }
    s.refresh_connections();
    Ok(())
}

/// Copies a task number, the agent note or the setup command.
pub fn copy_text(s: &AppState, _c: Caller, what: CopyWhat) -> AppResult<()> {
    let text = match what {
        CopyWhat::TaskNumber(id) => id.0.to_string(),
        CopyWhat::AgentNote => AGENT_NOTE.to_string(),
        CopyWhat::SetupCommand => SETUP_COMMAND.to_string(),
        CopyWhat::CodexSetup => codex_setup(&s.config.home),
        CopyWhat::Link(url) => url,
    };
    s.shell
        .copy_text(&text)
        .map_err(|e| AppError::internal(format!("clipboard: {e}")))
}

/// Links `~/.local/bin/airboss` to the bundled CLI.
pub fn install_cli(s: &AppState, _c: Caller) -> AppResult<InstallCliResult> {
    let sidecar = s.shell.sidecar().ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "The command-line tool comes with the app. Open Airboss from Applications, then try again.",
        )
    })?;
    let bin = s.config.home.join(".local/bin");
    let link = bin.join("airboss");
    let fail = |e: std::io::Error| AppError::internal(format!("install_cli: {e}"));
    std::fs::create_dir_all(&bin).map_err(fail)?;
    match std::fs::symlink_metadata(&link) {
        Ok(m) if m.file_type().is_symlink() => {
            // Replace our own link (this app, or a copy of it that moved); never someone
            // else's.
            let target = std::fs::read_link(&link).map_err(fail)?;
            let ours = target == sidecar || target.ends_with("Airboss.app/Contents/MacOS/airboss");
            if !ours {
                return Err(AppError::new(
                    ErrorCode::AlreadyExists,
                    format!(
                        "~/.local/bin/airboss already links to {}. Remove it, then try again.",
                        target.display()
                    ),
                ));
            }
            std::fs::remove_file(&link).map_err(fail)?;
        }
        Ok(_) => {
            return Err(AppError::new(
                ErrorCode::AlreadyExists,
                "~/.local/bin/airboss already exists and isn't Airboss's. Remove it, then try again.",
            ));
        }
        Err(_) => {}
    }
    std::os::unix::fs::symlink(&sidecar, &link).map_err(fail)?;
    let on_path = s.check_login_path();
    s.refresh_connections();
    Ok(InstallCliResult {
        path: path_text(&link),
        on_path,
    })
}

/// The `[mcp_servers.airboss]` table for `~/.codex/config.toml`, with the CLI's real path.
pub fn codex_setup(home: &Path) -> String {
    CODEX_SETUP.replace("{bin}", &home.join(".local/bin/airboss").to_string_lossy())
}

/// An executable file at `p`.
fn runnable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Finds `claude` the way `gh` is found (`sync::gh::find_gh`): the usual places first, then
/// the app's `PATH`, then the user's login shell's (`login_path.rs`), since an app opened from
/// Finder has launchd's short `PATH`. Returns it with the `PATH` its child should get, so a
/// `claude` that's a Node script finds `node`.
fn find_claude(s: &AppState) -> Option<(PathBuf, OsString)> {
    let app_path = std::env::var_os("PATH").unwrap_or_default();
    if let Some(p) = &s.config.claude_path {
        return runnable(p).then(|| (p.clone(), app_path));
    }
    let home = &s.config.home;
    let login = s.config.shell.as_deref().and_then(|sh| {
        crate::login_path::login_shell_path(sh, &[], crate::login_path::DEADLINE)
            .map(OsString::from)
    });
    let on = |path: &OsString| {
        std::env::split_paths(path)
            .map(|d| d.join("claude"))
            .find(|p| runnable(p))
    };
    let found = CLAUDE_PLACES
        .iter()
        .map(|p| home.join(p))
        .chain(CLAUDE_SYSTEM_PLACES.iter().map(PathBuf::from))
        .find(|p| runnable(p))
        .or_else(|| on(&app_path))
        .or_else(|| login.as_ref().and_then(on))?;
    Some((found, login.unwrap_or(app_path)))
}

/// The first line worth showing from what `claude` printed: stderr's, else stdout's.
fn first_line(out: &airboss_launch::CmdOutput) -> String {
    [&out.stderr, &out.stdout]
        .iter()
        .flat_map(|t| t.lines())
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("it gave no reason")
        .to_string()
}

/// "Connect Claude Code", only ever on the user's click: `claude mcp add --scope user airboss
/// -- <home>/.local/bin/airboss mcp`, with argv only (no shell, so the path is absolute) and a
/// 20 s deadline. Already registered counts as done. The connections refresh after.
pub fn connect_claude(s: &AppState, _c: Caller) -> AppResult<()> {
    let home = &s.config.home;
    let bin = home.join(".local/bin/airboss");
    if crate::connections::cli_at(&bin) != crate::connections::Cli::Ready {
        return Err(AppError::new(
            ErrorCode::NotFound,
            "Install the command-line tool first, then connect Claude Code.",
        ));
    }
    if crate::connections::agent_tools(home).claude {
        s.refresh_connections();
        return Ok(());
    }
    let (claude, path) = find_claude(s).ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "Couldn't find Claude Code's claude command. Copy the setup command and run it in Terminal.",
        )
    })?;
    // `claude`'s own folder first, so a Node install finds the `node` beside it.
    let path = std::env::join_paths(
        claude
            .parent()
            .map(Path::to_path_buf)
            .into_iter()
            .chain(std::env::split_paths(&path)),
    )
    .unwrap_or(path);
    let mut env = airboss_launch::child_env(std::env::vars_os());
    env.retain(|(k, _)| k != "PATH" && k != "HOME");
    env.push(("PATH".into(), path));
    env.push(("HOME".into(), home.clone().into_os_string()));
    let argv: Vec<String> = [
        "mcp",
        "add",
        "--scope",
        "user",
        "airboss",
        "--",
        &bin.to_string_lossy(),
        "mcp",
    ]
    .iter()
    .map(|a| a.to_string())
    .collect();
    log::info!("connect_claude: {} {}", claude.display(), argv.join(" "));
    let out = airboss_launch::ProcessRunner::new(claude, home.clone(), env)
        .run(&argv, None, CONNECT_TIMEOUT)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::TimedOut => AppError::new(
                ErrorCode::Timeout,
                "Claude Code didn't answer within 20 seconds. Copy the setup command and run it in Terminal.",
            ),
            _ => AppError::new(
                ErrorCode::NotFound,
                format!("Couldn't run Claude Code's claude command: {e}"),
            ),
        })?;
    let already = format!("{}{}", out.stdout, out.stderr).contains("already exists");
    if out.status != Some(0) && !already {
        let line = first_line(&out);
        log::warn!(
            "connect_claude: claude exited with {:?}: {line}",
            out.status
        );
        return Err(AppError::new(
            ErrorCode::Internal,
            format!("Couldn't connect Claude Code: {line}"),
        ));
    }
    log::info!("connect_claude: connected");
    s.refresh_connections();
    Ok(())
}

/// The settings now.
pub fn get_settings(s: &AppState, _c: Caller) -> AppResult<Settings> {
    Ok(s.settings())
}

/// Saves new settings: re-registers the hotkey (refusing macOS's own and keys in use),
/// switches autostart, and applies the appearance to both windows.
pub fn set_settings(s: &AppState, _c: Caller, settings: Settings) -> AppResult<Settings> {
    let old = s.settings();
    let mut new = settings;
    // Folds are saved through `set_folded`; keep the current ones.
    new.folded = old.folded.clone();
    if new.hotkey != old.hotkey {
        check_hotkey(&new.hotkey)?;
        s.shell
            .register_hotkey(&old.hotkey, &new.hotkey)
            .map_err(|e| {
                log::info!("hotkey {} refused: {e}", new.hotkey);
                AppError::new(ErrorCode::HotkeyInUse, MSG_HOTKEY_IN_USE)
            })?;
        s.set_hotkey_taken(None);
    }
    if new.open_at_login != old.open_at_login {
        s.shell
            .set_autostart(new.open_at_login)
            .map_err(|e| AppError::internal(format!("autostart: {e}")))?;
    }
    if new.appearance != old.appearance {
        s.shell.apply_appearance(new.appearance);
    }
    // Turning notifications on asks macOS, if it hasn't been asked yet.
    if new.notifications && !old.notifications {
        let n = s.shell.notifications();
        if n.permission() == Permission::NotAsked {
            n.request();
        }
    }
    let launcher_changed =
        new.start_agents_in != old.start_agents_in || new.herdr_agent != old.herdr_agent;
    s.store_settings(new.clone())?;
    // "Start agents in" changes the launcher and the Herdr row.
    if launcher_changed {
        s.refresh_connections();
    }
    Ok(new)
}

/// Whether macOS lets Airboss notify, for Settings' explanation when it doesn't.
pub fn notification_permission(s: &AppState, _c: Caller) -> AppResult<Permission> {
    Ok(s.shell.notifications().permission())
}

/// "Open System Settings": Notifications, at Airboss, to allow them.
pub fn open_notification_settings(s: &AppState, _c: Caller) -> AppResult<()> {
    s.shell
        .notifications()
        .open_settings()
        .map_err(|e| AppError::internal(format!("couldn't open System Settings: {e}")))
}

/// Folds or unfolds parents; saved a second after the last change.
pub fn set_folded(s: &AppState, _c: Caller, ids: Vec<TaskId>, folded: bool) -> AppResult<()> {
    {
        let mut st = lock(&s.settings);
        if folded {
            for id in ids {
                if !st.folded.contains(&id) {
                    st.folded.push(id);
                }
            }
        } else {
            st.folded.retain(|f| !ids.contains(f));
        }
    }
    s.save_folds_soon();
    Ok(())
}

/// The UI's focus zone, selection and view: the native menus follow it.
pub fn set_focus_context(s: &AppState, _c: Caller, context: FocusContext) -> AppResult<()> {
    *lock(&s.focus) = context;
    s.refresh_menus();
    Ok(())
}

/// The main window's first render is done: show it (unless started with `--hidden`).
pub fn ui_ready(s: &AppState, _c: Caller, window: WindowName) -> AppResult<()> {
    if window == WindowName::Main {
        log::info!(
            "ui_ready after {} ms; epoch_ms={}",
            s.config_started().elapsed().as_millis(),
            epoch_ms()
        );
        if !s.config.start_hidden && !s.main_shown.swap(true, std::sync::atomic::Ordering::SeqCst) {
            s.shell.show_main();
        }
    }
    Ok(())
}

/// Milliseconds since the epoch, for `measure.sh`.
pub fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Quick add has focus after the hotkey: log the time from the key press.
pub fn quickadd_shown(s: &AppState, _c: Caller) -> AppResult<()> {
    if let Some(pressed) = lock(&s.timing).quickadd_pressed.take() {
        log::info!(
            "quickadd shown {} ms after the hotkey",
            pressed.elapsed().as_millis()
        );
    }
    Ok(())
}

/// Esc or Create: hide quick add; the UI resets on `quickadd:reset`.
pub fn quickadd_hide(s: &AppState, _c: Caller) -> AppResult<()> {
    s.shell.hide_quickadd();
    s.shell.emit(UiEvent::QuickAddReset(QuickaddReset {
        hotkey: hotkey_label(&s.settings().hotkey),
        keep_text: Some(false),
    }));
    Ok(())
}

/// Quick add's height follows its content.
pub fn quickadd_resize(s: &AppState, _c: Caller, height: f64) -> AppResult<()> {
    if height.is_finite() && height > 0.0 {
        s.shell.resize_quickadd(height.min(900.0));
    }
    Ok(())
}

/// Shows and focuses the main window, and tells it where to go.
pub fn show_main(
    s: &AppState,
    _c: Caller,
    view: Option<String>,
    task: Option<TaskId>,
    needs_key: Option<String>,
) -> AppResult<()> {
    s.main_shown
        .store(true, std::sync::atomic::Ordering::SeqCst);
    s.shell.show_main();
    s.shell.emit(UiEvent::NavOpen(NavOpen {
        view,
        task,
        needs_key,
    }));
    Ok(())
}

/// Where the CLI link lives, whether it works, and whether its folder is on the user's
/// `PATH`: their shell's, once `check_login_path` has run, else the app's own (launchd's).
pub fn cli_status(s: &AppState) -> CliStatus {
    let bin = s.config.home.join(".local/bin");
    let link = bin.join("airboss");
    let on_path = (*lock(&s.login_path)).unwrap_or_else(|| {
        std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d == bin))
    });
    CliStatus {
        // Through the link: a dangling one, or one to a file that can't run, isn't installed.
        installed: crate::connections::cli_at(&link) == crate::connections::Cli::Ready,
        path: path_text(&link),
        on_path,
    }
}

impl AppState {
    /// When the process started (as near as `main` knows).
    pub(crate) fn config_started(&self) -> Instant {
        self.config.started
    }
}
