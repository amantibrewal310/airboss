//! The quick-add panel: a non-activating `NSPanel` over any app, on every Space and over
//! full-screen apps, shown by the global hotkey.
//!
//! It's created hidden at start-up and reset whenever it hides, so showing it does no I/O and
//! no rendering: only a position and an order-front.
//!
//! The window is transparent and exactly as tall as what it shows: the card, and while a picker
//! is open, the picker hanging below it (so the empty part never takes clicks meant for the app
//! behind for longer than that). Its elevation is AppKit's own window shadow, which follows the
//! card's painted pixels in both appearances; it's recomputed whenever the content changes.

use std::time::Instant;

use objc2_app_kit::NSWindowStyleMask;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use tauri::{AppHandle, Manager, PhysicalPosition, Wry};
use tauri_nspanel::{CollectionBehavior, ManagerExt, PanelLevel, WebviewWindowExt};
use tauri_plugin_log::log;

use crate::dto::QuickaddReset;
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
/// The least height it's given, whatever the UI asks for.
pub const MIN_HEIGHT: f64 = 80.0;

/// A window frame in AppKit's coordinates: points, with y growing upwards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The panel's frame at a new height: the top edge stays put, and the bottom stops at `floor`
/// (the bottom of the screen's visible area, above the Dock) when there is one.
pub fn fit_height(frame: Frame, height: f64, floor: Option<f64>) -> Frame {
    let top = frame.y + frame.height;
    let mut h = if height.is_finite() {
        height.round()
    } else {
        frame.height
    };
    if let Some(floor) = floor {
        h = h.min(top - floor);
    }
    let h = h.max(MIN_HEIGHT);
    Frame {
        height: h,
        y: top - h,
        ..frame
    }
}

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
    panel.set_has_shadow(true);
    Ok(())
}

/// Sets the panel's height (logical points), keeping its top edge where it is, then has AppKit
/// redraw the shadow around what's painted now. The UI calls this again once the new content
/// has painted, so the shadow ends up matching it. Main thread only.
pub fn resize(app: &AppHandle<Wry>, height: f64) {
    match app.get_webview_panel(LABEL) {
        Ok(panel) => {
            let ns = panel.as_panel();
            let f = ns.frame();
            let now = Frame {
                x: f.origin.x,
                y: f.origin.y,
                width: f.size.width,
                height: f.size.height,
            };
            let floor = ns.screen().map(|s| s.visibleFrame().origin.y);
            let next = fit_height(now, height, floor);
            if next != now {
                ns.setFrame_display(
                    NSRect::new(
                        NSPoint::new(next.x, next.y),
                        NSSize::new(next.width, next.height),
                    ),
                    true,
                );
            }
            ns.invalidateShadow();
        }
        Err(_) => {
            if let Some(w) = app.get_webview_window(LABEL) {
                let _ = w.set_size(tauri::LogicalSize::new(WIDTH, height.max(MIN_HEIGHT)));
            }
        }
    }
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
        Ok(panel) => {
            panel.show_and_make_key();
            panel.as_panel().invalidateShadow();
        }
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
        UiEvent::QuickAddReset(QuickaddReset {
            hotkey,
            keep_text: Some(keep_text),
        }),
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

#[cfg(test)]
mod tests {
    use super::*;

    const PANEL: Frame = Frame {
        x: 940.0,
        y: 900.0,
        width: WIDTH,
        height: 221.0,
    };

    #[test]
    fn growing_keeps_the_top_edge() {
        let f = fit_height(PANEL, 520.0, Some(0.0));
        assert_eq!(f.height, 520.0);
        assert_eq!(f.y + f.height, PANEL.y + PANEL.height);
        assert_eq!((f.x, f.width), (PANEL.x, PANEL.width));
        // And shrinking back gives the same frame as before.
        assert_eq!(fit_height(f, 221.0, Some(0.0)), PANEL);
    }

    #[test]
    fn the_bottom_stops_above_the_dock() {
        let f = fit_height(PANEL, 2000.0, Some(80.0));
        assert_eq!(f.y, 80.0);
        assert_eq!(f.y + f.height, PANEL.y + PANEL.height);
        // With no screen known, the height is what was asked.
        assert_eq!(fit_height(PANEL, 2000.0, None).height, 2000.0);
    }

    #[test]
    fn odd_heights_are_rounded_or_ignored() {
        assert_eq!(fit_height(PANEL, 300.4, None).height, 300.0);
        assert_eq!(fit_height(PANEL, f64::NAN, None), PANEL);
        assert_eq!(fit_height(PANEL, 3.0, None).height, MIN_HEIGHT);
    }
}
