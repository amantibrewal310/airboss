//! `airboss-mcp`: the `airboss` binary's insides.
//!
//! `airboss mcp` is a per-thread MCP server over stdio for agents; every other subcommand
//! is a small CLI that acts as `me`. Both go through `airboss_core::Store` and nothing else.
//! See `SPEC-mcp.md` at the repo root.

use airboss_core::Store;

pub mod cli;
pub mod identity;
pub mod render;
pub mod server;
pub mod tools;

/// The variable that overrides where the database lives; tests always set it.
pub use airboss_core::DB_VAR;

/// Opens the database at `core`'s [`airboss_core::default_db_path`], creating it and its
/// directory on first use.
pub fn open_store() -> Result<Store, String> {
    let path = airboss_core::default_db_path().map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
    }
    Store::open(&path).map_err(|e| match e.is_newer_database() {
        true => e.to_string(),
        false => format!("couldn't open {}: {e}", path.display()),
    })
}
