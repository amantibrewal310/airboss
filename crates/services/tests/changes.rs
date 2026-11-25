//! `Changes`: bumps wake waiters, timeouts return, and no bump is ever lost.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use airboss_services::Changes;

const LONG: Duration = Duration::from_secs(10);

#[test]
fn a_new_counter_is_at_zero_and_each_bump_adds_one() {
    let c = Changes::new();
    assert_eq!(c.current(), 0);
    c.bump();
    c.bump();
    assert_eq!(c.current(), 2);
}

#[test]
fn clones_share_one_counter() {
    let c = Changes::new();
    let other = c.clone();
    other.bump();
    assert_eq!(c.current(), 1);
}

#[test]
fn a_bump_wakes_a_waiter() {
    let c = Changes::new();
    let waiter = {
        let c = c.clone();
        thread::spawn(move || {
            let t = Instant::now();
            (c.wait_past(0, LONG), t.elapsed())
        })
    };
    thread::sleep(Duration::from_millis(30));
    c.bump();
    let (seen, waited) = waiter.join().unwrap();
    assert_eq!(seen, 1);
    assert!(waited < Duration::from_secs(5), "woke after {waited:?}");
}

#[test]
fn a_bump_wakes_every_waiter() {
    let c = Changes::new();
    let waiters: Vec<_> = (0..4)
        .map(|_| {
            let c = c.clone();
            thread::spawn(move || c.wait_past(0, LONG))
        })
        .collect();
    thread::sleep(Duration::from_millis(30));
    c.bump();
    for w in waiters {
        assert_eq!(w.join().unwrap(), 1);
    }
}

#[test]
fn a_wait_times_out_with_the_count_unchanged() {
    let c = Changes::new();
    c.bump();
    let t = Instant::now();
    assert_eq!(c.wait_past(1, Duration::from_millis(50)), 1);
    let waited = t.elapsed();
    assert!(
        waited >= Duration::from_millis(50),
        "returned after {waited:?}"
    );
    assert!(waited < Duration::from_secs(5));
}

#[test]
fn a_count_already_past_seen_returns_at_once() {
    let c = Changes::new();
    c.bump();
    c.bump();
    let t = Instant::now();
    // The bumps came before the wait: a listener that saw 0 must not miss them.
    assert_eq!(c.wait_past(0, LONG), 2);
    assert!(t.elapsed() < Duration::from_secs(1));
}

#[test]
fn no_bump_is_lost_with_several_bumpers_and_listeners() {
    const BUMPERS: u64 = 4;
    const EACH: u64 = 500;
    const TOTAL: u64 = BUMPERS * EACH;
    let c = Changes::new();
    let start = Arc::new(Barrier::new(BUMPERS as usize + 3));

    // Each listener waits from the last count it saw, as the app's listener threads do. It
    // must reach the final count without one wait ever running out. A wait that runs out
    // still returns the count, so "it moved" isn't enough: each wait must also end long
    // before its timeout, or a bump made before it began was missed.
    let listeners: Vec<_> = (0..3)
        .map(|_| {
            let (c, start) = (c.clone(), start.clone());
            thread::spawn(move || {
                start.wait();
                let mut seen = 0;
                let mut wakes = 0;
                while seen < TOTAL {
                    let t = Instant::now();
                    let now = c.wait_past(seen, LONG);
                    let waited = t.elapsed();
                    assert!(now > seen, "a wait timed out at {seen}: a bump was lost");
                    assert!(
                        waited < LONG / 2,
                        "a wait at {seen} ran {waited:?}: a bump before it was missed"
                    );
                    seen = now;
                    wakes += 1;
                    // Work between waits, as a real listener does, so bumps keep landing
                    // while it isn't waiting.
                    thread::sleep(Duration::from_micros(300));
                }
                (seen, wakes)
            })
        })
        .collect();
    let bumpers: Vec<_> = (0..BUMPERS)
        .map(|_| {
            let (c, start) = (c.clone(), start.clone());
            thread::spawn(move || {
                start.wait();
                for _ in 0..EACH {
                    c.bump();
                }
            })
        })
        .collect();
    for b in bumpers {
        b.join().unwrap();
    }
    for l in listeners {
        let (seen, wakes) = l.join().unwrap();
        assert_eq!(seen, TOTAL);
        assert!(wakes >= 1);
    }
    assert_eq!(c.current(), TOTAL);
}
