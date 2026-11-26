//! In-app updates (SPEC-updates): `update_status`, `check_for_updates` and `install_update`.
//!
//! Groundwork stubs: the interface is fixed; the behaviour is SPEC-updates'.

use crate::dto::UpdateStatus;
use crate::errors::{AppError, AppResult, ErrorCode};
use crate::state::{AppState, Caller};

/// What the update card shows now.
pub fn update_status(_s: &AppState, _c: Caller) -> AppResult<UpdateStatus> {
    Ok(UpdateStatus::Idle)
}

/// Airboss › Check for Updates…: checks now, whatever the setting, and toasts the outcome.
pub fn check_for_updates(_s: &AppState, _c: Caller) -> AppResult<UpdateStatus> {
    Ok(UpdateStatus::Idle)
}

/// Update and restart: download, verify, swap, restart. Progress arrives as
/// `update:changed`; a failure becomes `failed` and still returns `Ok`.
pub fn install_update(_s: &AppState, _c: Caller) -> AppResult<()> {
    Err(AppError::new(ErrorCode::NotFound, "There's no update to install."))
}
