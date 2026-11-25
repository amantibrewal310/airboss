//! `airboss-core`: the single source of truth for Airboss tasks.
//!
//! Every other module reads and writes tasks only through [`Store`]-level calls in this crate.
//! See `SPEC-core.md` at the repo root for the rules enforced here.

mod error;

pub use error::{Error, Result};

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
