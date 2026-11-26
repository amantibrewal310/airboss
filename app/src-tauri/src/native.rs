//! The native side: `TauriShell`, start-up, and the window, menu and run-loop handlers.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use airboss_core::Store;
use airboss_services::{NeedsKey, NeedsList, SystemClock};
use tauri::menu::MenuEvent;
use tauri::tray::TrayIcon;
use tauri::window::Color;
use tauri::{AppHandle, Emitter, Manager, RunEvent, Runtime, Theme, Window, WindowEvent, Wry};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tauri_plugin_log::log;
use tauri_plugin_opener::OpenerExt;

use crate::catchup::Trigger;
use crate::dto::{
    AppearanceChanged, CopyWhat, MenuAction, NavOpen, Theme as ThemeDto, ToastEvent, ToastTone,
};
use crate::errors::{AppError, AppResult, ErrorCode};
use crate::ipc::App;
use crate::menu::{self, MenuHandles, MenuState, ids};
use crate::notify::{Notice, Notifications, Permission};
use crate::services::Services;
use crate::settings::{self, Appearance};
use crate::shell::{Shell, UiEvent};
use crate::state::{AppConfig, AppState, Caller, lock, share};
use crate::util::Debounced;
use crate::{macos, quickadd, tray};

/// The main window's label.
pub const MAIN: &str = "main";

/// Sends an event to the window that listens for it.
pub fn emit(app: &AppHandle<Wry>, event: UiEvent) {
    crate::trace::event(&event);
    let (name, payload) = (event.name(), event.payload());
    let sent = match event {
        UiEvent::QuickAddReset(_) => app.emit_to(quickadd::LABEL, name, payload),
        UiEvent::AppearanceChanged(_) => app.emit(name, payload),
        _ => app.emit_to(MAIN, name, payload),
    };
    if let Err(e) = sent {
        log::warn!("couldn't send {name}: {e}");
    }
}

/// The background colour behind a window's web view, so no white shows before the first
/// paint: the theme's `--bg`.
fn background(theme: ThemeDto) -> Color {
    match theme {
        ThemeDto::Dark => Color(0x0f, 0x10, 0x12, 0xff),
        ThemeDto::Light => Color(0xff, 0xff, 0xff, 0xff),
    }
}

fn effective_theme(app: &AppHandle<Wry>, appearance: Appearance) -> ThemeDto {
    match appearance {
        Appearance::Dark => ThemeDto::Dark,
        Appearance::Light => ThemeDto::Light,
        Appearance::System => match app.get_webview_window(MAIN).and_then(|w| w.theme().ok()) {
            Some(Theme::Light) => ThemeDto::Light,
            _ => ThemeDto::Dark,
        },
    }
}

/// Notifications through `UNUserNotificationCenter` (`macos.rs`): permission, posting, and a
/// delegate that opens the main window on the clicked item. Each notification's identifier is
/// its Needs me key, so a click names the item, and posting the same key again replaces the
/// notification rather than adding one. Only with a bundle identifier (the built app).
pub struct TauriNotifications {
    app: AppHandle<Wry>,
}

impl TauriNotifications {
    /// Called from `setup`, on the main thread and before anything is posted, so the delegate
    /// is in place for every click, including one that launched the app.
    fn new(app: AppHandle<Wry>) -> Arc<TauriNotifications> {
        let clicked = app.clone();
        macos::set_notification_delegate(macos::Clicked(Box::new(move |key: String| {
            let app = clicked.clone();
            let _ = clicked.run_on_main_thread(move || {
                show_main_window(&app);
                emit(
                    &app,
                    UiEvent::NavOpen(NavOpen {
                        view: Some("needs".into()),
                        task: None,
                        needs_key: Some(key),
                    }),
                );
            });
        })));
        Arc::new(TauriNotifications { app })
    }
}

impl Notifications for TauriNotifications {
    fn post(&self, n: &Notice) -> Result<(), String> {
        macos::post_notification(&n.key.0, &n.title, &n.body, &n.group)
    }

    fn withdraw(&self, keys: &[NeedsKey]) -> Result<(), String> {
        let ids: Vec<&str> = keys.iter().map(|k| k.0.as_str()).collect();
        macos::withdraw_notifications(&ids);
        Ok(())
    }

