//! The app: plugins, windows and the run loop.

#![cfg_attr(not(test), windows_subsystem = "windows")]

fn main() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}))
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--hidden"])
                .build(),
        )
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION,
                )
                .with_denylist(&["quickadd"])
                .build(),
        )
        .plugin(tauri_nspanel::init())
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("Airboss couldn't start: {e}");
        std::process::exit(1);
    }
}
