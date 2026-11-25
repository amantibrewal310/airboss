//! PR rows and views: what a GitHub read becomes in `observed_prs` and in the snapshot.

use std::time::SystemTime;

use airboss_core::{Link, PrState};
use airboss_services::time::to_ts;
use airboss_services::{Checks, PrProblem, PrRef, PrView, Review};

use crate::db::{PrError, PrRow};
use crate::gh::PrData;

/// The row a successful read makes. `ready_since` is kept from the old row while the PR
/// stays open and out of draft, set now when it first is, and cleared otherwise.
pub(crate) fn row_from(
    d: &PrData,
    old: Option<&PrRow>,
    account: &str,
    linked: bool,
    now: SystemTime,
) -> PrRow {
    let ready = d.state == PrState::Open && !d.draft;
    PrRow {
        pr: d.pr.as_str().to_string(),
        linked,
        url: d.url.clone(),
        title: d.title.clone(),
        state: Some(d.state),
        draft: d.draft,
        review: Some(d.review),
        checks: Some(d.checks),
        additions: d.additions,
        deletions: d.deletions,
        head_ref: d.head_ref.clone(),
        author: d.author.clone(),
        gh_updated_at: d.updated_at.clone(),
        merged_at: d.merged_at.clone(),
        ready_since: ready.then(|| {
            old.and_then(|o| o.ready_since.clone())
                .unwrap_or_else(|| to_ts(now))
        }),
        account: Some(account.to_string()),
        error: None,
        error_detail: None,
        checked_at: to_ts(now),
    }
}

/// The row a failed read makes, or `None` when GitHub already gave this PR a state: a
/// known state is kept (and shown stale) rather than replaced by an error.
pub(crate) fn error_row(
    pr: &PrRef,
    old: Option<&PrRow>,
    error: PrError,
    detail: Option<String>,
    account: Option<&str>,
    linked: bool,
    now: SystemTime,
) -> Option<PrRow> {
    if old.is_some_and(|o| o.state.is_some()) {
        return None;
    }
    Some(PrRow {
        pr: pr.as_str().to_string(),
        linked,
        url: old.and_then(|o| o.url.clone()),
        title: old.and_then(|o| o.title.clone()),
        state: None,
        draft: false,
        review: None,
        checks: None,
        additions: None,
        deletions: None,
        head_ref: None,
        author: None,
        gh_updated_at: None,
        merged_at: None,
        ready_since: None,
        account: account.map(str::to_string),
        error: Some(error),
        error_detail: detail,
        checked_at: to_ts(now),
    })
}

/// A row as the snapshot shows it.
pub(crate) fn view(row: &PrRow, stale: bool) -> Option<PrView> {
    Some(PrView {
        r#ref: PrRef::parse(&row.pr)?,
        linked: row.linked,
        url: row.url.clone(),
        title: row.title.clone(),
        state: row.state,
        draft: row.draft,
        review: row.review.unwrap_or(Review::None),
        checks: row.checks.unwrap_or(Checks::None),
        additions: row.additions,
        deletions: row.deletions,
        head_ref: row.head_ref.clone(),
        author: row.author.clone(),
        updated_at: row.gh_updated_at.clone(),
        ready_since: row.ready_since.clone(),
        problem: row.error.map(|e| match e {
            PrError::NotFound => PrProblem::NotFound,
            PrError::NoAccess => PrProblem::NoAccess,
            PrError::Failed => PrProblem::Failed(row.error_detail.clone().unwrap_or_default()),
        }),
        stale,
    })
}

/// A linked PR GitHub hasn't been asked about yet: what the link says, state unknown.
pub(crate) fn unread(link: &Link) -> Option<PrView> {
    Some(PrView {
        r#ref: PrRef::parse(&link.r#ref)?,
        linked: true,
        url: link.url.clone(),
        title: link.title.clone(),
        state: None,
        draft: false,
        review: Review::None,
        checks: Checks::None,
        additions: None,
        deletions: None,
        head_ref: None,
        author: None,
        updated_at: None,
        ready_since: None,
        problem: None,
        stale: true,
    })
}