    fn permission(&self) -> Permission {
        match macos::notify_permission(Duration::from_secs(2)) {
            Some(macos::NotifyAuth::Granted) => Permission::Granted,
            Some(macos::NotifyAuth::Denied) => Permission::Denied,
            Some(macos::NotifyAuth::NotAsked) => Permission::NotAsked,
            None => Permission::Unknown,
        }
    }

    fn request(&self) {
        macos::request_notify_permission();
    }

    fn open_settings(&self) -> Result<(), String> {
        // An app constant, not a link anyone stored: it doesn't go through the link allowlist.
        let url = format!(
            "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id={}",
            self.app.config().identifier
        );
        self.app
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|e| e.to_string())
    }
}

/// The app's side of [`Shell`].
pub struct TauriShell {
    app: AppHandle<Wry>,
    menus: Mutex<Option<MenuHandles<Wry>>>,
    tray: Mutex<Option<TrayIcon<Wry>>>,
    needs: Mutex<Arc<NeedsList>>,
    tray_rebuild: Mutex<Option<Debounced>>,
    accent: Mutex<Option<String>>,
    hotkey: Mutex<String>,
    notifications: Arc<TauriNotifications>,
}

impl TauriShell {
    fn new(app: AppHandle<Wry>, hotkey: String) -> Arc<TauriShell> {
        let shell = Arc::new(TauriShell {
            notifications: TauriNotifications::new(app.clone()),
            app,
            menus: Mutex::new(None),
            tray: Mutex::new(None),
            needs: Mutex::new(Arc::default()),
            tray_rebuild: Mutex::new(None),
            accent: Mutex::new(None),
            hotkey: Mutex::new(hotkey),
        });
        let weak = Arc::downgrade(&shell);
        *lock(&shell.tray_rebuild) = Debounced::new("tray", tray::REBUILD_DELAY, move || {
            if let Some(s) = weak.upgrade() {
                s.rebuild_tray();
            }
        });
        shell
    }

    fn on_main(&self, f: impl FnOnce(&AppHandle<Wry>) + Send + 'static) {
        let app = self.app.clone();
        if let Err(e) = self.app.run_on_main_thread(move || f(&app)) {
            log::warn!("couldn't run on the main thread: {e}");
        }
    }

    fn rebuild_tray(self: &Arc<Self>) {
        let me = self.clone();
        self.on_main(move |app| {
            let needs = lock(&me.needs).clone();
            let now = SystemTime::now();
            let secs = now
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            let model = tray::model(&needs, now, macos::local_offset(secs));
            crate::trace::tray(&model);
            // The accelerator itself: the menu shows it as "⌃⌥N".
            let hotkey = lock(&me.hotkey).clone();
            let mut tray = lock(&me.tray);
            let result = match tray.as_ref() {
                Some(t) => tray::update(app, t, &model, &hotkey),
                None => tray::create(app, &model, &hotkey).map(|t| {
                    *tray = Some(t);
                }),
            };
            if let Err(e) = result {
                log::warn!("couldn't draw the menu-bar item: {e}");
            }
        });
    }

    fn set_backgrounds(&self, theme: ThemeDto) {
        self.on_main(move |app| {
            for label in [MAIN, quickadd::LABEL] {
                if let Some(w) = app.get_webview_window(label) {
                    // Quick add is transparent: its card paints the colour.
                    let color = if label == MAIN {
                        Some(background(theme))
                    } else {
                        Some(Color(0, 0, 0, 0))
                    };
                    let _ = w.set_background_color(color);
                }
            }
        });
    }
}

impl Shell for Arc<TauriShell> {
    fn emit(&self, event: UiEvent) {
        emit(&self.app, event);
    }

    fn show_main(&self) {
        self.on_main(show_main_window);
    }

    fn hide_quickadd(&self) {
        self.on_main(quickadd::hide);
    }

    fn resize_quickadd(&self, height: f64) {
        self.on_main(move |app| quickadd::resize(app, height));
    }

    fn open_url(&self, url: &str, app: &Path) -> Result<(), String> {
        // `open -a <app> -- <url>`: exactly the app whose signature was just checked, never
        // whatever LaunchServices would pick by then.
        self.app
            .opener()
            .open_url(url, Some(app.to_string_lossy().into_owned()))
            .map_err(|e| e.to_string())
    }

    fn open_file(&self, path: &Path, app: &Path) -> Result<(), String> {
        // `open -a <app> -- <path>`: the allowlist checked `path` as UTF-8, so nothing changes
        // on the way.
        let path = path.to_str().ok_or("the path isn't UTF-8")?;
        self.app
            .opener()
            .open_path(path, Some(app.to_string_lossy().into_owned()))
            .map_err(|e| e.to_string())
    }

