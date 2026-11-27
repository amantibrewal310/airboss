//! Sample data: DESIGN's tasks (the prototype's, with its ids), and a large tree for timings.
//! Used by the fixtures recorder, the perf tests and `examples/seed.rs`; never by the app.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use airboss_core::{Actor, LinkKind, NewLink, NewTask, PrState, RepoId, Status, Store, TaskId};

/// What the design data seeded: each task's age in the design ("6m", "2d" …), for the fixed
/// clock the fixtures use, and the proposals' ids.
#[derive(Debug, Clone, Default)]
pub struct Design {
    pub ages: HashMap<TaskId, Duration>,
    pub proposals: Vec<TaskId>,
}

const MIN: u64 = 60;
const HOUR: u64 = 3600;
const DAY: u64 = 86_400;

/// One task of the prototype's sample data.
struct Row {
    id: i64,
    title: &'static str,
    status: Status,
    parent: Option<i64>,
    repos: &'static [&'static str],
    age: u64,
}

const fn row(
    id: i64,
    title: &'static str,
    status: Status,
    parent: Option<i64>,
    repos: &'static [&'static str],
    age: u64,
) -> Row {
    Row {
        id,
        title,
        status,
        parent,
        repos,
        age,
    }
}

use Status::{Blocked, Doing, Done, InReview, Todo};

/// The prototype's tasks (docs/design/prototype.html), in id order so each parent exists first.
/// A parent's status is worked out from its steps (SPEC-core rule 10), so its own is `Todo`
/// and never set. Dark mode tokens keeps one step open, so it shows in progress rather than
/// the prototype's "Finish it?".
const ROWS: &[Row] = &[
    row(98, "New laptop setup", Done, None, &["dotfiles"], 14 * DAY),
    row(110, "Dark mode tokens", Todo, None, &["booking-web"], HOUR),
    row(
        111,
        "Colour tokens",
        Done,
        Some(110),
        &["booking-web"],
        3 * DAY,
    ),
    row(
        112,
        "Swap hard-coded colours",
        Done,
        Some(110),
        &["booking-web"],
        3 * DAY,
    ),
    row(
        113,
        "Dark screenshots",
        Done,
        Some(110),
        &["booking-web"],
        3 * DAY,
    ),
    row(
        114,
        "Contrast check",
        Todo,
        Some(110),
        &["booking-web"],
        3 * DAY,
    ),
    row(
        120,
        "Payments refactor stack",
        InReview,
        None,
        &["booking-api", "booking-web"],
        6 * DAY,
    ),
    row(128, "MCP server", InReview, None, &["airboss"], 40 * MIN),
    row(
        131,
        "Fix flaky login test",
        Doing,
        None,
        &["booking-web"],
        HOUR,
    ),
    row(133, "Menu-bar count", Doing, None, &["airboss"], 12 * MIN),
    row(142, "Booking flow", Todo, None, &[], 6 * MIN),
    row(
        143,
        "API: slots endpoint",
        Done,
        Some(142),
        &["booking-api"],
        HOUR,
    ),
    row(
        144,
        "Web: slot picker",
        Doing,
        Some(142),
        &["booking-web"],
        6 * MIN,
    ),
    row(
        145,
        "Web: confirm screen",
        Todo,
        Some(142),
        &["booking-web"],
        2 * DAY,
    ),
    row(
        146,
        "Confirmation email",
        Todo,
        Some(142),
        &["booking-api"],
        HOUR,
    ),
    row(
        147,
        "End-to-end tests",
        Todo,
        Some(142),
        &["booking-web", "booking-api"],
        2 * DAY,
    ),
    row(
        150,
        "Worktree cleanup research",
        Todo,
        None,
        &["airboss"],
        3 * HOUR,
    ),
    row(
        151,
        "Rotate local MCP token",
        Todo,
        None,
        &["airboss", "dotfiles"],
        2 * DAY,
    ),
    row(152, "Read sortie prior art", Todo, None, &[], 4 * DAY),
    row(200, "Refunds", Todo, None, &[], 20 * MIN),
    row(
        201,
        "Schema: refunds table",
        Done,
        Some(200),
        &["booking-api"],
        DAY,
    ),
    row(
        202,
        "Refund service",
        Doing,
        Some(200),
        &["booking-api"],
        20 * MIN,
    ),
    row(
        203,
        "Payment webhook",
        Blocked,
        Some(200),
        &["booking-api"],
        5 * HOUR,
    ),
    row(
        204,
        "Refund button",
        InReview,
        Some(200),
        &["booking-web"],
        2 * HOUR,
    ),
    row(
        205,
        "Refund status page",
        Todo,
        Some(200),
        &["booking-web"],
        DAY,
    ),
    row(206, "Write refund policy copy", Todo, Some(200), &[], DAY),
    row(207, "Announce to support", Todo, Some(200), &[], DAY),
    row(
        208,
        "Backfill old refunds",
        Todo,
        Some(200),
        &["booking-api"],
        DAY,
    ),
];

