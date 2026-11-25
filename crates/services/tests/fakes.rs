//! The fakes implement every trait, work as the trait objects the app and services hold, and
//! do the little they promise.
#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use airboss_core::{RepoId, TaskId};
use airboss_services::fakes::{FAKE_ENVIRONMENT, FakeAttention, FakeClock, FakeLaunch, FakeSync};
use airboss_services::{
    AttentionService, Clock, Connection, ErrorCode, LaunchService, Mode, NeedsGroup, NeedsItem,
    NeedsKey, NeedsKind, Place, PrRef, Refresh, Service, Source, StartRequest, SyncService,
    SyncSnapshot, Target, ThreadKey,
};

/// Compiles only if each fake implements its trait and every supertrait, and each trait is
/// usable as a shared trait object (the shapes `start` functions take).
#[test]
fn every_fake_implements_its_trait_as_a_shared_trait_object() {
    let sync: Arc<dyn SyncService> = Arc::new(FakeSync::new());
    let launch: Arc<dyn LaunchService> = Arc::new(FakeLaunch::new());
    let attention: Arc<dyn AttentionService> = Arc::new(FakeAttention::new());
    let clock: Arc<dyn Clock> = Arc::new(FakeClock::at(UNIX_EPOCH));
    // Every service as a plain `Service`, as the app's listener threads hold them.
    let all: Vec<Arc<dyn Service>> = vec![sync.clone(), launch.clone(), attention.clone()];
    for service in &all {
        assert!(service.connections().is_empty());
        service.recheck();
        service.woke();
        assert_eq!(service.changes().current(), 0);
    }
    fn send_sync<T: Send + Sync + ?Sized>(_: &T) {}
    send_sync(&*sync);
    send_sync(&*clock);
    assert_eq!(clock.now(), UNIX_EPOCH);
}

fn t3(r: &str) -> ThreadKey {
    ThreadKey {
        source: Source::T3,
        r#ref: r.into(),
    }
}

#[test]
fn fake_sync_serves_its_snapshot_and_scripted_answers() {
    let sync = FakeSync::new();
    let changes = sync.changes();
    sync.set_snapshot(SyncSnapshot {
        checked_at: Some("2026-11-26T09:00:00.000Z".into()),
        ..SyncSnapshot::default()
    });
    assert_eq!(changes.current(), 1);
    assert!(sync.snapshot().checked_at.is_some());

    sync.answer_find_thread("/src/web", "task-1-x", t3("abc"));
    assert_eq!(
        sync.find_thread(Path::new("/src/web"), "task-1-x"),
        Some(t3("abc"))
    );
    assert_eq!(sync.find_thread(Path::new("/src/web"), "other"), None);

    let pr = PrRef::parse("acme/web#1").unwrap();
    assert_eq!(sync.lookup_pr(&pr).unwrap_err().code, ErrorCode::PrNotFound);
    assert_eq!(
        sync.thread_url(&t3("abc")),
        Some(format!("t3code://threads/{FAKE_ENVIRONMENT}/abc"))
    );
    let claude = ThreadKey {
        source: Source::Claude,
        r#ref: "s".into(),
    };
    assert_eq!(sync.thread_url(&claude), None);

    sync.refresh(Refresh::PrsOf(TaskId(3)));
    sync.shutdown();
    let calls = sync.calls();
    assert!(
        calls.contains(&"refresh(PrsOf(TaskId(3)))".to_string()),
        "{calls:?}"
    );
    assert_eq!(calls.last().map(String::as_str), Some("shutdown"));
}

#[test]
fn fake_launch_records_each_start_and_answers_as_set() {
    let launch = FakeLaunch::new();
    let req = StartRequest {
        task: TaskId(1),
        mode: Mode::DoIt,
        place: Place::TaskRepos,
        again: false,
    };
    assert!(launch.start(&req).unwrap().started.is_empty());
    launch.set_start(Err(airboss_services::ServiceError::new(
        ErrorCode::T3NotRunning,
        "T3 Code isn't open.",
    )));
    assert_eq!(
        launch.start(&req).unwrap_err().code,
        ErrorCode::T3NotRunning
    );
    assert_eq!(launch.starts(), vec![req.clone(), req]);
    assert!(launch.prepare(TaskId(1), Mode::PlanIt).is_ok());
    launch.add_to_t3(RepoId(2)).unwrap();
    launch.set_connections(vec![Connection {
        name: "T3 Code".into(),
        ok: true,
        state: "Connected".into(),
        detail: "Do it and Plan it start agent threads here.".into(),
        fix: None,
    }]);
    assert_eq!(launch.connections().len(), 1);
    assert_eq!(launch.changes().current(), 1);
}

fn needs(key: &str, task: i64) -> NeedsItem {
    NeedsItem {
        key: NeedsKey(key.into()),
        kind: NeedsKind::Ready,
        group: NeedsGroup::Ready,
        task: TaskId(task),
        task_title: "t".into(),
        since: "2026-11-26T09:00:00.000Z".into(),
        headline: "Ready to start".into(),
        context: "ready to start".into(),
        target: Target::Task(TaskId(task)),
        proposed: vec![],
        stale: false,
    }
}

#[test]
fn fake_attention_hides_and_restores_with_versions_and_changes() {
    let attention = FakeAttention::new();
    let changes = attention.changes();
    attention.set_items(vec![needs("a", 1), needs("b", 2), needs("c", 1)]);
    let list = attention.needs_me();
    assert_eq!((list.count, list.version), (3, 1));
    assert_eq!(attention.for_task(TaskId(1)).len(), 2);

    let b = NeedsKey("b".into());
    attention.snooze(&b, None).unwrap();
    assert_eq!(attention.needs_me().count, 2);
    attention.restore(&b).unwrap();
    let keys: Vec<String> = attention
        .needs_me()
        .items
        .iter()
        .map(|i| i.key.0.clone())
        .collect();
    assert_eq!(keys, ["a", "b", "c"], "back where it was");
    assert_eq!(attention.needs_me().version, 3);
    assert_eq!(changes.current(), 3);

    let err = attention.restore(&b).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    attention.dismiss(&NeedsKey("not-showing".into())).unwrap();
    assert_eq!(attention.needs_me().version, 3, "nothing visible changed");
}

#[test]
fn a_fake_clock_moves_only_when_told() {
    let clock = FakeClock::at(UNIX_EPOCH);
    clock.advance(Duration::from_secs(6 * 86_400));
    assert_eq!(clock.now(), UNIX_EPOCH + Duration::from_secs(6 * 86_400));
    clock.set(UNIX_EPOCH);
    assert_eq!(clock.now(), UNIX_EPOCH);
}
