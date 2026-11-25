//! Launch's own connection and tables: `launch_threads` and `launch_repos`.

use airboss_core::Migration;

/// Launch's migrations, run with `Store::migrate_module("launch", MIGRATIONS)`.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: include_str!("migrations/001_launch.sql"),
}];
