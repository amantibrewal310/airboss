//! By hand: what `T3Db` says about the 10 most recent threads, next to what
//! `t3 thread show <id> --json` prints, to compare state, branch and settled.
//!
//! ```sh
//! cargo run -p airboss-sync --example t3_check
//! ```
//!
//! Both sides only read. The T3 CLI's JSON is printed as it comes, not parsed, so a change
//! in its shape can't break the comparison.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};

use airboss_services::time::to_ts;
use airboss_sync::t3::{T3Db, T3Query, T3Source};
use airboss_sync::threads;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    let mut db = T3Db::new(&Path::new(&home).join(".t3/userdata"));
    db.open()?;
    let since = to_ts(SystemTime::now() - Duration::from_secs(7 * 24 * 3600));
    let first = db.read(&T3Query {
        untracked_since: Some(since),
        ..T3Query::default()
    })?;
    let ids: Vec<String> = first
        .untracked
        .unwrap_or_default()
        .into_iter()
        .take(10)
        .map(|t| t.id)
        .collect();
    let read = db.read(&T3Query {
        linked: ids.clone(),
        ..T3Query::default()
    })?;
    for v in threads::linked(&read, &ids, &HashMap::new()).views {
        println!("== {}", v.key.r#ref);
        println!(
            "   sync: state {:?}, branch {:?}, title {:?}",
            v.state, v.branch, v.title
        );
        let out = Command::new("t3")
            .args(["thread", "show", &v.key.r#ref, "--json"])
            .output();
        match out {
            Ok(o) => println!("   t3:   {}", String::from_utf8_lossy(&o.stdout).trim()),
            Err(e) => println!("   t3:   couldn't run: {e}"),
        }
    }
    Ok(())
}
