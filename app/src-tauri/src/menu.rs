//! The native menu bar: its items and key equivalents, and which are enabled for the focus
//! the UI reports (Appendix C).
//!
//! Items whose key is a single letter show it in their title, not as a key equivalent,
//! because a bare-letter key equivalent would swallow typing.

use airboss_core::TaskId;
use serde::{Deserialize, Serialize};
use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Runtime};
use ts_rs::TS;

/// Where keyboard focus is, as the UI reports it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "FocusZone", export_to = "types.ts")]
pub enum Zone {
    #[default]
    List,
    Panel,
    Sidebar,
    Field,
    Overlay,
    Quickadd,
}

/// Which window reports its focus.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "types.ts")]
pub enum WindowName {
    #[default]
    Main,
    Quickadd,
}

/// `set_focus_context`'s argument.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "types.ts")]
pub struct FocusContext {
    pub window: WindowName,
    pub zone: Zone,
    #[ts(as = "Option<crate::dto::TaskIdTs>")]
    pub selection: Option<TaskId>,
    /// e.g. `"needs"`, `"all"`, `"repo:3"`.
    pub view: String,
}

/// What the menus should show now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuState {
    /// Task-menu items may act (not while typing in a field or an overlay is open).
    pub task_items: bool,
    /// …and there's a selected task for the items that need one.
    pub selection: bool,
    /// "Undo <label>", when there's something to undo and the Task menu may act.
    pub undo: Option<String>,
}

/// Appendix C: every Task-menu item is disabled while the zone is `field` or `overlay` (so its
/// key equivalent can't fire through a text field), or while quick add has focus; items that
/// need a selection are disabled without one.
pub fn menu_state(focus: &FocusContext, undo_label: Option<String>) -> MenuState {
    let task_items = focus.window == WindowName::Main
        && !matches!(focus.zone, Zone::Field | Zone::Overlay | Zone::Quickadd);
    MenuState {
        task_items,
        selection: task_items && focus.selection.is_some(),
        undo: if task_items { undo_label } else { None },
    }
}

/// One custom menu item: its id (sent as `menu:action`), title, key equivalent, and whether
/// it needs a selected task.
pub struct Item {
    pub id: &'static str,
    pub title: &'static str,
    pub key: Option<&'static str>,
    pub needs_selection: bool,
}

const fn item(
    id: &'static str,
    title: &'static str,
    key: Option<&'static str>,
    needs_selection: bool,
) -> Item {
    Item {
        id,
        title,
        key,
        needs_selection,
    }
}

/// `None` is a separator.
pub const TASK_MENU: &[Option<Item>] = &[
    Some(item("do_it", "Do It", Some("CmdOrCtrl+Enter"), true)),
    Some(item(
        "plan_it",
        "Plan It",
        Some("Shift+CmdOrCtrl+Enter"),
        true,
    )),
    None,
    Some(item("status", "Change Status…  S", None, true)),
    Some(item("toggle_done", "Mark Done  Space", None, true)),
    Some(item("rename", "Rename  ↩", None, true)),
    Some(item("link_repos", "Link Repos…  R", None, true)),
    Some(item(
        "add_waits_on",
        "Add Waits-On…",
        Some("CmdOrCtrl+D"),
        true,
    )),
    Some(item("add_link", "Add Link…", Some("CmdOrCtrl+L"), true)),
    Some(item(
        "mark_blocked",
        "Mark Blocked…",
        Some("CmdOrCtrl+B"),
        true,
    )),
    Some(item(
        "open_latest",
        "Open Latest Thread or PR",
        Some("CmdOrCtrl+O"),
        true,
    )),
    Some(item("move_under", "Move Under…", None, true)),
    None,
    Some(item("move_up", "Move Up", Some("Alt+CmdOrCtrl+Up"), true)),
    Some(item(
        "move_down",
        "Move Down",
        Some("Alt+CmdOrCtrl+Down"),
        true,
    )),
    Some(item("open_page", "Open Page", Some("CmdOrCtrl+Down"), true)),
    None,
    Some(item(
        "copy_number",
        "Copy Task Number",
        Some("Shift+CmdOrCtrl+C"),
        true,
    )),
    Some(item("drop", "Drop…", Some("CmdOrCtrl+Backspace"), true)),
];