/// The prototype's top-level order.
const TOP: &[i64] = &[142, 200, 128, 133, 131, 120, 150, 110, 151, 152, 98];

/// `(task, waits on)`.
const DEPS: &[(i64, i64)] = &[
    (145, 144),
    (146, 143),
    (147, 144),
    (147, 145),
    (147, 146),
    (202, 201),
    (203, 201),
    (205, 202),
    (205, 204),
    (207, 205),
    (208, 128),
];

/// Seeds DESIGN's sample data into an empty store, with the prototype's ids (filler tasks
/// take the ids in between and are deleted), repos, notes, branches, links, dependencies and
/// three proposals under 150.
pub fn design(s: &mut Store) -> airboss_core::Result<Design> {
    let me = Actor::Me;
    let mut repos: HashMap<&str, RepoId> = HashMap::new();
    for name in ["booking-web", "booking-api", "airboss", "dotfiles"] {
        let r = s.add_repo(
            name,
            Path::new(&format!("/Users/me/code/{name}")),
            Some(&format!("git@github.com:acme/{name}.git")),
            &me,
        )?;
        repos.insert(name, r.id);
    }
    let mut design = Design::default();
    let mut fillers = Vec::new();
    for r in ROWS {
        // SQLite hands out max(id) + 1: fill the gap, create the task, then drop the fillers.
        loop {
            let next = s.add_task(NewTask::new("filler"), &me)?;
            if next.id.0 == r.id {
                s.delete_task(next.id, &me)?;
                break;
            }
            if next.id.0 > r.id {
                return Err(airboss_core::Error::InvalidTarget);
            }
            fillers.push(next.id);
            if next.id.0 + 1 == r.id {
                break;
            }
        }
        let mut new = NewTask::new(r.title);
        new.parent = r.parent.map(TaskId);
        for name in r.repos {
            new = new.in_repo(repos[name]);
        }
        let t = s.add_task(new, &me)?;
        debug_assert_eq!(t.id.0, r.id);
        for f in fillers.drain(..) {
            s.delete_task(f, &me)?;
        }
        design.ages.insert(t.id, Duration::from_secs(r.age));
    }
    for (pos, id) in TOP.iter().enumerate() {
        s.move_task(TaskId(*id), None, pos as u32 + 1, &me)?;
    }
    for (a, b) in DEPS {
        s.add_dependency(TaskId(*a), TaskId(*b), &me)?;
    }
    // Parents take no branch (SPEC-core rule 10); the steps agents work on have theirs.
    s.set_branch(TaskId(144), Some("task-144-web-slot-picker"), &me)?;
    s.set_branch(TaskId(202), Some("task-202-refund-service"), &me)?;
    let notes = [
        (
            142,
            "Let people pick a slot and get a confirmation. Split across the API and the web app. The Slack thread has the product asks; the plan has the phases.",
        ),
        (
            144,
            "Keyboard support is a must: arrows move between slots, Return books.",
        ),
        (
            146,
            "Send the confirmation once a slot is held. Use the existing mail queue, not a new provider. Include the .ics file and a cancel link.",
        ),
        (200, "Refunds from the booking page, end to end."),
    ];
    for (id, body) in notes {
        s.edit_task(
            TaskId(id),
            airboss_core::TaskEdit::default().body(Some(body)),
            &me,
        )?;
    }
    let slack = NewLink::new(LinkKind::Slack, "/archives/C0BOOKING/p1764000000")
        .with_url("https://acme.slack.com/archives/C0BOOKING/p1764000000")
        .with_title("#booking thread");
    s.link(TaskId(142), slack.clone(), &me)?;
    s.link(
        TaskId(142),
        NewLink::new(LinkKind::Doc, "~/plans/booking-flow.md").with_title("booking-flow.md"),
        &me,
    )?;
    s.link(TaskId(146), slack, &me)?;
    s.link(
        TaskId(143),
        NewLink::pr("acme/booking-api#58").with_pr_state(PrState::Merged),
        &me,
    )?;
    s.link(TaskId(204), NewLink::pr("acme/booking-web#61"), &me)?;
    s.link(TaskId(128), NewLink::pr("acme/airboss#31"), &me)?;
    s.link(TaskId(120), NewLink::pr("acme/booking-api#70"), &me)?;
    for (id, thread) in [
        (144, "slot-picker"),
        (202, "refund-service"),
        (133, "tray-count"),
        (131, "fix-flaky-login"),
        (128, "mcp-server"),
    ] {
        s.link(
            TaskId(id),
            NewLink::new(LinkKind::T3Thread, thread),
            &Actor::Agent(format!("t3/{thread}")),
        )?;
    }
    for r in ROWS {
        match r.status {
            Todo => {}
            Blocked => {
                s.set_status(
                    TaskId(r.id),
                    Blocked,
                    Some("Needs test keys from the payments dashboard"),
                    &me,
                )?;
            }
            other => {
                s.set_status(TaskId(r.id), other, None, &me)?;
            }
        }
    }
    // What agents last said, so rows show their avatars.
    for (id, thread, note) in [
        (
            144,
            "slot-picker",
            "Asked whether to reuse the date picker.",
        ),
        (
            202,
            "refund-service",
            "Service skeleton and the refund state machine are in.",
        ),
        (
            131,
            "fix-flaky-login",
            "3 tests failed after the retry change.",
        ),
        (133, "tray-count", "Waiting for approval to add tray-icon."),
    ] {
        s.add_note(TaskId(id), note, &Actor::Agent(format!("t3/{thread}")))?;
    }
    let agent = Actor::Agent("t3/worktree-research".into());
    for title in [
        "List stale worktrees older than 14 days",
        "Find worktrees whose branch has merged",
        "Write a dry-run cleanup command",
    ] {
        let p = s.add_task(NewTask::new(title).under(TaskId(150)), &agent)?;
        design.ages.insert(p.id, Duration::from_secs(3 * HOUR));
        design.proposals.push(p.id);
    }
    Ok(design)
}

