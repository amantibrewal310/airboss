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

mod settling {
    use super::*;

    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use airboss_core::Status;
    use airboss_launch::fake::Reply;
    use airboss_services::{ErrorCode, Mode, Place, StartRequest};
    use common::{again, t0, t3_key};

    const SECS: fn(u64) -> Duration = Duration::from_secs;

    fn branch_of(id: airboss_core::TaskId) -> String {
        format!("task-{}-booking-flow", id.0)
    }

    #[test]
    fn a_timeout_where_t3_still_made_the_thread_links_it_without_a_second_start() {
        let mut h = H::new();
        let web = h.repo("web");
        let t = h.task("Booking flow", &[&web]);
        h.t3.push(Reply::Timeout);
        let report = h.launch.start(&do_it(t.id)).unwrap();
        assert_eq!(report.failed[0].1.code, ErrorCode::Timeout);
        assert_eq!(h.rows(t.id)[0].2, "pending");

        // T3 made it after all; sync sees it on its branch.
        h.sync
            .answer_find_thread(h.path_of(&web), &branch_of(t.id), t3_key("late-thread"));
        let step = h
            .worker
            .handle(airboss_launch::LaunchMsg::Tick, t0() + SECS(10));
        assert!(step.changed);
        assert_eq!(h.rows(t.id)[0].2, "started");
        assert_eq!(h.rows(t.id)[0].3.as_deref(), Some("late-thread"));
        let links = h.store.lock().unwrap().links(t.id).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].r#ref, "late-thread");
        assert_eq!(h.get(t.id).status, Status::Doing);

