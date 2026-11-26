//! The menu-bar item: the Needs me count as its title and a native menu of up to six items.
//!
//! Times are clock times, because a native menu isn't redrawn as time passes: "10:32" today,
//! "Mon 10:32" this week, "25 Nov" before that.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use airboss_services::{NeedsKind, NeedsList, time};
use tauri::image::Image;
use tauri::menu::{IconMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Runtime};

/// The tray's id.
pub const TRAY_ID: &str = "airboss";
/// Menu ids: an item is `needs:<key>`.
pub const NEEDS_PREFIX: &str = "needs:";
pub const OPEN_ID: &str = "tray:open";
pub const QUICK_ADD_ID: &str = "tray:quick_add";
pub const QUIT_ID: &str = "tray:quit";
/// How many items the menu lists.
pub const MAX_ITEMS: usize = 6;

/// One Needs me line in the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayItem {
    pub key: String,
    pub kind: NeedsKind,
    /// "144  Agent in slot-picker asks · 10:32".
    pub label: String,
}

/// What the menu-bar item shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayModel {
    /// The count, or nothing at 0.
    pub title: Option<String>,
    /// Also its accessibility label, which VoiceOver reads instead of the bare count:
    /// "Airboss, 8 waiting on you".
    pub tooltip: String,
    /// "Needs me 8", or "Nothing needs you".
    pub header: String,
    pub items: Vec<TrayItem>,
}

/// Builds the model. `offset` is the local time zone's offset from UTC, in seconds.
pub fn model(needs: &NeedsList, now: SystemTime, offset: i64) -> TrayModel {
    let count = needs.count;
    TrayModel {
        title: (count > 0).then(|| count.to_string()),
        tooltip: match count {
            0 => "Airboss, nothing waiting on you".into(),
            n => format!("Airboss, {n} waiting on you"),
        },
        header: match count {
            0 => "Nothing needs you".into(),
            n => format!("Needs me {n}"),
        },
        items: needs
            .items
            .iter()
            .take(MAX_ITEMS)
            .map(|i| {
                let when = time::from_ts(&i.since)
                    .map(|t| clock_label(t, now, offset))
                    .unwrap_or_default();
                let what = if i.context.is_empty() {
                    &i.headline
                } else {
                    &i.context
                };
                TrayItem {
                    key: i.key.0.clone(),
                    kind: i.kind,
                    label: if when.is_empty() {
                        format!("{}  {what}", i.task.0)
                    } else {
                        format!("{}  {what} · {when}", i.task.0)
                    },
                }
            })
            .collect(),
    }
}

const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Local days since the epoch, and seconds into that day.
fn local(t: SystemTime, offset: i64) -> (i64, i64) {
    let secs = match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    } + offset;
    (secs.div_euclid(86_400), secs.rem_euclid(86_400))
}

/// "10:32" today, "Mon 10:32" within the last six days, "25 Nov" before that.
pub fn clock_label(t: SystemTime, now: SystemTime, offset: i64) -> String {
    let (day, secs) = local(t, offset);
    let (today, _) = local(now, offset);
    let hm = format!("{:02}:{:02}", secs / 3600, secs % 3600 / 60);
    if day >= today {
        hm
    } else if today - day < 7 {
        format!("{} {hm}", WEEKDAYS[day.rem_euclid(7) as usize])
    } else {
        let (_, m, d) = civil(day);
        format!("{d} {}", MONTHS[(m - 1) as usize])
    }
}

/// Howard Hinnant's civil-from-days: (year, month 1–12, day 1–31).
fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// The template icon (macOS tints it for the menu bar's appearance).
fn icon() -> Image<'static> {
    tauri::include_image!("tray/icon.png")
}

/// The red icon for a kind of Needs me item.
fn kind_icon(kind: NeedsKind) -> Image<'static> {
    match kind {
        NeedsKind::Question => tauri::include_image!("tray/needs-question.png"),
        NeedsKind::Approval => tauri::include_image!("tray/needs-approval.png"),
        NeedsKind::Review => tauri::include_image!("tray/needs-review.png"),
        NeedsKind::FailedRun => tauri::include_image!("tray/needs-failed_run.png"),
        NeedsKind::AgentStopped => tauri::include_image!("tray/needs-agent_stopped.png"),
        NeedsKind::Proposed => tauri::include_image!("tray/needs-proposed.png"),
        NeedsKind::Ready => tauri::include_image!("tray/needs-ready.png"),
        NeedsKind::FinishIt => tauri::include_image!("tray/needs-finish_it.png"),
        NeedsKind::Quiet => tauri::include_image!("tray/needs-quiet.png"),
    }
}

/// The native menu for a model. `hotkey` is the quick-add hotkey as an accelerator
/// (`"Control+Alt+N"`, from Settings), shown as Quick Add's key equivalent ("⌃⌥N"). It has to
/// be the accelerator: tauri drops one that doesn't parse, such as the label "⌃⌥N".
///
/// The key equivalent doesn't register anything: muda only sets the menu item's
/// `keyEquivalent`, which AppKit consults while this menu is open. The global hotkey is a
/// Carbon hot key, which macOS takes before any menu sees the keystroke, so the two never
/// both fire.
pub fn menu<R: Runtime>(app: &AppHandle<R>, m: &TrayModel, hotkey: &str) -> tauri::Result<Menu<R>> {
    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(
        app,
        "tray:header",
        &m.header,
        false,
        None::<&str>,
    )?)?;
    for item in &m.items {
        menu.append(&IconMenuItem::with_id(
            app,
            format!("{NEEDS_PREFIX}{}", item.key),
            &item.label,
            true,
            Some(kind_icon(item.kind)),
            None::<&str>,
        )?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        OPEN_ID,
        "Open Airboss",
        true,
        Some("CmdOrCtrl+0"),
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        QUICK_ADD_ID,
        "Quick Add…",
        true,
        Some(hotkey),
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        QUIT_ID,
        "Quit Airboss",
        true,
        None::<&str>,
    )?)?;
    Ok(menu)
}

