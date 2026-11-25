//! Rule 6 (who may move a task where), rule 1 (blocked needs a reason) and rule 7.
#![allow(clippy::unwrap_used)]

mod common;

use airboss_core::{
    Actor, Error, EventKind, LinkKind, NewLink, NewTask, PrState, Status, Store, TaskId,
};
use common::agent;

use Status::*;

const ALL: [Status; 7] = [Proposed, Todo, Doing, InReview, Blocked, Done, Dropped];
const OPEN: [Status; 4] = [Todo, Doing, InReview, Blocked];
const CLOSED: [Status; 3] = [Proposed, Done, Dropped];

/// A fresh store holding one task in `from`, with one PR link per entry of `prs`.
fn task_in(from: Status, prs: &[PrState]) -> (Store, TaskId) {
    let mut s = Store::open_in_memory().unwrap();
    let creator = if from == Proposed { agent() } else { Actor::Me };
    let id = s.add_task(NewTask::new("t"), &creator).unwrap().id;
    for (i, &state) in prs.iter().enumerate() {
        s.link(
            id,
            NewLink::pr(format!("acme/web#{i}")).with_pr_state(state),
            &Actor::Me,
        )
        .unwrap();
    }
    if from != Proposed && from != Todo {
        s.set_status(id, from, Some("setup"), &Actor::Me).unwrap();
    }
    assert_eq!(s.task(id).unwrap().status, from);
    (s, id)
}

/// Checks one case: the change lands with a `status` event (none if it changes nothing),
/// or fails with `expected` and leaves the task and its history untouched.
fn case(
    from: Status,
    prs: &[PrState],
    actor: &Actor,
    to: Status,
    note: Option<&str>,
    expected: Result<(), fn(&Error) -> bool>,
) {
    let (mut s, id) = task_in(from, prs);
    let before = (s.task(id).unwrap(), s.history(id).unwrap());
    let got = s.set_status(id, to, note, actor);
    let label = format!("{actor}: {from} -> {to}, prs {prs:?}, note {note:?}");
    match expected {
        Ok(()) => {
            let task = got.unwrap_or_else(|e| panic!("{label}: expected ok, got {e:?}"));
            assert_eq!(task.status, to, "{label}");
            assert_eq!(s.task(id).unwrap(), task, "{label}");
            let history = s.history(id).unwrap();
            let reason = if to == Blocked { note } else { None };
            if before.0.status == to && before.0.blocked_reason.as_deref() == reason {
                assert_eq!(history, before.1, "{label}: a no-op writes nothing");
                return;
            }
            assert_eq!(history.len(), before.1.len() + 1, "{label}");
            let last = history.last().unwrap();
            assert_eq!(
                (last.kind, last.to_status, last.actor.clone()),
                (EventKind::Status, Some(to), actor.clone()),
                "{label}"
            );
        }
        Err(is_expected) => {
            let err = got.map(|_| ()).unwrap_err();
            assert!(is_expected(&err), "{label}: got {err:?}");
            assert_eq!(
                (s.task(id).unwrap(), s.history(id).unwrap()),
                before,
                "{label}"
            );
        }
    }
}

fn not_allowed(e: &Error) -> bool {
    matches!(e, Error::NotAllowed)
}
fn invalid_transition(e: &Error) -> bool {
    matches!(e, Error::InvalidTransition)
}
fn note_required(e: &Error) -> bool {
    matches!(e, Error::NoteRequired)
}

/// Blocked always needs a reason (rule 1), so every case into it carries one.
fn note_for(to: Status) -> Option<&'static str> {
    (to == Blocked).then_some("waiting on API keys")
}

// Row 1: `me`, any -> any except `proposed`. Reopening done or dropped is allowed.

#[test]
fn row_me_moves_any_task_anywhere_but_proposed() {
    for from in ALL {
        for to in ALL {
            let expected = if to == Proposed {
                Err(not_allowed as _)
            } else {
                Ok(())
            };
            case(from, &[], &Actor::Me, to, note_for(to), expected);
        }
    }
}