        // Retrying now skips it; t3 ran once in all.
        let retry = h.launch.start(&do_it(t.id)).unwrap();
        assert_eq!(retry.skipped, vec![web.id]);
        assert_eq!(h.t3.starts().len(), 1);
    }

    #[test]
    fn a_retry_after_a_timeout_settles_first_and_never_starts_a_duplicate() {
        let h = H::new();
        let web = h.repo("web");
        let t = h.task("Booking flow", &[&web]);
        h.t3.push(Reply::Timeout);
        h.launch.start(&do_it(t.id)).unwrap();
        // Retry before the thread is found: still starting.
        let r = h.launch.start(&do_it(t.id)).unwrap();
        assert_eq!(r.failed[0].1.code, ErrorCode::AlreadyStarting);
        assert_eq!(
            r.failed[0].1.message,
            "web is still starting. Check T3, or choose Start again."
        );
        // Once sync sees it, the retry itself links it.
        h.sync
            .answer_find_thread(h.path_of(&web), &branch_of(t.id), t3_key("late"));
        let r = h.launch.start(&do_it(t.id)).unwrap();
        assert_eq!(r.skipped, vec![web.id]);
        assert_eq!(h.t3.starts().len(), 1);
        assert_eq!(h.store.lock().unwrap().links(t.id).unwrap().len(), 1);
    }

    #[test]
    fn quitting_while_t3_runs_then_restarting_links_the_thread_and_starts_no_second() {
        let mut h = H::new();
        let web = h.repo("web");
        let t = h.task("Booking flow", &[&web]);
        // t3 is running when Airboss quits: the pending row is committed, no answer came.
        let (entered_tx, entered) = mpsc::channel();
        let (release, release_rx) = mpsc::channel::<()>();
        h.t3.push(Reply::Hold {
            entered: entered_tx,
            release: release_rx,
            then: Box::new(Reply::Killed),
        });
        let launch = h.launch.clone();
        let id = t.id;
        let running = thread::spawn(move || launch.start(&do_it(id)));
        entered.recv_timeout(SECS(5)).unwrap();
        assert_eq!(h.rows(t.id)[0].2, "pending");
        // The process died with it; T3 made the thread anyway.
        drop(release);
        let _ = running.join().unwrap();

        h.restart();
        h.sync.answer_find_thread(
            h.path_of(&web),
            &branch_of(t.id),
            t3_key("made-before-quit"),
        );
        // The worker's first jobs on start.
        while let Some(msg) = h.worker.try_next() {
            h.worker.handle(msg, t0() + SECS(1));
        }
        assert_eq!(h.rows(t.id)[0].2, "started");
        assert_eq!(
            h.store.lock().unwrap().links(t.id).unwrap()[0].r#ref,
            "made-before-quit"
        );
        assert_eq!(h.get(t.id).status, Status::Doing);
        let r = h.launch.start(&do_it(t.id)).unwrap();
        assert_eq!(r.skipped, vec![web.id]);
        assert_eq!(h.t3.starts().len(), 1);
    }

    #[test]
    fn a_pending_row_blocks_until_start_again() {
        let h = H::new();
        let web = h.repo("web");
        let t = h.task("Booking flow", &[&web]);
        h.t3.push(Reply::Timeout);
        h.launch.start(&do_it(t.id)).unwrap();
        for _ in 0..2 {
            let r = h.launch.start(&do_it(t.id)).unwrap();
            assert_eq!(r.failed[0].1.code, ErrorCode::AlreadyStarting);
        }
        assert_eq!(h.t3.starts().len(), 1);
        let r = h.launch.start(&again(do_it(t.id))).unwrap();
        assert_eq!(r.started.len(), 1);
        let rows = h.rows(t.id);
        assert_eq!(
            (rows[0].2.as_str(), rows[1].2.as_str()),
            ("lost", "started")
        );
        // The timed-out thread may hold the branch, so the new one gets -2.
        assert_eq!(rows[1].4, Some(format!("{}-2", branch_of(t.id))));
    }

    #[test]
    fn a_do_it_row_not_found_in_3_minutes_is_lost_and_a_retry_starts_again() {
        let mut h = H::new();
        let web = h.repo("web");
        let t = h.task("Booking flow", &[&web]);
        h.t3.push(Reply::Timeout);
        h.launch.start(&do_it(t.id)).unwrap();
        h.worker
            .handle(airboss_launch::LaunchMsg::Tick, t0() + SECS(179));
        assert_eq!(h.rows(t.id)[0].2, "pending");
        let step = h
            .worker
            .handle(airboss_launch::LaunchMsg::Tick, t0() + SECS(180));
        assert!(step.changed);
        assert_eq!(h.rows(t.id)[0].2, "lost");
        let r = h.launch.start(&do_it(t.id)).unwrap();
        assert_eq!(r.started.len(), 1);
        assert_eq!(h.t3.starts().len(), 2);
    }

    #[test]
    fn an_interrupted_plan_it_stays_pending_until_start_again() {
        let mut h = H::new();
        let web = h.repo("web");
        let t = h.task("Booking flow", &[&web]);
        h.t3.push(Reply::Timeout);
        h.launch.start(&plan_it(t.id)).unwrap();
        // No branch to look up, so no amount of time settles it.
        h.worker
            .handle(airboss_launch::LaunchMsg::Tick, t0() + SECS(3600));
        assert_eq!(h.rows(t.id)[0].2, "pending");
        let r = h.launch.start(&plan_it(t.id)).unwrap();
        assert_eq!(r.failed[0].1.code, ErrorCode::AlreadyStarting);
        let r = h.launch.start(&again(plan_it(t.id))).unwrap();
        assert_eq!(r.started.len(), 1);
        assert_eq!(h.rows(t.id)[0].2, "lost");
    }

    #[test]
    fn an_interrupted_research_start_stays_pending_too() {
        let mut h = H::new();
        let t = h.task("Research", &[]);
        let req = StartRequest {
            task: t.id,
            mode: Mode::DoIt,
            place: Place::NoRepo(h.cfg.research_dir.clone()),
            again: false,
            launcher: airboss_services::Launcher::T3,
        };
        h.t3.push(Reply::Timeout);
        h.launch.start(&req).unwrap();
        h.worker
            .handle(airboss_launch::LaunchMsg::Tick, t0() + SECS(3600));
        assert_eq!(h.rows(t.id)[0].2, "pending");
        let r = h.launch.start(&req).unwrap();
        assert_eq!(r.failed, vec![(None, r.failed[0].1.clone())]);
        assert_eq!(r.failed[0].1.code, ErrorCode::AlreadyStarting);
        assert_eq!(
            r.failed[0].1.message,
            "This task's thread is still starting. Check T3, or choose Start again."
        );
    }

    #[test]
    fn two_starts_at_once_make_one_pending_row_and_one_thread() {
        let h = H::new();
        let web = h.repo("web");
        let t = h.task("Booking flow", &[&web]);
        let (entered_tx, entered) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        h.t3.push(Reply::Hold {
            entered: entered_tx,
            release: release_rx,
            then: Box::new(Reply::Thread("only-one".into())),
        });
        let launch = h.launch.clone();
        let id = t.id;
        let first = thread::spawn(move || launch.start(&do_it(id)));
        entered.recv_timeout(SECS(5)).unwrap();
        let second = h.launch.start(&do_it(t.id)).unwrap();
        assert_eq!(second.failed[0].1.code, ErrorCode::AlreadyStarting);
        release.send(()).unwrap();
        let first = first.join().unwrap().unwrap();
        assert_eq!(first.started.len(), 1);
        assert_eq!(h.t3.starts().len(), 1);
        assert_eq!(h.rows(t.id).len(), 1);
    }

    #[test]
    fn many_starts_at_once_still_make_one_thread() {
        let h = H::new();
        let web = h.repo("web");
        let t = h.task("Booking flow", &[&web]);
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let launch = h.launch.clone();
                let id = t.id;
                thread::spawn(move || launch.start(&do_it(id)).unwrap())
            })
            .collect();
        let reports: Vec<_> = handles.into_iter().map(|j| j.join().unwrap()).collect();
        assert_eq!(h.t3.starts().len(), 1);
        assert_eq!(reports.iter().map(|r| r.started.len()).sum::<usize>(), 1);
        assert_eq!(h.rows(t.id).len(), 1);
        assert_eq!(h.store.lock().unwrap().links(t.id).unwrap().len(), 1);
    }

    #[test]
    fn two_start_agains_at_once_make_one_new_thread() {
        let h = H::new();
        let web = h.repo("web");
        let t = h.task("Booking flow", &[&web]);
        h.t3.push(Reply::Timeout);
        h.launch.start(&do_it(t.id)).unwrap();
        let (entered_tx, entered) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        h.t3.push(Reply::Hold {
            entered: entered_tx,
            release: release_rx,
            then: Box::new(Reply::Thread("again-1".into())),
        });
        let launch = h.launch.clone();
        let id = t.id;
        let first = thread::spawn(move || launch.start(&again(do_it(id))));
        entered.recv_timeout(SECS(5)).unwrap();
        // The second press replaces only what it saw; the first press's new row isn't one.
        let second = h.launch.start(&again(do_it(t.id))).unwrap();
        assert_eq!(second.failed[0].1.code, ErrorCode::AlreadyStarting);
        assert_eq!(
            second.failed[0].1.message,
            "web is starting right now. Wait a moment, then check T3."
        );
        release.send(()).unwrap();
        assert_eq!(first.join().unwrap().unwrap().started.len(), 1);
        assert_eq!(h.t3.starts().len(), 2);
        let states: Vec<String> = h.rows(t.id).into_iter().map(|r| r.2).collect();
        assert_eq!(states, ["lost", "started"]);
    }

    #[test]
    fn when_sync_and_t3_name_different_threads_both_are_linked() {
        let mut h = H::new();
        let web = h.repo("web");
        let t = h.task("Booking flow", &[&web]);
        let (entered_tx, entered) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        h.t3.push(Reply::Hold {
            entered: entered_tx,
            release: release_rx,
            then: Box::new(Reply::Thread("from-t3".into())),
        });
        let launch = h.launch.clone();
        let id = t.id;
        let running = thread::spawn(move || launch.start(&do_it(id)));
        entered.recv_timeout(SECS(5)).unwrap();
        // While t3 is slow to answer, the worker finds a thread on the branch.
        h.sync
            .answer_find_thread(h.path_of(&web), &branch_of(t.id), t3_key("from-sync"));
        h.worker
            .handle(airboss_launch::LaunchMsg::Tick, t0() + SECS(10));
        release.send(()).unwrap();
        let report = running.join().unwrap().unwrap();
        assert_eq!(report.started.len(), 1);
        assert_eq!(report.started[0].key, t3_key("from-sync"));
        let mut linked: Vec<String> = h
            .store
            .lock()
            .unwrap()
            .links(t.id)
            .unwrap()
            .into_iter()
            .map(|l| l.r#ref)
            .collect();
        linked.sort();
        assert_eq!(linked, ["from-sync", "from-t3"]);
        assert_eq!(h.t3.starts().len(), 1);
    }
}
