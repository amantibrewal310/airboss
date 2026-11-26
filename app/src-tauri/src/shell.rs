//! `Shell`: the one seam between the app's logic and the native side.
//!
//! Everything that isn't `core` or a service goes through it: events to the web views, windows,
//! the clipboard, the opener, the folder picker, autostart, the hotkey, the menus and the tray.
//! The app implements it with Tauri (`native::TauriShell`); tests use [`RecordingShell`], so
//! every command is a plain function that runs without Tauri.

use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use airboss_services::NeedsList;
use serde::Serialize;

use crate::dto::{
    AppearanceChanged, BackgroundError, ConnectionsChanged, MenuAction, NavOpen, Patch,
    QuickaddReset, ToastEvent, UpdateStatus,
};
use crate::errors::AppError;
use crate::handlers::{AppCheck, FakeAppCheck};
use crate::herdr::{FakeHerdrFolders, HerdrFolders};
use crate::menu::MenuState;
use crate::notify::{Notifications, RecordingNotifications};
use crate::settings::Appearance;
use crate::updates::{FakeUpdater, Updater};

/// An event for the web views (Appendix B).
#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    TreePatch(Patch),
    NeedsChanged(NeedsList),
    ConnectionsChanged(ConnectionsChanged),
    MenuAction(MenuAction),
    NavOpen(NavOpen),
    QuickAddReset(QuickaddReset),
    Toast(ToastEvent),
    AppearanceChanged(AppearanceChanged),
    ErrorBackground(BackgroundError),
    /// The database broke while the app ran: the full-window database problem state.
    DbUnavailable(AppError),
    /// The in-app update's status changed (SPEC-updates).
    UpdateChanged(UpdateStatus),
}

impl UiEvent {
    /// The event's name, e.g. `"tree:patch"`.
    pub fn name(&self) -> &'static str {
        match self {
            UiEvent::TreePatch(_) => "tree:patch",
            UiEvent::NeedsChanged(_) => "needs:changed",
            UiEvent::ConnectionsChanged(_) => "connections:changed",
            UiEvent::MenuAction(_) => "menu:action",
            UiEvent::NavOpen(_) => "nav:open",
            UiEvent::QuickAddReset(_) => "quickadd:reset",
            UiEvent::Toast(_) => "toast",
            UiEvent::AppearanceChanged(_) => "appearance:changed",
            UiEvent::ErrorBackground(_) => "error:background",
            UiEvent::DbUnavailable(_) => "db:unavailable",
            UiEvent::UpdateChanged(_) => "update:changed",
        }
    }

    /// The payload as JSON.
    pub fn payload(&self) -> serde_json::Value {
        fn json<T: Serialize>(t: &T) -> serde_json::Value {
            serde_json::to_value(t).unwrap_or(serde_json::Value::Null)
        }
        match self {
            UiEvent::TreePatch(p) => json(p),
            UiEvent::NeedsChanged(n) => json(n),
            UiEvent::ConnectionsChanged(c) => json(c),
            UiEvent::MenuAction(m) => json(m),
            UiEvent::NavOpen(n) => json(n),
            UiEvent::QuickAddReset(q) => json(q),
            UiEvent::Toast(t) => json(t),
            UiEvent::AppearanceChanged(a) => json(a),
            UiEvent::ErrorBackground(e) => json(e),
            UiEvent::DbUnavailable(e) => json(e),
            UiEvent::UpdateChanged(u) => json(u),
        }
    }

    /// Only the quick-add panel listens to it.
    pub fn for_quickadd(&self) -> bool {
        matches!(self, UiEvent::QuickAddReset(_))
    }
}

