//! In-app updates (SPEC-updates): `update_status`, `check_for_updates` and `install_update`.

use tauri_plugin_log::log;

use crate::dto::{ToastArgs, ToastEvent, ToastEventAction, ToastTone, UpdateStatus};
use crate::errors::{AppError, AppResult, ErrorCode};
use crate::shell::UiEvent;
use crate::state::{AppState, Caller};
use crate::updates::{InstallError, Progress, busy};

/// What Check for Updates… says in a development build.
pub const MSG_NOT_INSTALLED: &str = "Updates work only in the installed Airboss app.";
/// `install_update` with nothing to install.
pub const MSG_NOTHING_TO_INSTALL: &str = "There's no update to install.";
/// The reason on the card when the updater itself panicked.
pub const MSG_UPDATER_BROKE: &str = "The updater stopped with an error.";

/// What the update card shows now.
pub fn update_status(s: &AppState, _c: Caller) -> AppResult<UpdateStatus> {
    Ok(s.update_status())
}

fn toast(s: &AppState, text: String, tone: ToastTone, actions: Vec<ToastEventAction>) {
    s.shell.emit(UiEvent::Toast(ToastEvent {
        text,
        actions,
        undo: None,
        tone: Some(tone),
        task: None,
    }));
}

/// Airboss › Check for Updates…: checks now, whatever the setting, and toasts the outcome.
/// While an install runs, the card already says everything, so it only answers the status.
pub fn check_for_updates(s: &AppState, _c: Caller) -> AppResult<UpdateStatus> {
    let updater = s.shell.updater();
    if !updater.available() {
        toast(s, MSG_NOT_INSTALLED.into(), ToastTone::Info, Vec::new());
        return Ok(s.update_status());
    }
    if busy(&s.update_status()) {
        return Ok(s.update_status());
    }
    s.checked_now();
    let checked = updater.check();
    // The outcome lands in one step under the status lock. An install that started while
    // this check ran owns the card by now: no toast then, only the status.
    let mut quiet = false;
    s.update_if(|now| {
        if busy(now) {
            quiet = true;
            return None;
        }
        match &checked {
            Err(_) => None,
            Ok(None) => Some(UpdateStatus::Idle),
            // Asked for by hand: a failure for this version gives way to Update and restart.
            Ok(Some(r)) => Some(UpdateStatus::Available {
                version: r.version.clone(),
                notes_url: crate::updates::notes_url(&r.version),
            }),
        }
    });
    if quiet {
        return Ok(s.update_status());
    }
    match checked {
        Err(reason) => {
            log::warn!("update check failed: {reason}");
            toast(
                s,
                format!("Couldn't check for updates: {reason}"),
                ToastTone::Warning,
                Vec::new(),
            );
        }
        Ok(None) => toast(
            s,
            format!(
                "You have the newest Airboss, {}.",
                env!("CARGO_PKG_VERSION")
            ),
            ToastTone::Info,
            Vec::new(),
        ),
        Ok(Some(release)) => toast(
            s,
            format!("Airboss {} is available.", release.version),
            ToastTone::Info,
            vec![ToastEventAction {
                label: "Update and restart".into(),
                command: "install_update".into(),
                args: ToastArgs::Empty {},
            }],
        ),
    }
    Ok(s.update_status())
}

/// Update and restart: download, verify, swap, restart. Progress arrives as
/// `update:changed`; a failure becomes `failed` and still returns `Ok`. While an install
/// runs, a second call does nothing.
pub fn install_update(s: &AppState, _c: Caller) -> AppResult<()> {
    // The guard: the move to `downloading` happens under the status lock, so of two calls
    // only one gets past here.
    let mut version = None;
    let mut running = false;
    s.update_if(|now| match now {
        UpdateStatus::Available { version: v, .. } | UpdateStatus::Failed { version: v, .. } => {
            version = Some(v.clone());
            Some(UpdateStatus::Downloading {
                version: v.clone(),
                percent: None,
            })
        }
        other => {
            running = busy(other);
            None
        }
    });
    let Some(version) = version else {
        return if running {
            Ok(())
        } else {
            Err(AppError::new(ErrorCode::NotFound, MSG_NOTHING_TO_INSTALL))
        };
    };
    log::info!("updating to Airboss {version}");

    let updater = s.shell.updater();
    let installed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        updater.install(&version, &mut |p| {
            let v = version.clone();
            s.set_update(match p {
                Progress::Downloading(percent) => UpdateStatus::Downloading {
                    version: v,
                    percent,
                },
                Progress::Installing => UpdateStatus::Installing { version: v },
            });
        })
    }))
    .unwrap_or_else(|_| Err(InstallError::Failed(MSG_UPDATER_BROKE.into())));

    match installed {
        Ok(()) => {
            log::info!("installed Airboss {version}; restarting");
            s.set_update(UpdateStatus::Restarting { version });
            // No lock of the state is held here: restart stops the state as quit does.
            s.shell.restart();
        }
        Err(InstallError::Failed(message)) => {
            log::warn!("couldn't update to Airboss {version}: {message}");
            s.set_update(UpdateStatus::Failed { version, message });
        }
        // The manifest moved on since the card was drawn: offer what it offers now, so the
        // next Update and restart installs that.
        Err(InstallError::NotOffered(Some(newer))) => {
            log::info!(
                "Airboss {version} is no longer offered; {} is",
                newer.version
            );
            s.set_update(UpdateStatus::Available {
                notes_url: crate::updates::notes_url(&newer.version),
                version: newer.version,
            });
        }
        Err(InstallError::NotOffered(None)) => {
            log::info!("Airboss {version} is no longer offered");
            s.set_update(UpdateStatus::Failed {
                message: format!("Airboss {version} isn't offered any more."),
                version,
            });
        }
    }
    Ok(())
}