    fn reveal(&self, path: &Path) -> Result<(), String> {
        self.app
            .opener()
            .reveal_item_in_dir(path)
            .map_err(|e| e.to_string())
    }

    fn show_herdr_terminal(&self, terminal: &str) -> Result<(), String> {
        crate::herdr::show_terminal(&crate::services::home(), terminal)
    }

    fn herdr_folders(&self) -> Arc<dyn crate::herdr::HerdrFolders> {
        Arc::new(crate::herdr::RealHerdrFolders {
            home: crate::services::home(),
        })
    }

    fn copy_text(&self, text: &str) -> Result<(), String> {
        self.app
            .clipboard()
            .write_text(text.to_string())
            .map_err(|e| e.to_string())
    }

    fn pick_folder(&self) -> Option<PathBuf> {
        self.app
            .dialog()
            .file()
            .set_title("Add a repo")
            .blocking_pick_folder()
            .and_then(|p| p.into_path().ok())
    }

    fn pick_save(&self, name: &str) -> Option<PathBuf> {
        self.app
            .dialog()
            .file()
            .set_title("Back up the Airboss database")
            .set_file_name(name)
            .add_filter("SQLite database", &["db"])
            .blocking_save_file()
            .and_then(|p| p.into_path().ok())
    }

    fn set_autostart(&self, on: bool) -> Result<(), String> {
        let launcher = self.app.autolaunch();
        if on {
            launcher.enable()
        } else {
            launcher.disable()
        }
        .map_err(|e| e.to_string())
    }

    fn register_hotkey(&self, old: &str, new: &str) -> Result<(), String> {
        let gs = self.app.global_shortcut();
        // `old` is empty at start-up, when nothing is registered yet.
        if !old.is_empty() {
            let _ = gs.unregister(old);
        }
        match gs.register(new) {
            Ok(()) => {
                *lock(&self.hotkey) = new.to_string();
                // The menu-bar menu shows the hotkey on Quick Add.
                if let Some(d) = lock(&self.tray_rebuild).as_ref() {
                    d.poke();
                }
                Ok(())
            }
            Err(e) => {
                if !old.is_empty()
                    && let Err(e) = gs.register(old)
                {
                    log::warn!("couldn't restore the hotkey {old}: {e}");
                }
                Err(e.to_string())
            }
        }
    }

    fn apply_appearance(&self, appearance: Appearance) {
        let theme = match appearance {
            Appearance::System => None,
            Appearance::Dark => Some(Theme::Dark),
            Appearance::Light => Some(Theme::Light),
        };
        let app = self.app.clone();
        let me = self.clone();
        self.on_main(move |handle| {
            handle.set_theme(theme);
            let effective = effective_theme(&app, appearance);
            me.set_backgrounds(effective);
            emit(
                &app,
                UiEvent::AppearanceChanged(AppearanceChanged {
                    accent: lock(&me.accent).clone(),
                    theme: effective,
                }),
            );
        });
    }

    fn update_menus(&self, state: &MenuState) {
        let state = state.clone();
        let me = self.clone();
        self.on_main(move |_| {
            if let Some(m) = lock(&me.menus).as_ref() {
                m.apply(&state);
            }
        });
    }

    fn update_tray(&self, needs: &NeedsList) {
        *lock(&self.needs) = Arc::new(needs.clone());
        if let Some(d) = lock(&self.tray_rebuild).as_ref() {
            d.poke();
        }
    }

    fn accent(&self) -> Option<String> {
        lock(&self.accent).clone()
    }

    fn notifications(&self) -> Arc<dyn Notifications> {
        self.notifications.clone()
    }

    fn app_check(&self) -> Arc<dyn crate::handlers::AppCheck> {
        Arc::new(crate::handlers::MacAppCheck)
    }

    fn sidecar(&self) -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let dir = exe.parent()?;
        let inside_bundle = dir.ends_with("Contents/MacOS");
        let cli = dir.join("airboss");
        (inside_bundle && cli.exists()).then_some(cli)
    }
}

/// Shows and focuses the main window (from the Dock, the menu bar or a second launch).
pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    if let Some(s) = app.try_state::<App>().and_then(|a| a.current()) {
        s.main_shown.store(true, Ordering::SeqCst);
    }
}

