//! How much of the app log is kept: `~/Library/Logs/Airboss/app.log` plus the last two.
//!
//! The log plugin's default keeps one file of 40 KB and deletes it when it's full, so the
//! history a support question needs was gone at the first rotation.

use tauri_plugin_log::RotationStrategy;

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
