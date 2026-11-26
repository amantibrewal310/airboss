//! In-app updates (SPEC-updates): the automatic checker, Check for Updates…, and Update and
//! restart, on `FakeUpdater` and `RecordingShell`. No network and no real updater.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use airboss_app::commands::updates::{
    self, MSG_NOT_INSTALLED, MSG_NOTHING_TO_INSTALL, MSG_UPDATER_BROKE,
};
use airboss_app::dto::{ToastEvent, ToastTone, UpdateStatus};
use airboss_app::errors::ErrorCode;
use airboss_app::settings::Settings;
use airboss_app::updates::{FakeUpdater, Progress};
use airboss_app::{Caller, UiEvent};
use common::TestApp;

const M: Caller = Caller::Main;
const NEXT: &str = "0.1.0-alpha.5";

fn fake(app: &TestApp) -> Arc<FakeUpdater> {
    app.shell.updater_fake()
}

fn available(version: &str) -> UpdateStatus {
    UpdateStatus::Available {
        version: version.into(),
        notes_url: format!("https://github.com/amantibrewal310/airboss/releases/tag/v{version}"),
    }
}

/// Every `update:changed` so far, in order.
fn changes(app: &TestApp) -> Vec<UpdateStatus> {
    app.shell
        .events()
        .into_iter()
        .filter_map(|e| match e {
            UiEvent::UpdateChanged(u) => Some(u),
            _ => None,
        })
        .collect()
}

fn toasts(app: &TestApp) -> Vec<ToastEvent> {
    app.shell
        .events()
        .into_iter()
        .filter_map(|e| match e {
            UiEvent::Toast(t) => Some(t),
            _ => None,
        })
        .collect()
}

fn checks_off(app: &TestApp) {
    let s = Settings {
        check_for_updates: false,
        ..app.state.settings()
    };
    app.call("set_settings", serde_json::json!({ "settings": s }))
        .unwrap();
}

/// An app whose status is `available` for [`NEXT`], from a manual check.
fn offered() -> TestApp {
    let app = TestApp::new();
    fake(&app).offer(Some(NEXT));
    updates::check_for_updates(&app.state, M).unwrap();
    app.shell.take_events();
    app
}

// ---------------------------------------------------------------------------- automatic

#[test]
fn the_automatic_check_waits_thirty_seconds_then_six_hours() {
    let app = TestApp::new();
    fake(&app).offer(Some(NEXT));
    app.state.update_tick();
    assert!(fake(&app).calls().is_empty(), "checked at start");
    app.clock.advance(Duration::from_secs(30));
    app.state.update_tick();
    assert_eq!(fake(&app).calls(), vec!["check"]);
    assert_eq!(app.state.update_status(), available(NEXT));
    assert_eq!(changes(&app), vec![available(NEXT)]);
    // A found release is the card, not a toast.
    assert!(toasts(&app).is_empty());

    app.clock.advance(Duration::from_secs(6 * 3600 - 1));
    app.state.update_tick();
    assert_eq!(fake(&app).calls().len(), 1);
    app.clock.advance(Duration::from_secs(1));
    app.state.update_tick();
    assert_eq!(fake(&app).calls().len(), 2);
    // The same release again sends nothing new.
    assert_eq!(changes(&app).len(), 1);
}

#[test]
fn the_automatic_check_respects_the_setting() {
    let app = TestApp::new();
    checks_off(&app);
    fake(&app).offer(Some(NEXT));
    app.clock.advance(Duration::from_secs(60));
    app.state.update_tick();
    assert!(fake(&app).calls().is_empty());
    assert_eq!(app.state.update_status(), UpdateStatus::Idle);
}

#[test]
fn the_automatic_check_runs_only_in_an_installed_release_build() {
    let app = TestApp::new();
    fake(&app).set_available(false);
    fake(&app).offer(Some(NEXT));
    app.clock.advance(Duration::from_secs(60));
    app.state.update_tick();
    assert!(fake(&app).calls().is_empty());
    assert!(app.shell.events().is_empty());
}