/// The View menu: views (G then a letter), layouts, Jump and the shortcut sheet.
pub const VIEW_MENU: &[Option<Item>] = &[
    Some(item("go_needs", "Needs Me  G N", None, false)),
    Some(item("go_ready", "Ready to Start  G R", None, false)),
    Some(item("go_plans", "Plans  G L", None, false)),
    Some(item("go_progress", "In Progress  G P", None, false)),
    Some(item("go_all", "All Tasks  G A", None, false)),
    None,
    Some(item("layout_tree", "As Tree", None, false)),
    Some(item("layout_status", "By Status", None, false)),
    None,
    Some(item(
        "back",
        "Back to the List",
        Some("CmdOrCtrl+Up"),
        false,
    )),
    Some(item("jump", "Jump to…", Some("CmdOrCtrl+K"), false)),
];

/// The menu item ids Rust handles itself rather than sending to the UI.
pub mod ids {
    pub const SETTINGS: &str = "settings";
    pub const INSTALL_CLI: &str = "install_cli";
    pub const NEW_TASK: &str = "new_task";
    pub const QUICK_ADD: &str = "quick_add";
    pub const CLOSE_WINDOW: &str = "close_window";
    pub const OPEN_AIRBOSS: &str = "open_needs";
    pub const SHORTCUTS: &str = "shortcuts";
    pub const UNDO: &str = "undo";
    pub const COPY_AGENT_NOTE: &str = "copy_agent_note";
    pub const COPY_SETUP: &str = "copy_setup_command";
    pub const REPORT_PROBLEM: &str = "report_problem";
}

/// The handles Rust keeps to enable, disable and relabel items.
pub struct MenuHandles<R: Runtime> {
    /// Task-menu items with whether each needs a selection.
    pub task: Vec<(MenuItem<R>, bool)>,
    pub undo: MenuItem<R>,
    pub quick_add: MenuItem<R>,
}

fn add_items<R: Runtime>(
    app: &AppHandle<R>,
    sub: &Submenu<R>,
    items: &[Option<Item>],
) -> tauri::Result<Vec<(MenuItem<R>, bool)>> {
    let mut out = Vec::new();
    for it in items {
        match it {
            None => sub.append(&PredefinedMenuItem::separator(app)?)?,
            Some(it) => {
                let mi = MenuItem::with_id(app, it.id, it.title, true, it.key)?;
                sub.append(&mi)?;
                out.push((mi, it.needs_selection));
            }
        }
    }
    Ok(out)
}

