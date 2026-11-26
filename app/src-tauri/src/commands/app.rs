//! App commands: the opener, the clipboard, the CLI, settings, focus, and the windows.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use airboss_core::{LinkKind, TaskId};
use airboss_services::ThreadKey;
use tauri_plugin_log::log;

use crate::classify::{expand_home, file_url};
use crate::dto::{
    CliStatus, CopyWhat, InstallCliResult, NavOpen, OpenTarget, QuickaddReset, path_text,
};
use crate::errors::{AppError, AppResult, Ctx, ErrorCode, MSG_HOTKEY_IN_USE, MSG_LINK_GONE};
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

/// Opens a link that passes the allowlist (`opener::decide`): `https` in the browser, a T3
/// thread in T3 Code, and a document in TextEdit or Preview. Any other file (an app, a script, a folder,
/// anything without a known document type) is shown in Finder instead, because agents can add
/// links and opening it could run it.
fn open_checked(s: &AppState, url: &str) -> AppResult<()> {
    let opened = match opener::decide(url)? {
        Opening::Browser(u) | Opening::Thread(u) => s.shell.open_url(&u),
        Opening::Open(path, viewer) => s.shell.open_file(&path, viewer.app()),
        Opening::Reveal(path) => s.shell.reveal(&path),
    };
    opened.map_err(|e| AppError::internal(format!("couldn't open a link: {e}")))
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
    s.store_settings(new.clone())?;
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
