//! The one error type every `core` call returns.

/// Why a `core` call was refused or failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A task's title is empty or only spaces.
    #[error("a task needs a title")]
    EmptyTitle,
    /// Something with the same unique name already exists, e.g. a repo name or path.
    #[error("already exists")]
    AlreadyExists,
    /// The task, link or repo doesn't exist.
    #[error("not found")]
    NotFound,
    /// This actor may never make this kind of change.
    #[error("not allowed")]
    NotAllowed,
    /// This actor may make this change in general, but not from the task's current status.
    #[error("not allowed from the current status")]
    InvalidTransition,
    /// An agent finishing a task must say what it did.
    #[error("a note saying what was done is required")]
    NoteRequired,
    /// The target can't take this change, e.g. moving under a finished parent.
    #[error("invalid target")]
    InvalidTarget,
    /// The change would create a loop in the tree or in the dependencies.
    #[error("this would create a loop")]
    Loop,
    /// A plan used the same key twice.
    #[error("duplicate plan key: {0}")]
    DuplicateKey(String),
    /// The database itself failed.
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}

/// What a binary says when the file was migrated by a newer Airboss than itself, so its
/// writes could break rules it doesn't know about.
pub const NEWER_DATABASE: &str =
    "This database was created by a newer Airboss. Update the `airboss` command.";

impl Error {
    /// The file has a migration newer than this binary knows: it's refused, not written.
    ///
    /// Carried as `Error::Db`, so callers that match every variant keep compiling; check it
    /// with [`Error::is_newer_database`].
    pub(crate) fn newer_database() -> Error {
        Error::Db(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
            Some(NEWER_DATABASE.to_string()),
        ))
    }

    /// There's no file to open, e.g. no `HOME` to find the default one in; `why` says so.
    pub(crate) fn cant_open(why: String) -> Error {
        Error::Db(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
            Some(why),
        ))
    }

    /// Whether this is the refusal of a database a newer Airboss migrated. Its `Display` is
    /// [`NEWER_DATABASE`], which is what to show.
    pub fn is_newer_database(&self) -> bool {
        matches!(
            self,
            Error::Db(rusqlite::Error::SqliteFailure(_, Some(m))) if m == NEWER_DATABASE
        )
    }
}

/// Shorthand used across the crate.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// SQLite couldn't read or write the file itself: an I/O error, corruption, a file that
    /// isn't a database, or one it can't open. Not a busy or locked database, and not a rule.
    /// The app shows these as a database problem instead of a failed save.
    pub fn is_storage_failure(&self) -> bool {
        use rusqlite::ErrorCode as C;
        match self {
            Error::Db(e) => matches!(
                e.sqlite_error_code(),
                Some(C::SystemIoFailure | C::DatabaseCorrupt | C::NotADatabase | C::CannotOpen)
            ),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::ffi;

    fn sqlite(code: i32) -> Error {
        Error::Db(rusqlite::Error::SqliteFailure(ffi::Error::new(code), None))
    }

    #[test]
    fn io_errors_and_corruption_are_storage_failures_and_nothing_else_is() {
        for code in [
            ffi::SQLITE_IOERR,
            ffi::SQLITE_IOERR_SHORT_READ,
            ffi::SQLITE_IOERR_FSTAT,
            ffi::SQLITE_CORRUPT,
            ffi::SQLITE_NOTADB,
            ffi::SQLITE_CANTOPEN,
        ] {
            assert!(sqlite(code).is_storage_failure(), "{code}");
        }
        for code in [
            ffi::SQLITE_BUSY,
            ffi::SQLITE_LOCKED,
            ffi::SQLITE_CONSTRAINT,
            ffi::SQLITE_FULL,
        ] {
            assert!(!sqlite(code).is_storage_failure(), "{code}");
        }
        assert!(!Error::NotFound.is_storage_failure());
        assert!(!Error::Db(rusqlite::Error::QueryReturnedNoRows).is_storage_failure());
    }
}