// Row 2: an agent, open -> doing, in_review, blocked.

#[test]
fn row_agent_moves_open_work_between_doing_review_and_blocked() {
    for from in OPEN {
        for to in [Doing, InReview, Blocked] {
            case(from, &[PrState::Open], &agent(), to, note_for(to), Ok(()));
        }
    }
}

// Row 3: an agent, open -> done, only with no PR and with a note.

#[test]
fn row_agent_finishes_pr_less_work_with_a_note() {
    for from in OPEN {
        case(from, &[], &agent(), Done, Some("fixed the typo"), Ok(()));
    }
}

#[test]
fn agent_cannot_finish_without_a_note() {
    for note in [None, Some(""), Some("   ")] {
        case(Doing, &[], &agent(), Done, note, Err(note_required));
    }
}

#[test]
fn agent_cannot_finish_task_with_pr() {
    for state in [PrState::Open, PrState::Merged, PrState::Closed] {
        case(
            Doing,
            &[state],
            &agent(),
            Done,
            Some("done"),
            Err(not_allowed),
        );
    }
}

// Row 4: `sync:gh`, open -> done when it has PRs, none open, at least one merged.

#[test]
fn row_sync_gh_finishes_when_prs_have_merged() {
    for from in OPEN {
        case(from, &[PrState::Merged], &Actor::SyncGh, Done, None, Ok(()));
    }
    case(
        InReview,
        &[PrState::Merged, PrState::Closed],
        &Actor::SyncGh,
        Done,
        None,
        Ok(()),
    );
}

#[test]
fn sync_gh_cannot_finish_without_a_merged_pr_or_with_one_still_open() {
    for prs in [
        &[][..],
        &[PrState::Open],
        &[PrState::Closed],
        &[PrState::Closed, PrState::Closed],
        &[PrState::Merged, PrState::Open],
    ] {
        case(InReview, prs, &Actor::SyncGh, Done, None, Err(not_allowed));
    }
}

// Row 5: `launch`, todo or blocked -> doing.

#[test]
fn row_launch_starts_todo_or_blocked_work() {
    for from in [Todo, Blocked] {
        case(from, &[], &Actor::Launch, Doing, None, Ok(()));
    }
}

// Row 6: `sync:t3` and `plan` never change status.

#[test]
fn row_sync_t3_and_plan_never_change_status() {
    for actor in [Actor::SyncT3, Actor::Plan] {
        for from in ALL {
            for to in ALL {
                case(from, &[], &actor, to, note_for(to), Err(not_allowed));
            }
        }
    }
}

// Rejected: right target, wrong starting status, is `InvalidTransition`.

#[test]
fn agent_cannot_move_proposed_done_or_dropped_tasks() {
    for from in CLOSED {
        for to in [Doing, InReview, Blocked, Done] {
            let note = note_for(to).or(Some("done"));
            case(from, &[], &agent(), to, note, Err(invalid_transition));
        }
    }
}

#[test]
fn sync_gh_cannot_finish_proposed_done_or_dropped_tasks() {
    for from in CLOSED {
        case(
            from,
            &[PrState::Merged],
            &Actor::SyncGh,
            Done,
            None,
            Err(invalid_transition),
        );
    }
}

#[test]
fn launch_starts_only_todo_or_blocked_work() {
    for from in [Proposed, Doing, InReview, Done, Dropped] {
        case(
            from,
            &[],
            &Actor::Launch,
            Doing,
            None,
            Err(invalid_transition),
        );
    }
}

// Rejected: a target the actor may never set is `NotAllowed`, from any status.

#[test]
fn agent_cannot_set_todo_proposed_or_dropped() {
    for from in ALL {
        for to in [Todo, Proposed, Dropped] {
            case(from, &[], &agent(), to, Some("why"), Err(not_allowed));
        }
    }
}

