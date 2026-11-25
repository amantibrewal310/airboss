//! The quick-add panel: a non-activating `NSPanel` over any app, on every Space and over
//! full-screen apps, shown by the global hotkey.
//!
//! It's created hidden at start-up and reset whenever it hides, so showing it does no I/O and
//! no rendering: only a position and an order-front.

use std::time::Instant;

use objc2_app_kit::NSWindowStyleMask;
use tauri::{AppHandle, Manager, PhysicalPosition, Wry};
use tauri_nspanel::{CollectionBehavior, ManagerExt, PanelLevel, WebviewWindowExt};
use tauri_plugin_log::log;

use crate::dto::QuickAddReset;
use crate::ipc::App;
use crate::macos::QuickAddPanel;
use crate::settings::hotkey_label;
use crate::shell::UiEvent;
use crate::state::lock;

/// The panel's window label.
pub const LABEL: &str = "quickadd";
/// Its width; its height follows the content.
pub const WIDTH: f64 = 680.0;
/// How far down the screen its top sits.
pub const FROM_TOP: f64 = 0.22;

/// Turns the `quickadd` window into the panel. Call once, on the main thread, at start-up.
pub fn setup(app: &AppHandle<Wry>) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window(LABEL) else {
        log::error!("no quickadd window");
        return Ok(());
    };
    let panel = window.to_panel::<QuickAddPanel<Wry>>()?;
    panel.set_level(PanelLevel::Floating.value());
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .full_screen_auxiliary()
            .ignores_cycle()
            .value(),
    );
    if let Err(e) = panel.add_style_mask(NSWindowStyleMask::NonactivatingPanel) {
        log::warn!("quick add can't be a non-activating panel: {e}");
    }
    panel.set_hides_on_deactivate(false);
    panel.set_opaque(false);
    panel.set_has_shadow(false);
    Ok(())
}

/// Whether the panel is showing.
pub fn is_visible(app: &AppHandle<Wry>) -> bool {
    app.get_webview_panel(LABEL)
        .map(|p| p.is_visible())
        .unwrap_or(false)
}

/// Whether the panel is the key window (it has the keyboard).
pub fn is_key(app: &AppHandle<Wry>) -> bool {
    app.get_webview_window(LABEL)
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false)
}

/// Puts the panel 22% from the top of the screen with the mouse, centred, then shows and
/// focuses it. Main thread only.
pub fn show(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let monitor = app
            .cursor_position()
            .ok()
            .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
            .or_else(|| app.primary_monitor().ok().flatten());
        if let Some(m) = monitor {
            let (pos, size, scale) = (m.position(), m.size(), m.scale_factor());
            let x = pos.x as f64 + (size.width as f64 - WIDTH * scale) / 2.0;
            let y = pos.y as f64 + size.height as f64 * FROM_TOP;
            let _ = window.set_position(PhysicalPosition::new(x.round(), y.round()));
        }
    }
    match app.get_webview_panel(LABEL) {
        Ok(panel) => panel.show_and_make_key(),
        Err(_) => {
            // Not a panel (setup failed): an always-on-top window is the fallback.
            if let Some(w) = app.get_webview_window(LABEL) {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
    }
}

/// Hides the panel. Main thread only.
pub fn hide(app: &AppHandle<Wry>) {
    match app.get_webview_panel(LABEL) {
        Ok(panel) => panel.hide(),
        Err(_) => {
            if let Some(w) = app.get_webview_window(LABEL) {
                let _ = w.hide();
            }
        }
    }
}

/// Hides it and tells the UI to reset (keeping a half-typed title if it lost focus to a
/// click elsewhere).
pub fn hide_and_reset(app: &AppHandle<Wry>, keep_text: bool) {
    hide(app);
    let hotkey = app
        .try_state::<App>()
        .and_then(|a| a.current())
        .map(|s| hotkey_label(&s.settings().hotkey))
        .unwrap_or_default();
    crate::native::emit(
        app,
        UiEvent::QuickAddReset(QuickAddReset { hotkey, keep_text }),
    );
}

/// The hotkey: shows the panel, or hides it if it's already the key window.
pub fn toggle(app: &AppHandle<Wry>) {
    if is_visible(app) && is_key(app) {
        hide_and_reset(app, false);
        return;
    }
    if let Some(state) = app.try_state::<App>().and_then(|a| a.current()) {
        lock(&state.timing).quickadd_pressed = Some(Instant::now());
    }
    show(app);
}