#[test]
fn an_automatic_check_that_fails_is_only_logged() {
    let app = TestApp::new();
    fake(&app).fail_check(Some("Couldn't reach GitHub."));
    app.clock.advance(Duration::from_secs(30));
    app.state.update_tick();
    assert_eq!(fake(&app).calls(), vec!["check"]);
    assert!(app.shell.events().is_empty(), "{:?}", app.shell.events());
    assert_eq!(app.state.update_status(), UpdateStatus::Idle);
    // …and it counts as a check: offline doesn't mean a check every five minutes.
    app.clock.advance(Duration::from_secs(300));
    app.state.update_tick();
    assert_eq!(fake(&app).calls().len(), 1);
}

#[test]
fn an_automatic_check_leaves_a_failure_for_the_same_version_but_offers_a_newer_one() {
    let app = offered();
    fake(&app).fail_install(Some("Couldn't reach GitHub."));
    updates::install_update(&app.state, M).unwrap();
    let failed = app.state.update_status();
    assert!(matches!(failed, UpdateStatus::Failed { .. }), "{failed:?}");
    app.clock.advance(Duration::from_secs(7 * 3600));
    app.state.update_tick();
    assert_eq!(app.state.update_status(), failed);
    fake(&app).offer(Some("0.1.0-alpha.6"));
    app.clock.advance(Duration::from_secs(7 * 3600));
    app.state.update_tick();
    assert_eq!(app.state.update_status(), available("0.1.0-alpha.6"));
}

#[test]
fn the_checker_thread_checks_on_its_tick_and_stops_at_shutdown() {
    let app = TestApp::new();
    fake(&app).offer(Some(NEXT));
    app.clock.advance(Duration::from_secs(30));
    app.state
        .start_update_checker(Duration::from_millis(10), Duration::from_millis(10));
    let found = app.shell.wait_for(Duration::from_secs(5), |e| {
        matches!(e, UiEvent::UpdateChanged(UpdateStatus::Available { .. }))
    });
    assert!(found.is_some());
    app.state.shutdown(Duration::from_millis(100));
    // Give a stray tick time to land, then make one due: nothing checks it.
    std::thread::sleep(Duration::from_millis(50));
    let before = fake(&app).calls().len();
    app.clock.advance(Duration::from_secs(7 * 3600));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(fake(&app).calls().len(), before);
}

// ---------------------------------------------------------------------------- manual

#[test]
fn check_for_updates_up_to_date_says_the_version() {
    let app = TestApp::new();
    let status = updates::check_for_updates(&app.state, M).unwrap();
    assert_eq!(status, UpdateStatus::Idle);
    let t = toasts(&app);
    assert_eq!(t.len(), 1);
    assert_eq!(
        t[0].text,
        format!(
            "You have the newest Airboss, {}.",
            env!("CARGO_PKG_VERSION")
        )
    );
    assert_eq!(t[0].tone, Some(ToastTone::Info));
    assert!(t[0].actions.is_empty());
}

#[test]
fn check_for_updates_found_offers_update_and_restart() {
    let app = TestApp::new();
    // A manual check ignores the setting.
    checks_off(&app);
    fake(&app).offer(Some(NEXT));
    let status = app
        .call("check_for_updates", serde_json::json!({}))
        .unwrap();
    assert_eq!(
        status,
        serde_json::json!({
            "state": "available",
            "version": NEXT,
            "notes_url": "https://github.com/amantibrewal310/airboss/releases/tag/v0.1.0-alpha.5"
        })
    );
    assert_eq!(changes(&app), vec![available(NEXT)]);
    let t = toasts(&app);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].text, "Airboss 0.1.0-alpha.5 is available.");
    assert_eq!(t[0].tone, Some(ToastTone::Info));
    // The UI runs it as `api.installUpdate(...Object.values(args))`.
    assert_eq!(
        serde_json::to_value(&t[0].actions).unwrap(),
        serde_json::json!([{ "label": "Update and restart", "command": "install_update", "args": {} }])
    );
}

