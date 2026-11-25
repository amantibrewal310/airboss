//! `airboss-mcp`: the `airboss` binary's insides.
//!
//! `airboss mcp` is a per-thread MCP server over stdio for agents; every other subcommand
//! is a small CLI that acts as `me`. Both go through `airboss_core::Store` and nothing else.
//! See `SPEC-mcp.md` at the repo root.

use std::path::PathBuf;

use airboss_core::Store;

pub mod identity;
pub mod render;
pub mod server;
pub mod tools;

/// The variable that overrides where the database lives; tests always set it.
pub const DB_VAR: &str = "AIRBOSS_DB";

/// Where the database lives: `AIRBOSS_DB` if set, otherwise
/// `~/Library/Application Support/Airboss/airboss.db`.
pub fn db_path(var: impl Fn(&str) -> Option<String>) -> Result<PathBuf, String> {
    if let Some(path) = var(DB_VAR).filter(|p| !p.trim().is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let home = var("HOME")
        .filter(|h| !h.trim().is_empty())
        .ok_or_else(|| format!("HOME isn't set, so there's no default database; set {DB_VAR}"))?;
    Ok(PathBuf::from(home).join("Library/Application Support/Airboss/airboss.db"))
}

/// Opens the database, creating it and its directory on first use.
pub fn open_store() -> Result<Store, String> {
    let path = db_path(|name| std::env::var(name).ok())?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
    }
    Store::open(&path).map_err(|e| format!("couldn't open {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn airboss_db_overrides_the_default_path() {
        let path = db_path(|name| match name {
            DB_VAR => Some("/tmp/x/airboss.db".into()),
            "HOME" => Some("/Users/me".into()),
            _ => None,
        });
        assert_eq!(path.unwrap(), PathBuf::from("/tmp/x/airboss.db"));
    }

    #[test]
    fn the_default_path_is_in_application_support() {
        let path = db_path(|name| (name == "HOME").then(|| "/Users/me".into()));
        assert_eq!(
            path.unwrap(),
            PathBuf::from("/Users/me/Library/Application Support/Airboss/airboss.db")
        );
    }

    #[test]
    fn no_home_and_no_override_is_an_error() {
        assert!(db_path(|_| None).unwrap_err().contains(DB_VAR));
    }
}