/// `AIRBOSS_DB`, when it names another database (as core reads it: blank doesn't count).
fn db_override() -> Option<PathBuf> {
    std::env::var_os(airboss_core::DB_VAR)
        .filter(|p| !p.to_string_lossy().trim().is_empty())
        .map(PathBuf::from)
}

/// Where the database lives, worked out once in core for the CLI and the app: `AIRBOSS_DB`,
/// or `~/Library/Application Support/Airboss/airboss.db`. With neither `AIRBOSS_DB` nor
/// `HOME` it's the "can't open its database" state with core's reason, never a relative path.
pub fn db_location(found: airboss_core::Result<PathBuf>) -> AppResult<PathBuf> {
    found.map_err(|e| {
        AppError::new(ErrorCode::DbUnavailable, "Airboss can't open its database")
            .with_detail(e.to_string())
    })
}

/// [`db_location`] for this process.
pub fn db_path() -> AppResult<PathBuf> {
    db_location(airboss_core::default_db_path())
}

/// Why the database at `db` didn't open, as the full-window state shows it: "Update Airboss"
/// for one a newer Airboss migrated, otherwise "Airboss can't open its database".
pub fn open_error(db: &Path, e: airboss_core::Error) -> AppError {
    let detail = format!("{}: {e}", db.display());
    if e.is_newer_database() {
        AppError::db_too_new(&e).with_detail(detail)
    } else {
        AppError::new(ErrorCode::DbUnavailable, "Airboss can't open its database")
            .with_detail(detail)
    }
}

/// Whether this process runs from an installed `.app` bundle.
pub fn in_bundle() -> bool {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().contains(".app/Contents/MacOS/"))
        .unwrap_or(false)
}

fn open_state(db: &Path, shell: Arc<TauriShell>, config: AppConfig) -> AppResult<Arc<AppState>> {
    if let Some(dir) = db.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let store = Store::open(db).map_err(|e| open_error(db, e))?;
    let store = share(store);
    let services = Services::start(&store, &crate::services::home());
    let state = AppState::new(
        store,
        services,
        Arc::new(shell),
        Arc::new(SystemClock),
        config,
    )?;
    state.start_background();
    Ok(state)
}

