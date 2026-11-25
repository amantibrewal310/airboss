//! Every kind through `compute` alone: one test per kind showing, and one per rule hiding it.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use airboss_attention::AttentionConfig;
use common::*;

#[test]
fn the_defaults_are_the_em_thresholds() {
    let cfg = AttentionConfig::default();
    assert_eq!(cfg.quiet_after, DAY * 5);
    assert_eq!(cfg.snooze_for, DAY * 3);
    assert_eq!(cfg.ready_window, DAY * 7);
    assert_eq!(cfg.menu_items, 6);
    assert_eq!(cfg.settle, Duration::from_millis(150));
}

#[test]
fn nothing_in_gives_nothing_out() {
    let out = run(empty_core(), Links::new().build(), at(10, 0));
    assert!(out.items.is_empty());
    assert!(out.live_keys.is_empty());
    assert_eq!(out.next_wake, None);
}