#[test]
fn sync_gh_only_ever_sets_done() {
    for from in ALL {
        for to in [Proposed, Todo, Doing, InReview, Blocked, Dropped] {
            case(
                from,
                &[PrState::Merged],
                &Actor::SyncGh,
                to,
                note_for(to),
                Err(not_allowed),
            );
        }
    }
}

#[test]
fn launch_only_ever_sets_doing() {
    for from in ALL {
        for to in [Proposed, Todo, InReview, Blocked, Done, Dropped] {
            case(
                from,
                &[],
                &Actor::Launch,
                to,
                note_for(to),
                Err(not_allowed),
            );
        }
    }
}

#[test]
fn an_agent_with_an_empty_thread_ref_cannot_change_status() {
    case(
        Todo,
        &[],
        &Actor::Agent(" ".into()),
        Doing,
        None,
        Err(not_allowed),
    );
}

#[test]
fn setting_status_on_a_missing_task_is_not_found() {
    let mut s = Store::open_in_memory().unwrap();
    let err = s
        .set_status(TaskId(4), Doing, None, &Actor::Me)
        .unwrap_err();
    assert!(matches!(err, Error::NotFound), "{err:?}");
}

// Rule 1: blocked has a reason, and nothing else does.

#[test]
fn blocking_needs_a_reason() {
    for actor in [Actor::Me, agent()] {
        for note in [None, Some(" ")] {
            case(Doing, &[], &actor, Blocked, note, Err(note_required));
        }
    }
}

#[test]
fn the_note_becomes_the_blocked_reason_and_leaving_blocked_clears_it() {
    let (mut s, id) = task_in(Doing, &[]);
    let t = s
        .set_status(id, Blocked, Some("waiting on keys"), &agent())
        .unwrap();
    assert_eq!(t.blocked_reason.as_deref(), Some("waiting on keys"));
    let last = s.history(id).unwrap().pop().unwrap();
    assert_eq!(last.body.as_deref(), Some("waiting on keys"));

    let t = s
        .set_status(id, Doing, Some("keys arrived"), &agent())
        .unwrap();
    assert_eq!(t.blocked_reason, None);
    let last = s.history(id).unwrap().pop().unwrap();
    assert_eq!(last.body.as_deref(), Some("keys arrived"));
}

// Rule 7: a PR linked to a done task by anyone but `me` reopens it for review.

fn reopen_case(by: &Actor, link: NewLink, expected: Status) {
    let (mut s, id) = task_in(Done, &[]);
    let events_before = s.history(id).unwrap().len();
    s.link(id, link, by).unwrap();
    assert_eq!(s.task(id).unwrap().status, expected, "{by}");
    let new: Vec<_> = s.history(id).unwrap()[events_before..]
        .iter()
        .map(|e| (e.kind, e.to_status, e.actor.clone()))
        .collect();
    let mut want = vec![(EventKind::Linked, None, by.clone())];
    if expected == InReview {
        want.push((EventKind::Status, Some(InReview), by.clone()));
    }
    assert_eq!(new, want, "{by}");
}

#[test]
fn a_pr_linked_to_a_done_task_by_anyone_but_me_reopens_it_for_review() {
    for by in [
        agent(),
        Actor::SyncGh,
        Actor::SyncT3,
        Actor::Plan,
        Actor::Launch,
    ] {
        reopen_case(&by, NewLink::pr("acme/web#7"), InReview);
    }
}

#[test]
fn a_pr_linked_to_a_done_task_by_me_leaves_it_done() {
    reopen_case(&Actor::Me, NewLink::pr("acme/web#7"), Done);
}

#[test]
fn a_non_pr_link_to_a_done_task_leaves_it_done() {
    reopen_case(&agent(), NewLink::new(LinkKind::Doc, "notes.md"), Done);
}

#[test]
fn a_pr_linked_to_open_work_leaves_its_status_alone() {
    let (mut s, id) = task_in(Doing, &[]);
    s.link(id, NewLink::pr("acme/web#7"), &agent()).unwrap();
    assert_eq!(s.task(id).unwrap().status, Doing);
}

// Rule 21's exception: a PR event that reopens a task records the link and the reopen as
// two events, so history shows both.

