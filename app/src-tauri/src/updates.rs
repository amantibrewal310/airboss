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

/// How long the updater waits for GitHub, so a stalled connection can't hang the checker or
/// leave an install stuck in `downloading`. The download has only a read timeout, not a
/// total one, so a slow link still finishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// To open a connection.
    pub connect: Duration,
    /// Between two reads of a response, the download included.
    pub read: Duration,
    /// The whole manifest request.
    pub check: Duration,
}

/// The real limits: 20 s to connect, 30 s of silence, 30 s for the manifest.
pub const LIMITS: Limits = Limits {
    connect: Duration::from_secs(20),
    read: Duration::from_secs(30),
    check: Duration::from_secs(30),
};

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

/// Why an install didn't happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// It stopped before the swap: one plain sentence for the card.
    Failed(String),
    /// The manifest, read again for the install, no longer offers that version: what it
    /// offers now (a newer release, or nothing).
    NotOffered(Option<Release>),
}

/// Finds and installs a new Airboss. Every method blocks; none is called with a lock held.
pub trait Updater: Send + Sync {
    /// An installed release build: inside an `.app`, not a debug build.
    fn available(&self) -> bool;
    /// Reads the manifest: the release newer than this one, if any. The error is one plain
    /// sentence for a toast or the card.
    fn check(&self) -> Result<Option<Release>, String>;
    /// Reads the manifest afresh and, if it still offers `version`, downloads that, verifies
    /// its signature and swaps the app in place, reporting progress. Nothing from an earlier
    /// check is reused, so Try again after a fixed manifest gets the fixed release. Doesn't
    /// restart.
    fn install(
        &self,
        version: &str,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<(), InstallError>;
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
    panics: bool,
    held: bool,
    installing: bool,
    check_held: bool,
    checking: bool,
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
                panics: false,
                held: false,
                installing: false,
                check_held: false,
                checking: false,
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

    /// Makes `install` panic after its progress steps, as a bug in the updater would.
    pub fn panic_install(&self, on: bool) {
        lock(&self.script).panics = on;
    }

    /// Holds `check` until [`release_check`](FakeUpdater::release_check).
    pub fn hold_check(&self) {
        lock(&self.script).check_held = true;
    }

    /// Lets a held `check` answer.
    pub fn release_check(&self) {
        lock(&self.script).check_held = false;
        self.changed.notify_all();
    }

    /// Waits until a `check` is running (held or not), up to `timeout`.
    pub fn wait_checking(&self, timeout: Duration) -> bool {
        self.wait_until(timeout, |s| s.checking)
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
        self.wait_until(timeout, |s| s.installing)
    }

    fn wait_until(&self, timeout: Duration, f: impl Fn(&Script) -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        let mut s = lock(&self.script);
        while !f(&s) {
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
        s.checking = true;
        self.changed.notify_all();
        while s.check_held {
            s = self.changed.wait(s).unwrap_or_else(PoisonError::into_inner);
        }
        s.checking = false;
        if let Some(e) = &s.check_error {
            return Err(e.clone());
        }
        Ok(s.offer.clone().map(|version| Release { version }))
    }

    /// Like the real one, it goes by what's offered now, not by an earlier check.
    fn install(
        &self,
        version: &str,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<(), InstallError> {
        let (steps, error, panics) = {
            let mut s = lock(&self.script);
            s.calls.push(format!("install({version})"));
            let offer = s.offer.clone();
            still_offered(offer.as_deref(), version)?;
            s.installing = true;
            self.changed.notify_all();
            while s.held {
                s = self.changed.wait(s).unwrap_or_else(PoisonError::into_inner);
            }
            (s.steps.clone(), s.install_error.clone(), s.panics)
        };
        for step in steps {
            progress(step);
        }
        lock(&self.script).installing = false;
        if panics {
            panic!("the fake updater broke");
        }
        match error {
            Some(e) => Err(InstallError::Failed(e)),
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

/// Whether a fresh check that found `found` still offers `wanted`: the install goes on only
/// for exactly that version.
pub fn still_offered(found: Option<&str>, wanted: &str) -> Result<(), InstallError> {
    match found {
        Some(v) if v == wanted => Ok(()),
        other => Err(InstallError::NotOffered(other.map(|v| Release {
            version: v.to_string(),
        }))),
    }
}

/// Turns the plugin's download callbacks into whole-percent [`Progress`].
#[derive(Debug, Default)]
pub struct DownloadProgress {
    got: u64,
    shown: Option<u8>,
}

impl DownloadProgress {
    /// One chunk of `len` bytes, of `total` when the server said: the new percent, only when
    /// it changed (a few hundred events at most, not one per chunk).
    pub fn chunk(&mut self, len: usize, total: Option<u64>) -> Option<Progress> {
        self.got = self
            .got
            .saturating_add(u64::try_from(len).unwrap_or(u64::MAX));
        let percent = percent(self.got, total)?;
        if self.shown == Some(percent) {
            return None;
        }
        self.shown = Some(percent);
        Some(Progress::Downloading(Some(percent)))
    }
}

/// `got` of `total` bytes as a whole percent, at most 100; `None` without a size.
pub fn percent(got: u64, total: Option<u64>) -> Option<u8> {
    let total = total.filter(|t| *t > 0)?;
    let p = (u128::from(got) * 100 / u128::from(total)).min(100);
    Some(u8::try_from(p).unwrap_or(100))
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

/// The `.app` an executable runs from: `…/Airboss.app` for `…/Airboss.app/Contents/MacOS/x`.
pub fn bundle_of(exe: &std::path::Path) -> Option<std::path::PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let named = |p: &std::path::Path, n: &str| p.file_name().is_some_and(|f| f == n);
    (named(macos, "MacOS")
        && named(contents, "Contents")
        && bundle.extension().is_some_and(|e| e == "app"))
    .then(|| bundle.to_path_buf())
}

/// `/usr/bin/open`'s arguments to start the updated app at `bundle`:
///
/// - `-n`, a new instance (this one is on its way out), brought to the front (no `-g`): you
///   just chose Update and restart.
/// - `--env` for each `AIRBOSS_*` variable: LaunchServices starts an app with launchd's
///   environment, not ours, so without it a copy run on `AIRBOSS_DB` would come back on the
///   default database.
/// - This launch's arguments without `--hidden`, so an Airboss opened at login comes back with
///   its window.
pub fn relaunch_args(
    bundle: &std::path::Path,
    env: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
    args: &[std::ffi::OsString],
) -> Vec<std::ffi::OsString> {
    // Options first, then the bundle, then `--args`: `open` reads options only before the
    // operand.
    let mut out: Vec<std::ffi::OsString> = vec!["-n".into()];
    let mut vars: Vec<_> = env
        .into_iter()
        .filter(|(k, _)| k.to_string_lossy().starts_with("AIRBOSS_"))
        .collect();
    vars.sort();
    for (k, v) in vars {
        let mut kv = k;
        kv.push("=");
        kv.push(v);
        out.push("--env".into());
        out.push(kv);
    }
    out.push(bundle.as_os_str().to_owned());
    let rest = without_hidden(args);
    if !rest.is_empty() {
        out.push("--args".into());
        out.extend(rest);
    }
    out
}

/// This launch's arguments after the program, without `--hidden`.
fn without_hidden(args: &[std::ffi::OsString]) -> Vec<std::ffi::OsString> {
    args.iter()
        .skip(1)
        .filter(|a| *a != "--hidden")
        .cloned()
        .collect()
}

/// When `open` fails: the updated app's executable in `bundle` (by this executable's name)
/// and the arguments to start it with directly, without `--hidden`. Started as our child,
/// it inherits our environment, `AIRBOSS_*` included.
pub fn direct_relaunch(
    bundle: &std::path::Path,
    exe: &std::path::Path,
    args: &[std::ffi::OsString],
) -> Option<(std::path::PathBuf, Vec<std::ffi::OsString>)> {
    let name = exe.file_name()?;
    Some((
        bundle.join("Contents/MacOS").join(name),
        without_hidden(args),
    ))
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
    fn the_bundle_is_three_levels_up_from_an_app_executable() {
        use std::path::{Path, PathBuf};
        assert_eq!(
            bundle_of(Path::new(
                "/Applications/Airboss.app/Contents/MacOS/airboss-app"
            )),
            Some(PathBuf::from("/Applications/Airboss.app"))
        );
        assert_eq!(bundle_of(Path::new("/repo/target/debug/airboss-app")), None);
        assert_eq!(bundle_of(Path::new("/x/Contents/MacOS/airboss-app")), None);
    }

    #[test]
    fn the_relaunch_comes_forward_keeps_airboss_variables_and_drops_hidden() {
        use std::ffi::OsString;
        let os = |s: &str| OsString::from(s);
        let args = relaunch_args(
            std::path::Path::new("/Applications/Airboss.app"),
            [
                (os("HOME"), os("/Users/me")),
                (os("AIRBOSS_DB"), os("/tmp/qa/airboss.db")),
                (os("PATH"), os("/usr/bin")),
            ],
            &[
                os("/Applications/Airboss.app/Contents/MacOS/airboss-app"),
                os("--hidden"),
            ],
        );
        assert_eq!(
            args,
            // Options before the operand: `open` stops reading options at the bundle.
            [
                "-n",
                "--env",
                "AIRBOSS_DB=/tmp/qa/airboss.db",
                "/Applications/Airboss.app"
            ]
            .map(os)
        );
        let with_more = relaunch_args(
            std::path::Path::new("/A.app"),
            [(os("AIRBOSS_B"), os("2")), (os("AIRBOSS_A"), os("1"))],
            &[os("exe"), os("--hidden"), os("--other")],
        );
        assert_eq!(
            with_more,
            [
                "-n",
                "--env",
                "AIRBOSS_A=1",
                "--env",
                "AIRBOSS_B=2",
                "/A.app",
                "--args",
                "--other"
            ]
            .map(os)
        );
    }

    #[test]
    fn without_open_the_new_executable_starts_directly_without_hidden() {
        use std::ffi::OsString;
        use std::path::{Path, PathBuf};
        let os = |s: &str| OsString::from(s);
        let (exe, args) = direct_relaunch(
            Path::new("/Applications/Airboss.app"),
            Path::new("/Applications/Airboss.app/Contents/MacOS/airboss-app"),
            &[
                os("/Applications/Airboss.app/Contents/MacOS/airboss-app"),
                os("--hidden"),
                os("--x"),
            ],
        )
        .unwrap();
        assert_eq!(
            exe,
            PathBuf::from("/Applications/Airboss.app/Contents/MacOS/airboss-app")
        );
        assert_eq!(args, vec![os("--x")]);
        assert_eq!(
            direct_relaunch(Path::new("/A.app"), Path::new("/"), &[]),
            None
        );
    }

    #[test]
    fn the_percent_is_whole_capped_and_only_with_a_size() {
        assert_eq!(percent(0, Some(200)), Some(0));
        assert_eq!(percent(1, Some(200)), Some(0));
        assert_eq!(percent(2, Some(200)), Some(1));
        assert_eq!(percent(199, Some(200)), Some(99));
        assert_eq!(percent(200, Some(200)), Some(100));
        // A server that sent more than it said, or an absurd size, still ends at 100.
        assert_eq!(percent(300, Some(200)), Some(100));
        assert_eq!(percent(u64::MAX, Some(1)), Some(100));
        assert_eq!(percent(u64::MAX, Some(u64::MAX)), Some(100));
        assert_eq!(percent(10, None), None);
        assert_eq!(percent(10, Some(0)), None);
    }

    #[test]
    fn download_progress_reports_only_percent_changes() {
        let mut p = DownloadProgress::default();
        let d = |n| Some(Progress::Downloading(Some(n)));
        assert_eq!(p.chunk(10, Some(1000)), d(1));
        assert_eq!(p.chunk(5, Some(1000)), None); // still 1 %
        assert_eq!(p.chunk(5, Some(1000)), d(2));
        assert_eq!(p.chunk(500, Some(1000)), d(52));
        assert_eq!(p.chunk(480, Some(1000)), d(100));
        assert_eq!(p.chunk(1, Some(1000)), None);
        // No size: nothing to report beyond the command's own "downloading".
        let mut unknown = DownloadProgress::default();
        assert_eq!(unknown.chunk(4096, None), None);
    }

    #[test]
    fn an_install_goes_on_only_for_the_version_still_offered() {
        assert_eq!(
            still_offered(Some("0.1.0-alpha.5"), "0.1.0-alpha.5"),
            Ok(())
        );
        assert_eq!(
            still_offered(Some("0.1.0-alpha.6"), "0.1.0-alpha.5"),
            Err(InstallError::NotOffered(Some(Release {
                version: "0.1.0-alpha.6".into()
            })))
        );
        assert_eq!(
            still_offered(None, "0.1.0-alpha.5"),
            Err(InstallError::NotOffered(None))
        );
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
        f.offer(Some("1.0.0"));
        let r = f.install("1.0.0", &mut |p| seen.push(p));
        assert_eq!(r, Err(InstallError::Failed("bad".into())));
        assert_eq!(
            seen,
            vec![Progress::Downloading(Some(50)), Progress::Installing]
        );
        assert_eq!(f.calls(), vec!["install(1.0.0)"]);
    }
}
