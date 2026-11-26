//! Every command in Appendix A as a plain function, `fn(&AppState, Caller, args…) ->
//! AppResult<T>`, so tests call them without Tauri. `ipc.rs` wraps each one as a Tauri command.

pub mod app;
pub mod detail;
pub mod launch;
pub mod needs;
pub mod repos;
pub mod tasks;
pub mod updates;
