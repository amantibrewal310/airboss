//! Seeds a database for timings and screenshots, never the real one by default.
//!
//! `cargo run -p airboss-app --example seed -- --tasks 2000 --deps 1000 --db /tmp/ab.db`
//! `cargo run -p airboss-app --example seed -- --design --db /tmp/design.db`

use std::path::PathBuf;
use std::process::ExitCode;

use airboss_core::Store;

fn main() -> ExitCode {
    let mut tasks = 2000;
    let mut deps = 1000;
    let mut db: Option<PathBuf> = None;
    let mut design = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--tasks" => tasks = args.next().and_then(|v| v.parse().ok()).unwrap_or(tasks),
            "--deps" => deps = args.next().and_then(|v| v.parse().ok()).unwrap_or(deps),
            "--db" => db = args.next().map(PathBuf::from),
            "--design" => design = true,
            other => {
                eprintln!("unknown argument {other}");
                return ExitCode::FAILURE;
            }
        }
    }
    let Some(db) = db else {
        eprintln!("--db <path> is required (the real database is never seeded)");
        return ExitCode::FAILURE;
    };
    let mut store = match Store::open(&db) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("can't open {}: {e}", db.display());
            return ExitCode::FAILURE;
        }
    };
    let result = if design {
        airboss_app::seed::design(&mut store).map(|d| d.ages.len())
    } else {
        airboss_app::seed::large(&mut store, tasks, deps).map(|ids| ids.len())
    };
    match result {
        Ok(n) => {
            println!("seeded {n} tasks into {}", db.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("seeding failed: {e}");
            ExitCode::FAILURE
        }
    }
}
