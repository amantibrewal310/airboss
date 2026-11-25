//! The app log, `~/Library/Logs/Airboss/app.log`: what goes into it, and how much is kept.
//!
//! - **Everything goes in.** An app opened from Finder has stderr on `/dev/null`, so the
//!   services' lines (sync, launch, attention) and panics on any thread are sent to the log
//!   instead ([`install`]). Nothing else in the app process writes to stderr.
//! - **The file and the last two are kept.** The log plugin's default keeps one file of 40 KB
//!   and deletes it when it's full, so the history a support question needs was gone at the
//!   first rotation.

use tauri_plugin_log::RotationStrategy;
use tauri_plugin_log::log;

/// Sends the services' log lines and every panic's message to the app log. Call it once, at
/// start; lines logged before the log plugin is set up are dropped, and the services only
/// start after it.
pub fn install() {
    airboss_services::log::set_sink(service_line);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let payload = info.payload();
        let what = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("(no message)");
        let at = info
            .location()
            .map(|l| format!(" at {}:{}", l.file(), l.line()))
            .unwrap_or_default();
        log::error!(target: "panic", "thread {} panicked: {what}{at}", thread.name().unwrap_or("unnamed"));
        // Still on stderr too, for a run from a terminal.
        previous(info);
    }));
}

/// One service line, as a warning: services log only what went wrong or what they recovered.
fn service_line(source: &str, message: &str) {
    log::warn!(target: source, "{message}");
}

/// A log file is rotated at 1 MB …
pub const MAX_FILE_BYTES: u128 = 1_000_000;
/// … and the current file and the two before it are kept (`app_<date>.log`).
pub const KEEP_FILES: usize = 3;

/// The rotation the app log uses.
pub fn rotation() -> RotationStrategy {
    RotationStrategy::KeepSome(KEEP_FILES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_log_is_kept_as_an_older_file_not_deleted() {
        assert!(matches!(rotation(), RotationStrategy::KeepSome(n) if n >= 2));
        const { assert!(MAX_FILE_BYTES >= 1_000_000) };
    }
}
