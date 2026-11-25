//! `airboss-mcp`: the `airboss` binary's insides.
//!
//! `airboss mcp` is a per-thread MCP server over stdio for agents; every other subcommand
//! is a small CLI that acts as `me`. Both go through `airboss_core::Store` and nothing else.
//! See `SPEC-mcp.md` at the repo root.

pub mod identity;
