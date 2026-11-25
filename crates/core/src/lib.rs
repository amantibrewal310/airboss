//! `airboss-core`: the single source of truth for Airboss tasks.
//!
//! Every other module reads and writes tasks only through [`Store`]-level calls in this crate.
//! See `SPEC-core.md` at the repo root for the rules enforced here.

mod db;
mod error;

use std::path::Path;

use rusqlite::Connection;

pub use error::{Error, Result};

/// The one handle to the task database. All reads and writes go through it.
///
/// It isn't shared between threads by itself: the app keeps one behind a mutex.
pub struct Store {
    #[allow(dead_code)] // used from T3 on
    conn: Connection,
}

impl Store {
    /// Opens (or creates) the database file and brings `core`'s schema up to date.
    pub fn open(path: &Path) -> Result<Store> {
        Ok(Store {
            conn: db::open_file(path)?,
        })
    }

    /// Opens a private in-memory database, mainly for tests.
    pub fn open_in_memory() -> Result<Store> {
        Ok(Store {
            conn: db::open_memory()?,
        })
    }
}

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub i64);
    };
}

id_type!(
    /// A task's row id. Never reused while the task exists.
    TaskId
);
id_type!(
    /// An event's row id; also the cursor for "what changed since".
    EventId
);
id_type!(
    /// A link's row id.
    LinkId
);
id_type!(
    /// A repo's row id.
    RepoId
);