/// The native side. Every method returns at once or after a short native call; none is called
/// with the store lock held.
pub trait Shell: Send + Sync {
    /// Sends an event to the web views that listen for it.
    fn emit(&self, event: UiEvent);
    /// Shows and focuses the main window.
    fn show_main(&self);
    /// Hides the quick-add panel (the caller sends `quickadd:reset`).
    fn hide_quickadd(&self);
    /// Sets the quick-add panel's height to follow its content.
    fn resize_quickadd(&self, height: f64);
    /// Opens a URL the allowlist has already passed (`https` or `t3code`) with `app`, the
    /// handler whose signature was checked (`open -a app -- url`).
    fn open_url(&self, url: &str, app: &std::path::Path) -> Result<(), String>;
    /// Opens a document the allowlist has already passed, by its resolved path, in `app`
    /// (`open -a app -- path`), never in the app LaunchServices would pick.
    fn open_file(&self, path: &std::path::Path, app: &std::path::Path) -> Result<(), String>;
    /// Shows a file or folder in Finder, without opening it.
    fn reveal(&self, path: &std::path::Path) -> Result<(), String>;
    /// Open thread on a Herdr terminal: `open -b dev.herdr.app`, then `herdr-app do
    /// agent.focus '{"terminal":…}'` (`herdr::show_terminal`, which waits for a Herdr that was
    /// quit). An app constant, not a stored link, so no allowlist.
    fn show_herdr_terminal(&self, terminal: &str) -> Result<(), String>;
    /// The folders Herdr knows, for first run.
    fn herdr_folders(&self) -> Arc<dyn HerdrFolders>;
    /// Puts text on the clipboard.
    fn copy_text(&self, text: &str) -> Result<(), String>;
    /// The folder picker; `None` when cancelled.
    fn pick_folder(&self) -> Option<PathBuf>;
    /// A save dialog offering `name`; `None` when cancelled. It asks before replacing a file.
    fn pick_save(&self, name: &str) -> Option<PathBuf>;
    /// Turns open at login on or off.
    fn set_autostart(&self, on: bool) -> Result<(), String>;
    /// Swaps the global hotkey; on failure the old one stays registered. `old` is empty at
    /// start-up, when nothing is registered yet.
    fn register_hotkey(&self, old: &str, new: &str) -> Result<(), String>;
    /// Applies System, Dark or Light to both windows and their background colours.
    fn apply_appearance(&self, appearance: Appearance);
    /// Enables, disables and relabels the native menus.
    fn update_menus(&self, state: &MenuState);
    /// Redraws the menu-bar item's count and menu.
    fn update_tray(&self, needs: &NeedsList);
    /// The system accent as `#rrggbb`, or `None` for Multicolor.
    fn accent(&self) -> Option<String>;
    /// The bundled `airboss` CLI, when running from the app bundle.
    fn sidecar(&self) -> Option<PathBuf>;
    /// macOS notifications.
    fn notifications(&self) -> Arc<dyn Notifications>;
    /// Which app opens a URL, and whether its signature meets a requirement.
    fn app_check(&self) -> Arc<dyn AppCheck>;
    /// Finds and installs a new Airboss (SPEC-updates).
    fn updater(&self) -> Arc<dyn Updater>;
    /// After an update is in place: saves the windows, stops the app as quit does, releases
    /// the single-instance lock and opens the new app without `--hidden`. Returns at once;
    /// the process exits soon after.
    fn restart(&self);
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A shell for tests and the fixtures recorder: records every call and event.
#[derive(Debug, Default)]
pub struct RecordingShell {
    events: Mutex<Vec<UiEvent>>,
    calls: Mutex<Vec<String>>,
    arrived: Condvar,
    folder: Mutex<Option<PathBuf>>,
    save: Mutex<Option<PathBuf>>,
    refuse_hotkey: Mutex<bool>,
    sidecar: Mutex<Option<PathBuf>>,
    accent: Mutex<Option<String>>,
    notifications: Arc<RecordingNotifications>,
    app_check: Arc<FakeAppCheck>,
    herdr_fails: Mutex<bool>,
    herdr_folders: Arc<FakeHerdrFolders>,
    updater: Arc<FakeUpdater>,
}

impl RecordingShell {
    /// Records everything, answers nothing special.
    pub fn new() -> RecordingShell {
        RecordingShell::default()
    }

    /// Every event so far.
    pub fn events(&self) -> Vec<UiEvent> {
        lock(&self.events).clone()
    }

    /// Takes the events so far, leaving none.
    pub fn take_events(&self) -> Vec<UiEvent> {
        std::mem::take(&mut *lock(&self.events))
    }

    /// Every other call so far, e.g. `"open_url(https://…)"`.
    pub fn calls(&self) -> Vec<String> {
        lock(&self.calls).clone()
    }