/// Creates the menu-bar item.
pub fn create<R: Runtime>(
    app: &AppHandle<R>,
    m: &TrayModel,
    hotkey: &str,
) -> tauri::Result<TrayIcon<R>> {
    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon())
        .icon_as_template(true)
        .title(m.title.as_deref().unwrap_or(""))
        .tooltip(&m.tooltip)
        .menu(&menu(app, m, hotkey)?)
        .show_menu_on_left_click(true)
        .build(app)?;
    set_label(&tray, &m.tooltip)?;
    Ok(tray)
}

/// Sets the status item button's accessibility label, so VoiceOver says "Airboss, 8 waiting
/// on you" rather than the title "8". tauri has no setter for it; the `tray-icon` item
/// underneath hands out its `NSStatusItem` (tauri is pinned to an exact version, as
/// `with_inner_tray_icon` asks).
fn set_label<R: Runtime>(tray: &TrayIcon<R>, label: &str) -> tauri::Result<()> {
    let label = label.to_string();
    tray.with_inner_tray_icon(move |inner| {
        if let Some(item) = inner.ns_status_item() {
            crate::macos::set_status_item_label(&item, &label);
        }
    })
}

/// Updates the title, tooltip and menu.
pub fn update<R: Runtime>(
    app: &AppHandle<R>,
    tray: &TrayIcon<R>,
    m: &TrayModel,
    hotkey: &str,
) -> tauri::Result<()> {
    tray.set_title(Some(m.title.as_deref().unwrap_or("")))?;
    tray.set_tooltip(Some(&m.tooltip))?;
    set_label(tray, &m.tooltip)?;
    tray.set_menu(Some(menu(app, m, hotkey)?))?;
    Ok(())
}

/// How long the menu waits after a change before it's rebuilt.
pub const REBUILD_DELAY: Duration = Duration::from_millis(250);

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use airboss_core::TaskId;
    use airboss_services::{NeedsItem, NeedsKey, Target};

    fn at(ts: &str) -> SystemTime {
        time::from_ts(ts).unwrap()
    }

    #[test]
    fn clock_times_today_this_week_and_before() {
        let now = at("2026-11-25T18:00:00.000Z"); // a Wednesday
        assert_eq!(clock_label(at("2026-11-25T10:32:00.000Z"), now, 0), "10:32");
        assert_eq!(
            clock_label(at("2026-11-23T10:32:00.000Z"), now, 0),
            "Mon 10:32"
        );
        assert_eq!(
            clock_label(at("2026-11-18T09:00:00.000Z"), now, 0),
            "18 Nov"
        );
        // In UTC+1 a 23:30 UTC time is already the next local day.
        assert_eq!(
            clock_label(at("2026-11-24T23:30:00.000Z"), now, 3600),
            "00:30"
        );
    }

    #[test]
    fn the_title_is_the_count_or_nothing() {
        let item = NeedsItem {
            key: NeedsKey("question:1".into()),
            kind: NeedsKind::Question,
            group: NeedsKind::Question.group(),
            task: TaskId(144),
            task_title: "Web: slot picker".into(),
            since: "2026-11-25T10:32:00.000Z".into(),
            headline: "Use the existing date picker?".into(),
            context: "Agent in slot-picker asks".into(),
            target: Target::Task(TaskId(144)),
            proposed: vec![],
            stale: false,
        };
        let list = NeedsList {
            items: vec![item; 8],
            count: 8,
            checked_at: None,
            version: 1,
        };
        let m = model(&list, at("2026-11-25T18:00:00.000Z"), 0);
        assert_eq!(m.title.as_deref(), Some("8"));
        assert_eq!(m.tooltip, "Airboss, 8 waiting on you");
        assert_eq!(m.header, "Needs me 8");
        assert_eq!(m.items.len(), 6);
        assert_eq!(m.items[0].label, "144  Agent in slot-picker asks · 10:32");
        let stopped = NeedsItem {
            key: NeedsKey("stopped:131:2026-11-25T10:32:00.000Z".into()),
            kind: NeedsKind::AgentStopped,
            group: NeedsKind::AgentStopped.group(),
            headline: "Agent stopped · no PR yet".into(),
            context: "fix-flaky-login stopped".into(),
            task: TaskId(131),
            ..list.items[0].clone()
        };
        let one = NeedsList {
            items: vec![stopped],
            count: 1,
            ..NeedsList::default()
        };
        let m = model(&one, at("2026-11-25T18:00:00.000Z"), 0);
        assert_eq!(m.items[0].kind, NeedsKind::AgentStopped);
        assert_eq!(m.items[0].label, "131  fix-flaky-login stopped · 10:32");
        let empty = model(&NeedsList::default(), SystemTime::now(), 0);
        assert_eq!(empty.title, None);
        assert_eq!(empty.header, "Nothing needs you");
    }
}