#[test]
fn check_for_updates_that_fails_says_why() {
    let app = TestApp::new();
    fake(&app).fail_check(Some("Couldn't reach GitHub."));
    let status = updates::check_for_updates(&app.state, M).unwrap();
    assert_eq!(status, UpdateStatus::Idle);
    let t = toasts(&app);
    assert_eq!(t.len(), 1);
    assert_eq!(
        t[0].text,
        "Couldn't check for updates: Couldn't reach GitHub."
    );
    assert_eq!(t[0].tone, Some(ToastTone::Warning));
    assert!(changes(&app).is_empty());
}

#[test]
fn check_for_updates_in_a_development_build_says_where_updates_work() {
    let app = TestApp::new();
    fake(&app).set_available(false);
    fake(&app).offer(Some(NEXT));
    let status = updates::check_for_updates(&app.state, M).unwrap();
    assert_eq!(status, UpdateStatus::Idle);
    assert!(fake(&app).calls().is_empty());
    let t = toasts(&app);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].text, MSG_NOT_INSTALLED);
    assert_eq!(t[0].text, "Updates work only in the installed Airboss app.");
    assert_eq!(t[0].tone, Some(ToastTone::Info));
}

#[test]
fn check_for_updates_after_a_release_was_pulled_goes_back_to_idle() {
    let app = offered();
    fake(&app).offer(None);
    let status = updates::check_for_updates(&app.state, M).unwrap();
    assert_eq!(status, UpdateStatus::Idle);
    assert_eq!(changes(&app), vec![UpdateStatus::Idle]);
}

#[test]
fn update_status_answers_the_current_status() {
    let app = TestApp::new();
    assert_eq!(
        app.call("update_status", serde_json::json!({})).unwrap(),
        serde_json::json!({ "state": "idle" })
    );
    let app = offered();
    assert_eq!(
        updates::update_status(&app.state, M).unwrap(),
        available(NEXT)
    );
}

// ---------------------------------------------------------------------------- install

#[test]
fn install_update_downloads_installs_then_restarts() {
    let app = offered();
    fake(&app).set_steps(vec![
        Progress::Downloading(None),
        Progress::Downloading(Some(0)),
        Progress::Downloading(Some(42)),
        Progress::Downloading(Some(100)),
        Progress::Installing,
    ]);
    app.call("install_update", serde_json::json!({})).unwrap();
    let v = || NEXT.to_string();
    assert_eq!(
        changes(&app),
        vec![
            // The first report (no size yet) is what the command already said.
            UpdateStatus::Downloading {
                version: v(),
                percent: None
            },
            UpdateStatus::Downloading {
                version: v(),
                percent: Some(0)
            },
            UpdateStatus::Downloading {
                version: v(),
                percent: Some(42)
            },
            UpdateStatus::Downloading {
                version: v(),
                percent: Some(100)
            },
            UpdateStatus::Installing { version: v() },
            UpdateStatus::Restarting { version: v() },
        ]
    );
    assert_eq!(fake(&app).calls(), vec!["check", "install(0.1.0-alpha.5)"]);
    assert_eq!(app.shell.calls(), vec!["restart"]);
    assert!(toasts(&app).is_empty());
}

#[test]
fn a_failed_install_says_why_on_the_card_and_doesnt_restart() {
    let app = offered();
    fake(&app).set_steps(vec![Progress::Downloading(Some(100)), Progress::Installing]);
    fake(&app).fail_install(Some(
        "The download's signature didn't match, so it wasn't installed.",
    ));
    // Still `Ok`: the card is the only place that says so.
    updates::install_update(&app.state, M).unwrap();
    let failed = UpdateStatus::Failed {
        version: NEXT.into(),
        message: "The download's signature didn't match, so it wasn't installed.".into(),
    };
    assert_eq!(changes(&app).last(), Some(&failed));
    assert_eq!(app.state.update_status(), failed);
    assert!(!app.shell.calls().contains(&"restart".to_string()));
    assert!(toasts(&app).is_empty());

    // Try again installs the same version.
    fake(&app).fail_install(None);
    updates::install_update(&app.state, M).unwrap();
    assert_eq!(
        app.state.update_status(),
        UpdateStatus::Restarting {
            version: NEXT.into()
        }
    );
    assert_eq!(app.shell.calls(), vec!["restart"]);
}

