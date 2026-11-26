//! The app: plugins, windows and the run loop. Everything else is in the library.

use std::time::Instant;

use airboss_app::native;
use tauri_plugin_global_shortcut::ShortcutState;
use tauri_plugin_log::{Target, TargetKind, log::LevelFilter};

fn main() {
    let started = Instant::now();
    // Before anything starts a thread: service lines and panics go to the app log.
    airboss_app::logs::install();
    airboss_app::macos::disable_state_restoration();
    let hidden = std::env::args().any(|a| a == "--hidden");
    let logs = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default()
        .join("Library/Logs/Airboss");
    let mut log = tauri_plugin_log::Builder::new()
        .clear_targets()
        .target(Target::new(TargetKind::Folder {
            path: logs,
            file_name: Some("app".into()),
        }))
        .max_file_size(airboss_app::logs::MAX_FILE_BYTES)
        .rotation_strategy(airboss_app::logs::rotation())
        .level(LevelFilter::Info);
    if cfg!(debug_assertions) {
        log = log.target(Target::new(TargetKind::Stdout));
    }
    let app = tauri::Builder::default()
        // First, so a second launch never gets further than handing over its arguments.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            native::second_launch(app, &args);
        }))
        .plugin(log.build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    // Pressed only: holding or releasing the keys does nothing more.
                    if event.state == ShortcutState::Pressed {
                        airboss_app::quickadd::toggle(app);
                    }
                })
                .build(),
        )
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .macos_launcher(tauri_plugin_autostart::MacosLauncher::LaunchAgent)
                .args(["--hidden"])
                .build(),
        )
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION,
                )
                .with_denylist(&[airboss_app::quickadd::LABEL])
                .build(),
        )
        .plugin(tauri_nspanel::init())
        .invoke_handler(airboss_app::ipc::handler())
        .setup(move |app| Ok(native::setup(app, hidden, started)?))
        .on_window_event(native::on_window_event)
        .on_menu_event(native::on_menu_event)
        .build(tauri::generate_context!());
    match app {
        Ok(app) => app.run(native::on_run_event),
        Err(e) => {
            tauri_plugin_log::log::error!("Airboss couldn't start: {e}");
            eprintln!("Airboss couldn't start: {e}");
            std::process::exit(1);
        }
    }
}
