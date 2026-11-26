//! `airboss-app`: the Airboss Mac app.
//!
//! The library holds every module so that tests call the commands as plain functions without
//! Tauri; `main.rs` only builds and runs the app. See `SPEC-app.md`.

pub mod catchup;
pub mod classify;
pub mod commands;
pub mod connections;
pub mod dbhealth;
pub mod dto;
pub mod errors;
pub mod handlers;
pub mod ipc;
pub mod login_path;
pub mod logs;
pub mod macos;
pub mod menu;
pub mod native;
pub mod notify;
pub mod opener;
pub mod quickadd;
pub mod runmap;
pub mod seed;
pub mod services;
pub mod settings;
pub mod shell;
pub mod state;
pub mod trace;
pub mod tray;
pub mod undo;
pub mod util;
pub mod viewmodel;
pub mod watcher;

pub use ipc::App;
pub use services::Services;
pub use shell::{RecordingShell, Shell, UiEvent};
pub use state::{AppConfig, AppState, Caller, share};