/// Builds everything at start-up: settings, the state, menus, the menu-bar item, the quick-add
/// panel, the hotkey, the appearance and the observers. The main window shows on `ui_ready`,
/// or after 2 s at most, unless started with `--hidden`.
pub fn setup(
    app: &mut tauri::App<Wry>,
    hidden: bool,
    started: std::time::Instant,
) -> tauri::Result<()> {
    let handle = app.handle().clone();
    // No place for the database (no HOME and no AIRBOSS_DB) is the full-window state with
    // core's reason; the settings then aren't saved anywhere.
    let db = db_path();
    let settings_path = db.as_deref().ok().map(settings::path_for);
    let (mut settings, first_run) = settings_path
        .as_deref()
        .map_or((settings::Settings::default(), false), settings::load);

    let shell = TauriShell::new(handle.clone(), settings.hotkey.clone());
    *lock(&shell.accent) = macos::accent_hex();

    // Open at login on first run, for the installed app on its own database only.
    if first_run {
        settings.open_at_login =
            settings::open_at_login_on_first_run(in_bundle(), db_override().is_some());
        if settings.open_at_login
            && let Err(e) = Shell::set_autostart(&shell, true)
        {
            log::warn!("couldn't turn on open at login: {e}");
        }
        if let Some(path) = &settings_path
            && let Err(e) = settings::save(path, &settings)
        {
            log::warn!("couldn't save settings: {e}");
        }
        // Alongside open at login: macOS asks once whether Airboss may notify.
        if settings.notifications {
            shell.notifications.request();
        }
    }

    let mut config = AppConfig::new(settings.clone(), settings_path);
    config.start_hidden = hidden;
    config.started = started;
    let opened = db
        .clone()
        .and_then(|db| open_state(&db, shell.clone(), config.clone()));
    let app_state = match opened {
        Ok(state) => App::ready(state),
        Err(e) => {
            log::error!("{}: {}", e.message, e.detail.as_deref().unwrap_or(""));
            let shell = shell.clone();
            App::new(move || {
                db.clone()
                    .and_then(|db| open_state(&db, shell.clone(), config.clone()))
            })
        }
    };
    handle.manage(app_state.clone());

    // Menus.
    let (menu, handles) = menu::build(&handle, &settings.hotkey)?;
    handle.set_menu(menu)?;
    *lock(&shell.menus) = Some(handles);
    if let Some(s) = app_state.current() {
        s.refresh_menus();
    }

    // The menu-bar item, with the current Needs me list.
    if let Some(s) = app_state.current() {
        *lock(&shell.needs) = s.vm(|vm| vm.needs.clone());
    }
    shell.rebuild_tray();

    // Quick add as a panel.
    if let Err(e) = quickadd::setup(&handle) {
        log::error!("quick add isn't a panel: {e}");
    }

    // The hotkey; a clash shows in the connections footer, and Settings picks another.
    match app_state.current() {
        Some(s) => s.register_hotkey_at_start(),
        None => {
            if let Err(e) = handle.global_shortcut().register(settings.hotkey.as_str()) {
                log::warn!("the hotkey {} is taken: {e}", settings.hotkey);
            }
        }
    }

    // Appearance: the window theme and background before anything paints.
    Shell::apply_appearance(&shell, settings.appearance);

    // Wake and accent changes.
    let wake_app = handle.clone();
    let color_app = handle.clone();
    let color_shell = shell.clone();
    macos::observe(macos::Callbacks {
        woke: Box::new(move || {
            if let Some(s) = wake_app.try_state::<App>().and_then(|a| a.current()) {
                s.trigger(Trigger::Wake);
            }
        }),
        colors_changed: Box::new(move || {
            let accent = macos::accent_hex();
            *lock(&color_shell.accent) = accent.clone();
            let appearance = color_app
                .try_state::<App>()
                .and_then(|a| a.current())
                .map(|s| s.settings().appearance)
                .unwrap_or_default();
            emit(
                &color_app,
                UiEvent::AppearanceChanged(AppearanceChanged {
                    accent,
                    theme: effective_theme(&color_app, appearance),
                }),
            );
        }),
    });

    // The backstop: a UI that never says it's ready still gets a window.
    if !hidden {
        let backstop = handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            let shown = backstop
                .try_state::<App>()
                .and_then(|a| a.current())
                .is_some_and(|s| s.main_shown.load(Ordering::SeqCst));
            if !shown {
                log::warn!("ui_ready didn't arrive within 2 s; showing the main window anyway");
                let app = backstop.clone();
                let _ = backstop.run_on_main_thread(move || show_main_window(&app));
            }
        });
    }
    Ok(())
}

/// A second launch: `--quick-add` opens quick add; anything else shows the main window. The
/// single-instance plugin calls this from its own thread, so the work moves to the main one.
pub fn second_launch(app: &AppHandle<Wry>, args: &[String]) {
    if crate::trace::simulated_wake(args) {
        if let Some(s) = app.try_state::<App>().and_then(|a| a.current()) {
            s.trigger(Trigger::Wake);
        }
        return;
    }
    let quick_add = args.iter().any(|a| a == "--quick-add");
    let handle = app.clone();
    let run = app.run_on_main_thread(move || {
        if quick_add {
            quickadd::toggle(&handle);
        } else {
            show_main_window(&handle);
            emit(&handle, UiEvent::NavOpen(NavOpen::default()));
        }
    });
    if let Err(e) = run {
        log::warn!("couldn't handle a second launch: {e}");
    }
}

/// Window events: the red button hides, focus checks `data_version`, the theme follows
/// macOS, and quick add hides when it loses focus.
pub fn on_window_event(window: &Window<Wry>, event: &WindowEvent) {
    let app = window.app_handle();
    match (window.label(), event) {
        (MAIN, WindowEvent::CloseRequested { api, .. }) => {
            api.prevent_close();
            let _ = window.hide();
        }
        (MAIN, WindowEvent::Focused(focused)) => {
            if let Some(s) = app.try_state::<App>().and_then(|a| a.current()) {
                // No notifications while the window is in front.
                s.set_main_focused(*focused);
                if *focused {
                    s.trigger(Trigger::Focus);
                }
            }
        }
        (MAIN, WindowEvent::ThemeChanged(theme)) => {
            let appearance = app
                .try_state::<App>()
                .and_then(|a| a.current())
                .map(|s| s.settings().appearance)
                .unwrap_or_default();
            if appearance == Appearance::System {
                let t = match theme {
                    Theme::Light => ThemeDto::Light,
                    _ => ThemeDto::Dark,
                };
                if let Some(w) = app.get_webview_window(MAIN) {
                    let _ = w.set_background_color(Some(background(t)));
                }
                emit(
                    app,
                    UiEvent::AppearanceChanged(AppearanceChanged {
                        accent: macos::accent_hex(),
                        theme: t,
                    }),
                );
            }
        }
        (quickadd::LABEL, WindowEvent::Focused(false)) if quickadd::is_visible(app) => {
            quickadd::hide_and_reset(app, true);
        }
        _ => {}
    }
}