#[test]
fn a_reopening_link_writes_the_link_and_the_reopen_as_two_events_in_that_order() {
    let (mut s, id) = task_in(Done, &[]);
    let rx = s.subscribe();
    s.link(id, NewLink::pr("acme/web#7"), &Actor::SyncGh)
        .unwrap();

    let ids = rx.try_recv().unwrap();
    let history = s.history(id).unwrap();
    let new = &history[history.len() - 2..];
    assert_eq!(ids, new.iter().map(|e| e.id).collect::<Vec<_>>());
    let got: Vec<_> = new
        .iter()
        .map(|e| (e.kind, e.to_status, e.actor.clone()))
        .collect();
    assert_eq!(
        got,
        vec![
            (EventKind::Linked, None, Actor::SyncGh),
            (EventKind::Status, Some(InReview), Actor::SyncGh),
        ]
    );
}

// Rule 7: only an open PR reopens a done task, and a PR reopened on GitHub reopens it.

#[test]
fn a_merged_or_closed_pr_linked_to_a_done_task_leaves_it_done() {
    for state in [PrState::Merged, PrState::Closed] {
        reopen_case(
            &Actor::SyncGh,
            NewLink::pr("acme/web#7").with_pr_state(state),
            Done,
        );
    }
}

#[test]
fn a_pr_reopened_on_github_reopens_its_done_task_for_review() {
    let (mut s, id) = task_in(Doing, &[]);
    s.link(id, NewLink::pr("acme/web#7"), &Actor::SyncGh)
        .unwrap();
    s.set_pr_state("acme/web#7", PrState::Merged, &Actor::SyncGh)
        .unwrap();
    s.set_status(id, Done, None, &Actor::SyncGh).unwrap();
    let events_before = s.history(id).unwrap().len();

    let changed = s
        .set_pr_state("acme/web#7", PrState::Open, &Actor::SyncGh)
        .unwrap();

    assert_eq!(changed, vec![id]);
    assert_eq!(s.task(id).unwrap().status, InReview);
    let new: Vec<_> = s.history(id).unwrap()[events_before..]
        .iter()
        .map(|e| (e.kind, e.to_status, e.actor.clone()))
        .collect();
    assert_eq!(
        new,
        vec![
            (EventKind::LinkState, None, Actor::SyncGh),
            (EventKind::Status, Some(InReview), Actor::SyncGh),
        ]
    );
}

#[test]
fn a_pr_reopened_on_github_leaves_open_work_alone() {
    let (mut s, id) = task_in(Doing, &[PrState::Closed]);
    s.set_pr_state("acme/web#0", PrState::Open, &Actor::SyncGh)
        .unwrap();
    assert_eq!(s.task(id).unwrap().status, Doing);
    assert_eq!(
        s.history(id).unwrap().last().unwrap().kind,
        EventKind::LinkState
    );
}

// Rule 21: a status change that changes nothing writes nothing.

#[test]
fn setting_the_status_a_task_already_has_writes_no_event() {
    for (from, note) in [
        (Todo, None),
        (Doing, Some("still at it")),
        (Done, None),
        (Blocked, Some("setup")),
    ] {
        let (mut s, id) = task_in(from, &[]);
        let before = s.history(id).unwrap();
        let rx = s.subscribe();
        let t = s.set_status(id, from, note, &Actor::Me).unwrap();
        assert_eq!(t.status, from);
        assert_eq!(s.history(id).unwrap(), before, "{from}");
        assert!(rx.try_recv().is_err(), "{from}: no notification");
    }
}

#[test]
fn blocking_again_with_a_new_reason_is_a_change() {
    let (mut s, id) = task_in(Blocked, &[]);
    let before = s.history(id).unwrap().len();
    let t = s
        .set_status(id, Blocked, Some("new reason"), &Actor::Me)
        .unwrap();
    assert_eq!(t.blocked_reason.as_deref(), Some("new reason"));
    assert_eq!(s.history(id).unwrap().len(), before + 1);
}