/// A large tree for timings: `tasks` tasks five levels deep, `deps` dependencies between
/// leaves of different parents, and a repo on every fourth task.
pub fn large(s: &mut Store, tasks: usize, deps: usize) -> airboss_core::Result<Vec<TaskId>> {
    let me = Actor::Me;
    let repos: Vec<RepoId> = (0..4)
        .map(|i| {
            s.add_repo(
                &format!("repo-{i}"),
                Path::new(&format!("/src/repo-{i}")),
                None,
                &me,
            )
            .map(|r| r.id)
        })
        .collect::<airboss_core::Result<_>>()?;
    let mut ids: Vec<TaskId> = Vec::with_capacity(tasks);
    // A breadth of 5 at each level gives 5 + 25 + 125 + 625 + 3125 slots; fill in order.
    let mut parents: Vec<(Option<TaskId>, u32)> = vec![(None, 0)];
    let mut i = 0;
    while ids.len() < tasks {
        let (parent, depth) = parents[i % parents.len().max(1)];
        let mut new = NewTask::new(format!("Task {} at depth {depth}", ids.len() + 1));
        new.parent = parent;
        if ids.len().is_multiple_of(4) {
            new = new.in_repo(repos[(ids.len() / 4) % repos.len()]);
        }
        let t = s.add_task(new, &me)?;
        ids.push(t.id);
        if depth < 4 {
            parents.push((Some(t.id), depth + 1));
        }
        i += 1;
    }
    let mut added = 0;
    let mut k = 0usize;
    while added < deps && k < ids.len() * 4 {
        let a = ids[(k * 7 + 3) % ids.len()];
        let b = ids[(k * 13 + 1) % ids.len()];
        k += 1;
        if a != b && s.add_dependency(a, b, &me).is_ok() {
            added += 1;
        }
    }
    for (n, id) in ids.iter().enumerate() {
        let to = match n % 10 {
            0 => Some(Status::Done),
            1 => Some(Status::Doing),
            2 => Some(Status::InReview),
            _ => None,
        };
        if let Some(to) = to {
            let _ = s.set_status(*id, to, None, &me);
        }
    }
    Ok(ids)
}
