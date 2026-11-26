//! The real menu bar, built by `menu::build` on Tauri's mock runtime. AppKit menus can only
//! be made on the main thread, so this test has no harness and runs in `main`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use airboss_app::menu::{self, ids};
use tauri::menu::MenuItemKind;

/// The Airboss menu as (id, title) pairs; separators have an empty title.
fn app_menu() -> Vec<(String, String)> {
    let app = tauri::test::mock_app();
    let (bar, _handles) = menu::build(app.handle(), "Control+Alt+N").unwrap();
    let items = bar.items().unwrap();
    let MenuItemKind::Submenu(first) = &items[0] else {
        panic!("the first menu isn't a submenu");
    };
    assert_eq!(first.text().unwrap(), "Airboss");
    first
        .items()
        .unwrap()
        .into_iter()
        .map(|it| {
            let title = match &it {
                MenuItemKind::MenuItem(m) => m.text().unwrap(),
                MenuItemKind::Predefined(p) => p.text().unwrap(),
                _ => String::new(),
            };
            (it.id().0.clone(), title)
        })
        .collect()
}

fn check_for_updates_is_right_under_about() {
    let items = app_menu();
    assert_eq!(items[0].1, "About Airboss", "{items:?}");
    assert_eq!(
        items[1],
        (
            ids::CHECK_FOR_UPDATES.to_string(),
            "Check for Updates…".to_string()
        ),
        "{items:?}"
    );
    // Rust doesn't answer it: `on_menu_event` forwards it as `menu:action` with this id,
    // the window's command in `commands.ts` (app/tests/unit/menu.test.ts checks the window
    // runs every id it's sent).
    assert_eq!(ids::CHECK_FOR_UPDATES, "check_for_updates");
    // Then a separator, then Settings…
    assert_eq!(items[2].1, "", "{items:?}");
    assert_eq!(items[3].0, ids::SETTINGS, "{items:?}");
}

fn main() {
    check_for_updates_is_right_under_about();
    println!("test check_for_updates_is_right_under_about ... ok");
}