#[test]
fn a_second_install_while_one_runs_does_nothing() {
    let app = offered();
    fake(&app).hold();
    let state = app.state.clone();
    let first = std::thread::spawn(move || updates::install_update(&state, M));
    assert!(fake(&app).wait_installing(Duration::from_secs(5)));

    updates::install_update(&app.state, M).unwrap();
    // A check meanwhile doesn't check or change anything either.
    updates::check_for_updates(&app.state, M).unwrap();
    app.clock.advance(Duration::from_secs(7 * 3600));
    app.state.update_tick();
    assert_eq!(fake(&app).calls(), vec!["check", "install(0.1.0-alpha.5)"]);
    assert_eq!(
        app.state.update_status(),
        UpdateStatus::Downloading {
            version: NEXT.into(),
            percent: None
        }
    );

    fake(&app).release();
    first.join().unwrap().unwrap();
    assert_eq!(app.shell.calls(), vec!["restart"]);
    let installs = fake(&app)
        .calls()
        .iter()
        .filter(|c| c.starts_with("install"))
        .count();
    assert_eq!(installs, 1);
}

#[test]
fn install_update_with_nothing_to_install_is_not_found() {
    let app = TestApp::new();
    let e = updates::install_update(&app.state, M).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    assert_eq!(e.message, MSG_NOTHING_TO_INSTALL);
    assert_eq!(e.message, "There's no update to install.");
    assert!(fake(&app).calls().is_empty());
    assert!(app.shell.calls().is_empty());
}

// ---------------------------------------------------------------------------- round 2

#[test]
fn a_manual_check_restarts_the_six_hour_wait() {
    let app = TestApp::new();
    app.clock.advance(Duration::from_secs(30));
    app.state.update_tick();
    assert_eq!(fake(&app).calls(), vec!["check"]);
    app.clock.advance(Duration::from_secs(5 * 3600));
    updates::check_for_updates(&app.state, M).unwrap();
    assert_eq!(fake(&app).calls().len(), 2);
    // Six hours after the automatic check, but two after the manual one: not yet.
    app.clock.advance(Duration::from_secs(2 * 3600));
    app.state.update_tick();
    assert_eq!(fake(&app).calls().len(), 2);
    app.clock.advance(Duration::from_secs(4 * 3600));
    app.state.update_tick();
    assert_eq!(fake(&app).calls().len(), 3);
}

#[test]
fn a_manual_check_offers_again_a_version_that_failed() {
    let app = offered();
    fake(&app).fail_install(Some("Couldn't reach GitHub."));
    updates::install_update(&app.state, M).unwrap();
    assert!(matches!(
        app.state.update_status(),
        UpdateStatus::Failed { .. }
    ));
    app.shell.take_events();
    // An automatic check leaves the failure (Try again is on the card)…
    app.clock.advance(Duration::from_secs(7 * 3600));
    app.state.update_tick();
    assert!(changes(&app).is_empty());
    // …but asked for by hand, it's Update and restart again, with the toast.
    let status = updates::check_for_updates(&app.state, M).unwrap();
    assert_eq!(status, available(NEXT));
    assert_eq!(changes(&app), vec![available(NEXT)]);
    assert_eq!(toasts(&app)[0].text, "Airboss 0.1.0-alpha.5 is available.");
}

#[test]
fn an_automatic_check_after_a_failure_with_nothing_offered_goes_idle() {
    let app = offered();
    fake(&app).fail_install(Some("Couldn't reach GitHub."));
    updates::install_update(&app.state, M).unwrap();
    fake(&app).offer(None);
    app.shell.take_events();
    app.clock.advance(Duration::from_secs(7 * 3600));
    app.state.update_tick();
    assert_eq!(app.state.update_status(), UpdateStatus::Idle);
    assert_eq!(changes(&app), vec![UpdateStatus::Idle]);
    assert!(toasts(&app).is_empty());
}

