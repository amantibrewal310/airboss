//! No duplicate threads: the pending row comes first, and a place that started is skipped.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::{Arc, Mutex};

use airboss_services::LaunchService;
use common::{H, do_it, plan_it, rows_at};

#[test]
fn the_pending_row_is_committed_before_t3_runs() {
    let h = H::new();
    let web = h.repo("web");
    let api = h.repo("api");
    let t = h.task("Booking flow", &[&web, &api]);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (db, seen2, id) = (h.cfg.db_path.clone().unwrap(), seen.clone(), t.id);
    h.t3.on_call(move |call| {
        if call.is_start() {
            // Read from another connection: the row must already be committed.
            let states: Vec<String> = rows_at(&db, id).into_iter().map(|r| r.2).collect();
            seen2.lock().unwrap().push(states);
        }
    });
    h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        vec![
            vec!["pending".to_string()],
            vec!["started".to_string(), "pending".to_string()]
        ]
    );
}

#[test]
fn a_started_place_is_skipped_on_retry() {
    let h = H::new();
    let web = h.repo("web");
    let t = h.task("Booking flow", &[&web]);
    h.launch.start(&do_it(t.id)).unwrap();
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(h.t3.starts().len(), 1);
    assert_eq!(report.skipped, vec![web.id]);
    assert!(report.started.is_empty() && report.failed.is_empty());
    assert!(!report.moved_to_doing);
    // Do it and Plan it are separate places.
    h.launch.start(&plan_it(t.id)).unwrap();
    assert_eq!(h.t3.starts().len(), 2);
}
