//! In-app updates (SPEC-updates): the [`Updater`] seam, its fake, the schedule, and the
//! status the sidebar's update card shows.
//!
//! The real updater is `native::TauriUpdater` (Tauri's updater plugin). Everything here runs
//! without Tauri, so the commands in `commands/updates.rs` and the automatic checker are
//! tested on [`FakeUpdater`].

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use tauri_plugin_log::log;

use crate::dto::UpdateStatus;
use crate::shell::UiEvent;
use crate::state::AppState;

/// The first automatic check, this long after start.
pub const FIRST_CHECK: Duration = Duration::from_secs(30);
/// Then one whenever this much wall-clock time has passed since the last.
pub const EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// How often the checker looks at the clock, so a sleeping Mac doesn't stretch the wait.
pub const TICK: Duration = Duration::from_secs(5 * 60);

/// A newer release the manifest offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
}

/// How an install is going, as [`Updater::install`] reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// Downloading: the whole percent once the size is known.
    Downloading(Option<u8>),
    /// The download finished: verifying it and swapping the app.
    Installing,
}

/// Finds and installs a new Airboss. Every method blocks; none is called with a lock held.
pub trait Updater: Send + Sync {
    /// An installed release build: inside an `.app`, not a debug build.
    fn available(&self) -> bool;
    /// Reads the manifest: the release newer than this one, if any. The error is one plain
    /// sentence for a toast or the card.
    fn check(&self) -> Result<Option<Release>, String>;
    /// Downloads `version`, verifies its signature and swaps the app in place, reporting
    /// progress. Doesn't restart.
    fn install(&self, version: &str, progress: &mut dyn FnMut(Progress)) -> Result<(), String>;
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a [`FakeUpdater`] answers.
#[derive(Debug)]
struct Script {
    available: bool,
    offer: Option<String>,
    check_error: Option<String>,
    install_error: Option<String>,
    steps: Vec<Progress>,
    held: bool,
    installing: bool,
    calls: Vec<String>,
}

/// An updater for tests: an installed build that's up to date, until a test says otherwise.
#[derive(Debug)]
pub struct FakeUpdater {
    script: Mutex<Script>,
    changed: Condvar,
}

impl Default for FakeUpdater {
    fn default() -> FakeUpdater {
        FakeUpdater {
            script: Mutex::new(Script {
                available: true,
                offer: None,
                check_error: None,
                install_error: None,
                steps: vec![Progress::Installing],
                held: false,
                installing: false,
                calls: Vec::new(),
            }),
            changed: Condvar::new(),
        }
    }
}

impl FakeUpdater {
    /// What `available` answers: `false` is a development build.
    pub fn set_available(&self, on: bool) {
        lock(&self.script).available = on;
    }

    /// The release `check` finds, or `None` for up to date.
    pub fn offer(&self, version: Option<&str>) {
        lock(&self.script).offer = version.map(str::to_string);
    }

    /// Makes `check` fail with this reason (or succeed again with `None`).
    pub fn fail_check(&self, reason: Option<&str>) {
        lock(&self.script).check_error = reason.map(str::to_string);
    }

    /// Makes `install` fail with this reason after its progress steps.
    pub fn fail_install(&self, reason: Option<&str>) {
        lock(&self.script).install_error = reason.map(str::to_string);
    }

    /// The progress `install` reports, in order.
    pub fn set_steps(&self, steps: Vec<Progress>) {
        lock(&self.script).steps = steps;
    }

    /// Holds `install` before its first step until [`release`](FakeUpdater::release).
    pub fn hold(&self) {
        lock(&self.script).held = true;
    }

    /// Lets a held `install` go on.
    pub fn release(&self) {
        lock(&self.script).held = false;
        self.changed.notify_all();
    }