/// Builds the menu bar: Airboss, File, Edit, Task, View, Window, Help.
pub fn build<R: Runtime>(
    app: &AppHandle<R>,
    hotkey: &str,
) -> tauri::Result<(Menu<R>, MenuHandles<R>)> {
    let about = AboutMetadata {
        name: Some("Airboss".into()),
        // The full version, alpha and all. Left out, the panel shows Info.plist's
        // CFBundleShortVersionString, which Apple limits to three numbers ("0.1.0").
        version: Some(env!("CARGO_PKG_VERSION").into()),
        comments: Some("A to-do list where agents do the work.".into()),
        copyright: Some("Copyright © 2026 Aman Tibrewal. MIT License.".into()),
        ..AboutMetadata::default()
    };
    let app_menu = Submenu::with_items(
        app,
        "Airboss",
        true,
        &[
            &PredefinedMenuItem::about(app, Some("About Airboss"), Some(about))?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, ids::SETTINGS, "Settings…", true, Some("CmdOrCtrl+,"))?,
            &MenuItem::with_id(
                app,
                ids::INSTALL_CLI,
                "Install Command-Line Tool",
                true,
                None::<&str>,
            )?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, Some("Quit Airboss"))?,
        ],
    )?;
    let quick_add = MenuItem::with_id(app, ids::QUICK_ADD, "Quick Add", true, Some(hotkey))?;
    let file = Submenu::with_items(
        app,
        "File",
        true,
        &[
            &MenuItem::with_id(app, ids::NEW_TASK, "New Task", true, Some("CmdOrCtrl+N"))?,
            &quick_add,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(
                app,
                ids::CLOSE_WINDOW,
                "Close Window",
                true,
                Some("CmdOrCtrl+W"),
            )?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let task = Submenu::new(app, "Task", true)?;
    let mut task_items = add_items(app, &task, TASK_MENU)?;
    task.append(&PredefinedMenuItem::separator(app)?)?;
    // "Undo <action>" has no key equivalent: ⌘Z belongs to Edit › Undo for text fields, and
    // the page handles ⌘Z outside them.
    let undo = MenuItem::with_id(app, ids::UNDO, "Undo", false, None::<&str>)?;
    task.append(&undo)?;
    let view = Submenu::new(app, "View", true)?;
    add_items(app, &view, VIEW_MENU)?;
    let window = Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, Some("Zoom"))?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(
                app,
                ids::OPEN_AIRBOSS,
                "Open Airboss",
                true,
                Some("CmdOrCtrl+0"),
            )?,
        ],
    )?;
    let help = Submenu::with_items(
        app,
        "Help",
        true,
        &[
            &MenuItem::with_id(
                app,
                ids::SHORTCUTS,
                "Keyboard Shortcuts  ?",
                true,
                None::<&str>,
            )?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(
                app,
                ids::COPY_AGENT_NOTE,
                "Copy Agent Note",
                true,
                None::<&str>,
            )?,
            &MenuItem::with_id(
                app,
                ids::COPY_SETUP,
                "Copy Setup Command",
                true,
                None::<&str>,
            )?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(
                app,
                ids::REPORT_PROBLEM,
                "Report a Problem…",
                true,
                None::<&str>,
            )?,
        ],
    )?;
    let menu = Menu::with_items(
        app,
        &[&app_menu, &file, &edit, &task, &view, &window, &help],
    )?;
    task_items.shrink_to_fit();
    Ok((
        menu,
        MenuHandles {
            task: task_items,
            undo,
            quick_add,
        },
    ))
}

impl<R: Runtime> MenuHandles<R> {
    /// Applies a [`MenuState`]: enables, disables and relabels.
    pub fn apply(&self, state: &MenuState) {
        for (item, needs_selection) in &self.task {
            let on = if *needs_selection {
                state.selection
            } else {
                state.task_items
            };
            let _ = item.set_enabled(on);
        }
        match &state.undo {
            Some(label) => {
                let _ = self.undo.set_text(format!("Undo {label}"));
                let _ = self.undo.set_enabled(true);
            }
            None => {
                let _ = self.undo.set_text("Undo");
                let _ = self.undo.set_enabled(false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn focus(zone: Zone, selection: Option<i64>) -> FocusContext {
        FocusContext {
            window: WindowName::Main,
            zone,
            selection: selection.map(TaskId),
            view: "all".into(),
        }
    }

    #[test]
    fn task_items_act_in_list_panel_and_sidebar() {
        for zone in [Zone::List, Zone::Panel, Zone::Sidebar] {
            let s = menu_state(&focus(zone, Some(1)), Some("Mark done #1".into()));
            assert!(s.task_items && s.selection, "{zone:?}");
            assert_eq!(s.undo.as_deref(), Some("Mark done #1"));
        }
    }

    #[test]
    fn fields_overlays_and_quick_add_disable_every_task_item() {
        for zone in [Zone::Field, Zone::Overlay, Zone::Quickadd] {
            let s = menu_state(&focus(zone, Some(1)), Some("x".into()));
            assert!(
                !s.task_items && !s.selection && s.undo.is_none(),
                "{zone:?}"
            );
        }
        let mut f = focus(Zone::List, Some(1));
        f.window = WindowName::Quickadd;
        assert!(!menu_state(&f, None).task_items);
    }

    #[test]
    fn items_that_need_a_selection_are_off_without_one() {
        let s = menu_state(&focus(Zone::List, None), None);
        assert!(s.task_items);
        assert!(!s.selection);
        assert!(s.undo.is_none());
    }

    #[test]
    fn no_menu_item_has_a_bare_letter_key_equivalent() {
        for it in TASK_MENU.iter().chain(VIEW_MENU).flatten() {
            if let Some(k) = it.key {
                assert!(k.contains('+'), "{} has a bare key {k}", it.id);
            }
        }
    }
}