    /// Waits until an event matching `f` has arrived (or had already), up to `timeout`.
    pub fn wait_for(&self, timeout: Duration, f: impl Fn(&UiEvent) -> bool) -> Option<UiEvent> {
        let deadline = Instant::now() + timeout;
        let mut events = lock(&self.events);
        loop {
            if let Some(e) = events.iter().find(|e| f(e)) {
                return Some(e.clone());
            }
            let left = deadline.checked_duration_since(Instant::now())?;
            events = self
                .arrived
                .wait_timeout(events, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// What `pick_folder` answers.
    pub fn answer_folder(&self, folder: Option<PathBuf>) {
        *lock(&self.folder) = folder;
    }

    /// What `pick_save` answers.
    pub fn answer_save(&self, to: Option<PathBuf>) {
        *lock(&self.save) = to;
    }

    /// Makes `register_hotkey` fail, as when another app holds the key.
    pub fn refuse_hotkeys(&self, refuse: bool) {
        *lock(&self.refuse_hotkey) = refuse;
    }

    /// What `sidecar` answers.
    pub fn set_sidecar(&self, path: Option<PathBuf>) {
        *lock(&self.sidecar) = path;
    }

    /// The notifications fake: what was posted, and what permission answers.
    pub fn notifications_fake(&self) -> Arc<RecordingNotifications> {
        self.notifications.clone()
    }

    /// The handler and signature fake: genuine T3 Code and Safari until a test says otherwise.
    pub fn app_check_fake(&self) -> Arc<FakeAppCheck> {
        self.app_check.clone()
    }

    /// Makes `show_herdr_terminal` fail, as when Herdr isn't open or the terminal is gone.
    pub fn fail_herdr(&self, fail: bool) {
        *lock(&self.herdr_fails) = fail;
    }

    /// The Herdr folders fake: no Herdr until a test says otherwise.
    pub fn herdr_folders_fake(&self) -> Arc<FakeHerdrFolders> {
        self.herdr_folders.clone()
    }

    /// The updater fake: an installed build that's up to date, until a test says otherwise.
    pub fn updater_fake(&self) -> Arc<FakeUpdater> {
        self.updater.clone()
    }

    /// What `accent` answers.
    pub fn set_accent(&self, accent: Option<String>) {
        *lock(&self.accent) = accent;
    }

    fn call(&self, c: String) {
        lock(&self.calls).push(c);
    }
}

impl Shell for RecordingShell {
    fn emit(&self, event: UiEvent) {
        lock(&self.events).push(event);
        self.arrived.notify_all();
    }
    fn show_main(&self) {
        self.call("show_main".into());
    }
    fn hide_quickadd(&self) {
        self.call("hide_quickadd".into());
    }
    fn resize_quickadd(&self, height: f64) {
        self.call(format!("resize_quickadd({height})"));
    }
    fn open_url(&self, url: &str, app: &std::path::Path) -> Result<(), String> {
        self.call(format!("open_url({url} with {})", app.display()));
        Ok(())
    }
    fn open_file(&self, path: &std::path::Path, app: &std::path::Path) -> Result<(), String> {
        self.call(format!(
            "open_file({} in {})",
            path.display(),
            app.display()
        ));
        Ok(())
    }
    fn reveal(&self, path: &std::path::Path) -> Result<(), String> {
        self.call(format!("reveal({})", path.display()));
        Ok(())
    }
    fn show_herdr_terminal(&self, terminal: &str) -> Result<(), String> {
        // The commands it would run, as their argv.
        let args = crate::herdr::show_args(terminal).join(" ");
        self.call(format!(
            "show_herdr_terminal(open -b {}; herdr-app {args})",
            crate::herdr::BUNDLE_ID
        ));
        if *lock(&self.herdr_fails) {
            Err("herdr-app exited with Some(1): {\"error\":\"no such terminal\"}".into())
        } else {
            Ok(())
        }
    }
    fn herdr_folders(&self) -> Arc<dyn HerdrFolders> {
        self.herdr_folders.clone()
    }
    fn copy_text(&self, text: &str) -> Result<(), String> {
        self.call(format!("copy_text({text})"));
        Ok(())
    }
    fn pick_folder(&self) -> Option<PathBuf> {
        self.call("pick_folder".into());
        lock(&self.folder).clone()
    }
    fn pick_save(&self, name: &str) -> Option<PathBuf> {
        self.call(format!("pick_save({name})"));
        lock(&self.save).clone()
    }
    fn set_autostart(&self, on: bool) -> Result<(), String> {
        self.call(format!("set_autostart({on})"));
        Ok(())
    }
    fn register_hotkey(&self, old: &str, new: &str) -> Result<(), String> {
        self.call(format!("register_hotkey({old} -> {new})"));
        if *lock(&self.refuse_hotkey) {
            Err("in use".into())
        } else {
            Ok(())
        }
    }
    fn apply_appearance(&self, appearance: Appearance) {
        self.call(format!("apply_appearance({appearance:?})"));
    }
    fn update_menus(&self, state: &MenuState) {
        self.call(format!("update_menus({state:?})"));
    }
    fn update_tray(&self, needs: &NeedsList) {
        self.call(format!("update_tray({})", needs.count));
    }
    fn accent(&self) -> Option<String> {
        lock(&self.accent).clone()
    }
    fn sidecar(&self) -> Option<PathBuf> {
        lock(&self.sidecar).clone()
    }
    fn notifications(&self) -> Arc<dyn Notifications> {
        self.notifications.clone()
    }
    fn app_check(&self) -> Arc<dyn AppCheck> {
        self.app_check.clone()
    }
    fn updater(&self) -> Arc<dyn Updater> {
        self.updater.clone()
    }
    fn restart(&self) {
        self.call("restart".into());
    }
}