fn toast(app: &AppHandle<Wry>, text: &str, tone: ToastTone) {
    emit(
        app,
        UiEvent::Toast(ToastEvent {
            text: text.to_string(),
            actions: Vec::new(),
            undo: None,
            tone: Some(tone),
            task: None,
        }),
    );
}

/// Runs a command body for a menu item on a background thread, toasting its outcome.
fn menu_command(
    app: &AppHandle<Wry>,
    ok: Option<&'static str>,
    f: impl FnOnce(&AppState) -> AppResult<()> + Send + 'static,
) {
    let Some(state) = app.try_state::<App>().and_then(|a| a.current()) else {
        return;
    };
    let app = app.clone();
    std::thread::spawn(move || match crate::ipc::guarded_call(|| f(&state)) {
        Ok(()) => {
            if let Some(ok) = ok {
                toast(&app, ok, ToastTone::Info);
            }
        }
        Err(e) => toast(&app, &e.message, ToastTone::Warning),
    });
}

/// Native menu and menu-bar menu items.
pub fn on_menu_event(app: &AppHandle<Wry>, event: MenuEvent) {
    let id = event.id().as_ref().to_string();
    // The menu-bar menu.
    if let Some(key) = id.strip_prefix(tray::NEEDS_PREFIX) {
        show_main_window(app);
        emit(
            app,
            UiEvent::NavOpen(NavOpen {
                view: Some("needs".into()),
                task: None,
                needs_key: Some(key.to_string()),
            }),
        );
        return;
    }
    match id.as_str() {
        tray::OPEN_ID => {
            show_main_window(app);
            emit(
                app,
                UiEvent::NavOpen(NavOpen {
                    view: Some("needs".into()),
                    ..NavOpen::default()
                }),
            );
            return;
        }
        tray::QUICK_ADD_ID | ids::QUICK_ADD => {
            quickadd::show(app);
            return;
        }
        tray::QUIT_ID => {
            app.exit(0);
            return;
        }
        _ => {}
    }
    // While quick add has the keyboard, only ⌘W (close it) acts.
    if quickadd::is_key(app) {
        if id == ids::CLOSE_WINDOW {
            quickadd::hide_and_reset(app, false);
        }
        return;
    }
    match id.as_str() {
        ids::CLOSE_WINDOW => {
            if let Some(w) = app.get_webview_window(MAIN) {
                let _ = w.hide();
            }
        }
        ids::INSTALL_CLI => menu_command(app, Some("Installed the airboss command."), |s| {
            crate::commands::app::install_cli(s, Caller::Other).map(|_| ())
        }),
        ids::COPY_AGENT_NOTE => menu_command(app, Some("Copied the agent note."), |s| {
            crate::commands::app::copy_text(s, Caller::Other, CopyWhat::AgentNote)
        }),
        ids::COPY_SETUP => menu_command(app, Some("Copied the setup command."), |s| {
            crate::commands::app::copy_text(s, Caller::Other, CopyWhat::SetupCommand)
        }),
        // The browser coming forward with the form says it worked.
        ids::REPORT_PROBLEM => menu_command(app, None, |s| {
            crate::commands::app::report_problem(s, Caller::Other)
        }),
        _ => {
            show_main_window(app);
            emit(app, UiEvent::MenuAction(MenuAction { command: id }));
        }
    }
}

/// The run loop: the Dock icon shows the main window, and quitting shuts down cleanly.
///
/// ⌘Q, the Dock's Quit and logging out arrive as `Exit` only (AppKit's `terminate:`); the
/// menu-bar menu's Quit (`app.exit`) sends `ExitRequested` first. Either shuts down once.
pub fn on_run_event<R: Runtime>(app: &AppHandle<R>, event: RunEvent) {
    match event {
        RunEvent::Reopen { .. } => show_main_window(app),
        RunEvent::ExitRequested { .. } | RunEvent::Exit => {
            if let Some(s) = app.try_state::<App>().and_then(|a| a.current()) {
                s.shutdown(Duration::from_secs(1));
            }
        }
        _ => {}
    }
}