    /// Waits until an `install` is running (held or not), up to `timeout`.
    pub fn wait_installing(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut s = lock(&self.script);
        while !s.installing {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            s = self
                .changed
                .wait_timeout(s, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }

    /// Every `check` and `install(version)` so far.
    pub fn calls(&self) -> Vec<String> {
        lock(&self.script).calls.clone()
    }
}

impl Updater for FakeUpdater {
    fn available(&self) -> bool {
        lock(&self.script).available
    }

    fn check(&self) -> Result<Option<Release>, String> {
        let mut s = lock(&self.script);
        s.calls.push("check".into());
        if let Some(e) = &s.check_error {
            return Err(e.clone());
        }
        Ok(s.offer.clone().map(|version| Release { version }))
    }

    fn install(&self, version: &str, progress: &mut dyn FnMut(Progress)) -> Result<(), String> {
        let (steps, error) = {
            let mut s = lock(&self.script);
            s.calls.push(format!("install({version})"));
            s.installing = true;
            self.changed.notify_all();
            while s.held {
                s = self.changed.wait(s).unwrap_or_else(PoisonError::into_inner);
            }
            (s.steps.clone(), s.install_error.clone())
        };
        for step in steps {
            progress(step);
        }
        lock(&self.script).installing = false;
        match error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// Whether the automatic checker checks now: [`FIRST_CHECK`] after `started`, then once
/// [`EVERY`] has passed since `last`. A clock set back before `last` checks once more and
/// starts over from there, rather than waiting out the difference.
pub fn due(now: SystemTime, started: SystemTime, last: Option<SystemTime>) -> bool {
    let since = |t: SystemTime| now.duration_since(t).ok();
    match last {
        None => since(started).is_none_or(|d| d >= FIRST_CHECK),
        Some(last) => since(last).is_none_or(|d| d >= EVERY),
    }
}

/// The updater plugin's error as one plain sentence for the card or a toast. The raw error
/// goes to the log (`native::TauriUpdater`), never to the screen.
pub fn plain_reason(e: &tauri_plugin_updater::Error) -> String {
    use tauri_plugin_updater::Error as E;
    match e {
        E::Minisign(_)
        | E::Base64(_)
        | E::SignatureUtf8(_)
        | E::SignedVersionMismatch { .. }
        | E::MissingSignedVersion => {
            "The download's signature didn't match, so it wasn't installed.".into()
        }
        E::Reqwest(r) if r.is_timeout() => "GitHub didn't answer in time.".into(),
        E::Reqwest(_) => "Couldn't reach GitHub.".into(),
        E::Network(_) => "The download didn't finish.".into(),
        E::ReleaseNotFound | E::Serialization(_) | E::Semver(_) => {
            "Couldn't read the list of releases.".into()
        }
        E::TargetNotFound(_) | E::TargetsNotFound(_) => {
            "There's no download for this Mac yet.".into()
        }
        E::PreviousAppNotRestored(at) => format!(
            "The update failed, and the old Airboss was left at {}.",
            at.display()
        ),
        E::Io(_)
        | E::TempDirNotOnSameMountPoint
        | E::TempDirNotFound
        | E::FailedToDetermineExtractPath
        | E::BinaryNotFoundInArchive
        | E::InvalidUpdaterFormat => "Couldn't replace Airboss where it's installed.".into(),
        _ => "The updater stopped with an error.".into(),
    }
}

/// The release's GitHub page, for "What's new".
pub fn notes_url(version: &str) -> String {
    format!("{}/releases/tag/v{version}", env!("CARGO_PKG_REPOSITORY"))
}

/// Whether an install is under way: no second one, and no check, until it ends.
pub fn busy(status: &UpdateStatus) -> bool {
    matches!(
        status,
        UpdateStatus::Downloading { .. }
            | UpdateStatus::Installing { .. }
            | UpdateStatus::Restarting { .. }
    )
}

/// The update status, when the checker last checked, and the checker's stop signal.
#[derive(Debug)]
pub(crate) struct Updates {
    inner: Mutex<Inner>,
    stop: Mutex<Option<Sender<()>>>,
}

#[derive(Debug)]
struct Inner {
    status: UpdateStatus,
    started: SystemTime,
    last: Option<SystemTime>,
}

impl Updates {
    /// Nothing to show yet; the schedule counts from `started`.
    pub(crate) fn new(started: SystemTime) -> Updates {
        Updates {
            inner: Mutex::new(Inner {
                status: UpdateStatus::Idle,
                started,
                last: None,
            }),
            stop: Mutex::new(None),
        }
    }

    /// Stops the checker thread (at quit and before a restart).
    pub(crate) fn stop(&self) {
        lock(&self.stop).take();
    }
}

impl AppState {
    /// What the update card shows now.
    pub fn update_status(&self) -> UpdateStatus {
        lock(&self.updates.inner).status.clone()
    }

    /// Changes the status and sends `update:changed` if it changed. The event goes out under
    /// the status lock, so the UI hears the changes in the order they happened.
    pub(crate) fn set_update(&self, status: UpdateStatus) {
        let mut inner = lock(&self.updates.inner);
        if inner.status != status {
            inner.status = status.clone();
            self.shell.emit(UiEvent::UpdateChanged(status));
        }
    }

    /// Moves to `next` only from a state `from` accepts, in one step under the lock: the
    /// guard that keeps two installs (or a check and an install) from crossing.
    pub(crate) fn update_if(
        &self,
        from: impl FnOnce(&UpdateStatus) -> Option<UpdateStatus>,
    ) -> Option<UpdateStatus> {
        let mut inner = lock(&self.updates.inner);
        let next = from(&inner.status)?;
        if inner.status != next {
            inner.status = next.clone();
            self.shell.emit(UiEvent::UpdateChanged(next.clone()));
        }
        Some(next)
    }

    /// Notes that a check ran now, for the schedule.
    pub(crate) fn checked_now(&self) {
        let now = self.now();
        lock(&self.updates.inner).last = Some(now);
    }

    /// The status after a check found `found`: `available` for a new release, `idle` when
    /// up to date. A failure for the same version stays until Try again or a manual check;
    /// an install under way is left alone.
    pub(crate) fn found_update(&self, found: Option<&Release>) {
        self.update_if(|now| {
            if busy(now) {
                return None;
            }
            Some(match found {
                Some(r) => match now {
                    UpdateStatus::Failed { version, .. } if *version == r.version => now.clone(),
                    _ => UpdateStatus::Available {
                        version: r.version.clone(),
                        notes_url: notes_url(&r.version),
                    },
                },
                None => UpdateStatus::Idle,
            })
        });
    }

    /// One look by the automatic checker: checks when it's [`due`], automatic checks are on,
    /// this is an installed release build, and no install is under way. A failure is only
    /// logged: being offline isn't news.
    pub fn update_tick(&self) {
        let now = self.now();
        let is_due = {
            let inner = lock(&self.updates.inner);
            due(now, inner.started, inner.last) && !busy(&inner.status)
        };
        if !is_due || !self.settings().check_for_updates {
            return;
        }
        let updater = self.shell.updater();
        if !updater.available() {
            return;
        }
        self.checked_now();
        match updater.check() {
            Ok(found) => {
                match &found {
                    Some(r) => log::info!("update check: Airboss {} is available", r.version),
                    None => log::info!("update check: up to date"),
                }
                self.found_update(found.as_ref());
            }
            Err(e) => log::warn!("update check failed: {e}"),
        }
    }

    /// Starts the automatic checker: a first look after `first`, then one every `tick`,
    /// until [`AppState::shutdown`]. Started with the app's other threads.
    pub fn start_update_checker(self: &Arc<Self>, first: Duration, tick: Duration) {
        let (tx, rx) = mpsc::channel::<()>();
        *lock(&self.updates.stop) = Some(tx);
        let me = Arc::downgrade(self);
        let spawned = std::thread::Builder::new()
            .name("airboss-updates".into())
            .spawn(move || {
                let mut wait = first;
                loop {
                    // Shutdown drops the sender: the wait ends at once.
                    match rx.recv_timeout(wait) {
                        Err(RecvTimeoutError::Timeout) => {}
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
                    }
                    let Some(s) = me.upgrade() else { return };
                    let looked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        s.update_tick();
                    }));
                    if looked.is_err() {
                        log::error!("the update check panicked");
                    }
                    wait = tick;
                }
            });
        if let Err(e) = spawned {
            log::error!("couldn't start the update checker: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn t(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + secs)
    }

    #[test]
    fn the_first_check_is_thirty_seconds_after_start() {
        let started = t(0);
        assert!(!due(t(0), started, None));
        assert!(!due(t(29), started, None));
        assert!(due(t(30), started, None));
        assert!(due(t(31), started, None));
        // A Mac that slept through start-up checks on the first look after.
        assert!(due(t(3 * 3600), started, None));
    }

    #[test]
    fn then_every_six_hours_of_wall_clock_time() {
        let started = t(0);
        let last = t(30);
        assert!(!due(t(31), started, Some(last)));
        assert!(!due(t(30 + 6 * 3600 - 1), started, Some(last)));
        assert!(due(t(30 + 6 * 3600), started, Some(last)));
        // Sleep doesn't stretch it: ten hours later is past due, on the next look.
        assert!(due(t(30 + 10 * 3600), started, Some(last)));
    }

    #[test]
    fn a_clock_set_back_checks_once_more_instead_of_waiting() {
        let started = t(1000);
        assert!(due(t(10), started, None));
        assert!(due(t(10), started, Some(t(1000))));
        // …and the next wait counts from that check.
        assert!(!due(t(11), started, Some(t(10))));
    }

    #[test]
    fn the_notes_are_the_release_page_on_github() {
        assert_eq!(
            notes_url("0.1.0-alpha.5"),
            "https://github.com/amantibrewal310/airboss/releases/tag/v0.1.0-alpha.5"
        );
    }

    #[test]
    fn plugin_errors_become_one_plain_sentence() {
        use tauri_plugin_updater::Error as E;
        assert_eq!(
            plain_reason(&E::SignatureUtf8("x".into())),
            "The download's signature didn't match, so it wasn't installed."
        );
        assert_eq!(
            plain_reason(&E::SignedVersionMismatch {
                signed: "1".into(),
                announced: "2".into()
            }),
            "The download's signature didn't match, so it wasn't installed."
        );
        assert_eq!(
            plain_reason(&E::ReleaseNotFound),
            "Couldn't read the list of releases."
        );
        assert_eq!(
            plain_reason(&E::Network("status 404".into())),
            "The download didn't finish."
        );
        assert_eq!(
            plain_reason(&E::TargetNotFound("darwin-aarch64".into())),
            "There's no download for this Mac yet."
        );
        assert_eq!(
            plain_reason(&E::Io(std::io::Error::other("denied"))),
            "Couldn't replace Airboss where it's installed."
        );
        assert_eq!(
            plain_reason(&E::PreviousAppNotRestored("/tmp/old/Airboss.app".into())),
            "The update failed, and the old Airboss was left at /tmp/old/Airboss.app."
        );
        // Nothing raw reaches the screen.
        assert_eq!(
            plain_reason(&E::EmptyEndpoints),
            "The updater stopped with an error."
        );
    }

    #[test]
    fn the_fake_plays_its_steps_then_fails_if_told_to() {
        let f = FakeUpdater::default();
        f.set_steps(vec![Progress::Downloading(Some(50)), Progress::Installing]);
        f.fail_install(Some("bad"));
        let mut seen = Vec::new();
        let r = f.install("1.0.0", &mut |p| seen.push(p));
        assert_eq!(r, Err("bad".into()));
        assert_eq!(
            seen,
            vec![Progress::Downloading(Some(50)), Progress::Installing]
        );
        assert_eq!(f.calls(), vec!["install(1.0.0)"]);
    }
}