#[test]
fn an_updater_that_panics_leaves_a_failure_not_a_stuck_download() {
    let app = offered();
    fake(&app).panic_install(true);
    updates::install_update(&app.state, M).unwrap();
    assert_eq!(
        app.state.update_status(),
        UpdateStatus::Failed {
            version: NEXT.into(),
            message: MSG_UPDATER_BROKE.into()
        }
    );
    assert_eq!(MSG_UPDATER_BROKE, "The updater stopped with an error.");
    assert!(!app.shell.calls().contains(&"restart".to_string()));
    // Try again isn't blocked by a busy status.
    fake(&app).panic_install(false);
    updates::install_update(&app.state, M).unwrap();
    assert_eq!(app.shell.calls(), vec!["restart"]);
}

#[test]
fn try_again_after_the_manifest_moved_on_offers_the_newer_release() {
    // The QA finding: Try again must go by the manifest now, not by an earlier check.
    let app = offered();
    fake(&app).fail_install(Some(
        "The download's signature didn't match, so it wasn't installed.",
    ));
    updates::install_update(&app.state, M).unwrap();
    fake(&app).fail_install(None);
    fake(&app).offer(Some("0.1.0-alpha.6"));
    app.shell.take_events();
    updates::install_update(&app.state, M).unwrap();
    assert_eq!(
        changes(&app),
        vec![
            UpdateStatus::Downloading {
                version: NEXT.into(),
                percent: None
            },
            available("0.1.0-alpha.6"),
        ]
    );
    assert!(app.shell.calls().is_empty(), "restarted");
    // The next Update and restart installs that one.
    updates::install_update(&app.state, M).unwrap();
    assert_eq!(
        app.state.update_status(),
        UpdateStatus::Restarting {
            version: "0.1.0-alpha.6".into()
        }
    );
    assert_eq!(fake(&app).calls().last().unwrap(), "install(0.1.0-alpha.6)");
}

#[test]
fn an_install_of_a_release_that_was_pulled_says_so_on_the_card() {
    let app = offered();
    fake(&app).offer(None);
    updates::install_update(&app.state, M).unwrap();
    assert_eq!(
        app.state.update_status(),
        UpdateStatus::Failed {
            version: NEXT.into(),
            message: "Airboss 0.1.0-alpha.5 isn't offered any more.".into()
        }
    );
    assert!(app.shell.calls().is_empty());
}

/// A manual check is held in `check` while an install runs to the end, then answers `answer`.
fn check_across_an_install(answer: impl FnOnce(&FakeUpdater)) -> (TestApp, UpdateStatus) {
    let app = offered();
    fake(&app).hold_check();
    let state = app.state.clone();
    let check = std::thread::spawn(move || updates::check_for_updates(&state, M));
    assert!(fake(&app).wait_checking(Duration::from_secs(5)));
    updates::install_update(&app.state, M).unwrap();
    answer(&fake(&app));
    app.shell.take_events();
    fake(&app).release_check();
    let status = check.join().unwrap().unwrap();
    (app, status)
}

#[test]
fn a_manual_check_that_ends_after_an_install_began_says_nothing() {
    let restarting = UpdateStatus::Restarting {
        version: NEXT.into(),
    };
    let (app, status) = check_across_an_install(|f| f.offer(None));
    assert_eq!(status, restarting);
    assert!(app.shell.events().is_empty(), "{:?}", app.shell.events());

    let (app, status) = check_across_an_install(|f| f.fail_check(Some("Couldn't reach GitHub.")));
    assert_eq!(status, restarting);
    assert!(app.shell.events().is_empty(), "{:?}", app.shell.events());

    let (app, status) = check_across_an_install(|_| {});
    assert_eq!(status, restarting);
    assert!(app.shell.events().is_empty(), "{:?}", app.shell.events());
}
