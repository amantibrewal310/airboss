//! Due: an open task whose due date (SPEC-core rule 35) is today or past, in my own time
//! zone. It shows from the first moment of that day until the task is done or dropped, its date
//! changes or is cleared, or I dismiss or snooze it.
//!
//! Agents turn "Friday" or "in 2 days" into the date; attention only compares days. Its text
//! changes at each local midnight ("Due today", then "Due yesterday" …), so a visible item
//! wakes the worker then, and a task due later wakes it when its day starts.

use std::time::SystemTime;

use airboss_services::time::{date_days, days_civil, local_day, local_day_start, to_ts, weekday};
use airboss_services::{NeedsKind, Target};

use crate::compute::{Ctx, Entry, ItemFor, Wake, count, is_open};

pub(crate) fn due(ctx: &Ctx<'_>, out: &mut Vec<Entry>, wake: &mut Wake) {
    // Most trees have no dates at all: skip asking the time zone.
    if !ctx.core.tree.iter().any(|n| n.task.due_at.is_some()) {
        return;
    }
    let today = local_day(ctx.now, ctx.cfg.utc_offset_at(ctx.now));
    for node in &ctx.core.tree {
        let Some(date) = node.task.due_at.as_deref() else {
            continue;
        };
        if !is_open(node.task.status) {
            continue;
        }
        // Core refuses any other shape, so this is only a hand-edited file.
        let Some(day) = date_days(date) else {
            continue;
        };
        let starts = day_start(ctx, day);
        if day > today {
            wake.at(starts);
            continue;
        }
        let item = ItemFor {
            kind: NeedsKind::Due,
            key: format!("due:{}:{date}", node.task.id.0),
            node,
            since: to_ts(starts),
            headline: headline(today - day),
            context: format!("Due {}", spoken(day, today)),
            target: Target::Task(node.task.id),
        };
        out.push(Entry {
            item: item.build(false),
            tasks: vec![node.task.id],
            // The headline counts days, so it changes when the next one starts.
            changes_at: Some(day_start(ctx, today + 1)),
        });
    }
}

/// The first moment of local day `day`, in the zone's offset on that day: the offset now,
/// corrected once for a daylight-saving change in between.
fn day_start(ctx: &Ctx<'_>, day: i64) -> SystemTime {
    let guess = local_day_start(day, ctx.cfg.utc_offset_at(ctx.now));
    local_day_start(day, ctx.cfg.utc_offset_at(guess))
}

/// "Due today", "Due yesterday", "Overdue by 3 days".
fn headline(late: i64) -> String {
    match late {
        ..=0 => "Due today".to_string(),
        1 => "Due yesterday".to_string(),
        n => format!("Overdue by {}", count(n as usize, "day")),
    }
}

const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// "Fri 4 Dec", with the year when it isn't this one: "Wed 31 Dec 2025".
pub(crate) fn spoken(day: i64, today: i64) -> String {
    let (year, month, date) = days_civil(day);
    let (this_year, _, _) = days_civil(today);
    let name = DAYS[weekday(day) as usize];
    let month = MONTHS[(month - 1) as usize];
    match year == this_year {
        true => format!("{name} {date} {month}"),
        false => format!("{name} {date} {month} {year}"),
    }
}
