---
name: Airboss
description: A dark, dense Mac task list that shows where agent work stands and what needs me.
colors:
  desk: "#050506"
  bg: "#0f1012"
  bg-side: "#0a0b0c"
  bg-raised: "#16171a"
  bg-input: "#1b1c20"
  bg-hover: "rgba(255,255,255,.045)"
  bg-sel: "rgba(255,255,255,.075)"
  line: "rgba(255,255,255,.07)"
  line-2: "rgba(255,255,255,.11)"
  text: "#ededf0"
  text-2: "#9a9ca5"
  text-3: "#6b6e78"
  accent: "#7b7ff5"
  accent-fill: "#5e63e6"
  accent-soft: "rgba(123,127,245,.14)"
  needs: "#f2555a"
  needs-soft: "rgba(242,85,90,.12)"
  st-todo: "#8a8d96"
  st-doing: "#e6b450"
  st-review: "#4cc38a"
  st-done: "#7b7ff5"
  st-blocked: "#f08c3a"
  repo-1: "#5b9cff"
  repo-2: "#e66aa6"
  repo-3: "#3fc79a"
  repo-4: "#f0a04a"
  actor-agent: "#4b4fc4"
  actor-me: "#2c7a5a"
  actor-sync: "#555a66"
  actor-plan: "#8a5a2c"
  actor-launch: "#3f6aa8"
  light-desk: "#e4e5e9"
  light-bg: "#ffffff"
  light-bg-side: "#f7f7f8"
  light-bg-raised: "#ffffff"
  light-bg-input: "#f3f3f5"
  light-bg-hover: "rgba(0,0,0,.04)"
  light-bg-sel: "rgba(0,0,0,.06)"
  light-line: "rgba(0,0,0,.07)"
  light-line-2: "rgba(0,0,0,.11)"
  light-text: "#1b1c1f"
  light-text-2: "#62656e"
  light-text-3: "#8e919a"
  light-accent: "#5e63e6"
  light-accent-soft: "rgba(94,99,230,.1)"
  light-needs: "#e03e44"
  light-needs-soft: "rgba(224,62,68,.09)"
  light-st-todo: "#9a9da5"
  light-st-doing: "#d39a1d"
  light-st-review: "#2a9f68"
  light-st-done: "#5e63e6"
  light-st-blocked: "#e0751f"
  light-repo-1: "#2f73e0"
  light-repo-2: "#cc4386"
  light-repo-3: "#1f9e74"
  light-repo-4: "#d07a12"
typography:
  display:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, \"SF Pro Text\", sans-serif"
    fontSize: "26px"
    fontWeight: 600
    lineHeight: 1.25
    letterSpacing: "-0.018em"
    fontFeature: "\"cv11\", \"ss01\", \"tnum\""
  headline:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, \"SF Pro Text\", sans-serif"
    fontSize: "21px"
    fontWeight: 600
    lineHeight: 1.25
    letterSpacing: "-0.018em"
  title:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, \"SF Pro Text\", sans-serif"
    fontSize: "17px"
    fontWeight: 500
    lineHeight: 1.4
    letterSpacing: "-0.01em"
  field:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, \"SF Pro Text\", sans-serif"
    fontSize: "15px"
    fontWeight: 400
    lineHeight: 1.45
  body:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, \"SF Pro Text\", sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.45
    letterSpacing: "-0.006em"
    fontFeature: "\"cv11\", \"ss01\""
  notes:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, \"SF Pro Text\", sans-serif"
    fontSize: "13.5px"
    fontWeight: 400
    lineHeight: 1.65
  meta:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, \"SF Pro Text\", sans-serif"
    fontSize: "12px"
    fontWeight: 400
    lineHeight: 1.35
  label:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, \"SF Pro Text\", sans-serif"
    fontSize: "11.5px"
    fontWeight: 500
    lineHeight: 1.25
  mono:
    fontFamily: "ui-monospace, \"SF Mono\", Menlo, monospace"
    fontSize: "12px"
    fontWeight: 400
rounded:
  xs: "4px"
  sm: "5px"
  md: "6px"
  lg: "8px"
  xl: "10px"
  window: "12px"
  pill: "11px"
spacing:
  xxs: "2px"
  xs: "4px"
  sm: "6px"
  md: "8px"
  row-gap: "10px"
  lg: "12px"
  xl: "16px"
  gutter: "18px"
  panel: "24px"
  section: "26px"
components:
  window-sidebar:
    backgroundColor: "{colors.bg-side}"
    width: "232px"
    padding: "0 8px"
  header:
    backgroundColor: "{colors.bg}"
    height: "44px"
    padding: "0 14px 0 18px"
  nav-item:
    textColor: "{colors.text-2}"
    rounded: "{rounded.md}"
    height: "28px"
    padding: "0 8px"
  nav-item-selected:
    backgroundColor: "{colors.bg-sel}"
    textColor: "{colors.text}"
  needs-badge:
    backgroundColor: "{colors.needs}"
    textColor: "#ffffff"
    rounded: "9px"
    height: "18px"
    padding: "0 5px"
  task-row:
    textColor: "{colors.text}"
    typography: "{typography.body}"
    height: "38px"
    padding: "0 16px 0 14px"
  task-row-selected:
    backgroundColor: "{colors.bg-sel}"
  group-header:
    backgroundColor: "{colors.bg-raised}"
    textColor: "{colors.text-2}"
    height: "34px"
    padding: "0 18px"
  button-primary:
    backgroundColor: "{colors.accent-fill}"
    textColor: "#ffffff"
    rounded: "{rounded.md}"
    height: "28px"
    padding: "0 11px"
  button-secondary:
    backgroundColor: "{colors.bg-input}"
    textColor: "{colors.text}"
    rounded: "{rounded.md}"
    height: "28px"
    padding: "0 11px"
  button-secondary-hover:
    backgroundColor: "{colors.bg-sel}"
  button-ghost:
    textColor: "{colors.text-2}"
    rounded: "{rounded.md}"
    height: "28px"
    padding: "0 11px"
  button-ghost-hover:
    backgroundColor: "{colors.bg-hover}"
    textColor: "{colors.text}"
  chip-button:
    textColor: "{colors.text-2}"
    rounded: "{rounded.md}"
    height: "26px"
    padding: "0 9px"
  icon-button:
    textColor: "{colors.text-2}"
    rounded: "{rounded.md}"
    size: "26px"
  pill:
    textColor: "{colors.text-2}"
    typography: "{typography.label}"
    rounded: "{rounded.pill}"
    height: "22px"
    padding: "0 8px"
  pill-blocked:
    textColor: "{colors.st-blocked}"
  pill-ready:
    textColor: "{colors.st-review}"
  tag:
    backgroundColor: "{colors.bg-input}"
    textColor: "{colors.text}"
    rounded: "{rounded.sm}"
    height: "22px"
    padding: "0 7px"
  live-chip:
    backgroundColor: "{colors.bg-input}"
    textColor: "{colors.text-2}"
    typography: "{typography.label}"
    rounded: "{rounded.xl}"
    height: "20px"
    padding: "0 7px 0 6px"
  kbd:
    backgroundColor: "{colors.bg-input}"
    textColor: "{colors.text-2}"
    rounded: "{rounded.xs}"
    height: "18px"
    padding: "0 5px"
  needs-callout:
    backgroundColor: "{colors.needs-soft}"
    textColor: "{colors.text}"
    rounded: "{rounded.lg}"
    padding: "11px 12px"
  detail-panel:
    backgroundColor: "{colors.bg}"
    width: "480px"
    padding: "20px 24px 40px"
  plan-card:
    backgroundColor: "{colors.bg-raised}"
    rounded: "{rounded.window}"
    padding: "16px 16px 14px"
  menu:
    backgroundColor: "{colors.bg-raised}"
    rounded: "9px"
    padding: "5px"
  menu-item:
    rounded: "{rounded.md}"
    height: "30px"
    padding: "0 9px"
  menu-item-highlight:
    backgroundColor: "{colors.bg-sel}"
  palette:
    backgroundColor: "{colors.bg-raised}"
    rounded: "{rounded.window}"
    width: "640px"
  quick-add:
    backgroundColor: "{colors.bg-raised}"
    rounded: "{rounded.window}"
    width: "680px"
  toast:
    backgroundColor: "{colors.bg-raised}"
    textColor: "{colors.text}"
    rounded: "{rounded.lg}"
    padding: "9px 13px"
---

# Design System: Airboss

## Overview

**Creative North Star: "The Quiet Tower"**

Airboss is a control tower for agent work. It sits dark and still until something needs the person running it, and then it says so in one colour. The app is a dense list first: 38px rows, hairline borders, 13px type, one indigo accent. Detail is always one keystroke away, in a 480px panel or a full page laid out like a document.

It is a native Mac app, not a website in a window. Traffic lights, menus, a menu-bar item and standard Mac keys are part of the look. Dark is the main theme; light uses the same structure with its own values. Surfaces step from near black to slightly lighter grey; there are no gradients on surfaces and no decoration that does not carry state.

Status is shown by shape first and colour second, so it never depends on colour alone. Motion is short and only answers something the user or an agent did.

**Key Characteristics:**
- Dense rows at a fixed 38px, so long lists can be virtualised.
- Hairline borders at 7% and 11% opacity; depth comes from tonal steps, not shadows.
- One accent (indigo) for done, selection, focus and the main button.
- One alarm colour (red) for "needs me", and nothing else.
- Every number uses tabular figures.
- Keyboard first: every action has a shortcut, and the main buttons show their key.

## Colors

A near-black, low-contrast base with one cool accent, one alarm red, a five-colour status set and four repo colours. Values are in the frontmatter; light-mode values carry the `light-` prefix and replace the dark ones under the light theme.

### Primary
- **Signal Indigo** (accent): done status, keyboard selection outlines, focus rings, selected cards, progress rings, toasts' icon. In light mode the accent darkens to the fill value.
- **Deep Indigo Fill** (accent-fill): the background of the one primary button on a view (Do it, New task, Create task). Text on it is white.
- **Indigo Wash** (accent-soft): new-row highlight, selected station on the run-order map, focus halo, text selection.

### Secondary
- **Needs-Me Red** (needs): the sidebar Needs me count, the menu-bar count, the needs-me dot on a row, the "Waiting on you" chip, the kind icon and label on needs-me cards.
- **Needs-Me Wash** (needs-soft): the fill of a needs-me callout in the detail panel.

### Tertiary (status and repos)
- **Status set**: todo grey (st-todo), in-progress amber (st-doing), in-review green (st-review), blocked orange (st-blocked), done indigo (st-done, the same as the accent). Each status also has its own icon shape (see Components).
- **Repo set** (repo-1 to repo-4): blue, pink, green, orange. Each repo keeps one colour everywhere: its dot in the sidebar, row tags, property tags and run-order stations. Repo colour never colours lines or lanes, since a task can have no repo or several.
- **Actor set** (actor-agent, actor-me, actor-sync, actor-plan, actor-launch): fills for the 18–20px initial avatars in rows and the activity feed. They are the same in both themes.

### Neutral
- **Desk** (desk): behind the window, outside the app.
- **Main Surface** (bg): the list, header, detail panel and page.
- **Sidebar Black** (bg-side): the darkest surface, the navigation sidebar.
- **Raised Grey** (bg-raised): menus, popovers, modals, toasts, plan cards, sticky group headers, needs-me cards.
- **Field Grey** (bg-input): buttons, tags, key caps, chips, empty bar track.
- **Hover and Selected Washes** (bg-hover, bg-sel): white at 4.5% and 7.5% (black at 4% and 6% in light).
- **Hairlines** (line, line-2): 7% for dividers between rows and panes, 11% for control borders and popovers.
- **Text ramp** (text, text-2, text-3): titles and values; secondary text and labels; ids, times, counts and placeholders.

### Named Rules
**The Red Is Mine Rule.** Red means "needs me" and nothing else. If it isn't waiting on the user's decision, it isn't red. Blocked work and broken connections are orange; failures that need the user are red because they sit in Needs me.

**The One Accent Rule.** Indigo is the only accent. Done, selection, focus and the primary button share it. No second brand colour.

**The Owned Colour Rule.** A repo's colour is the same in every place it appears. Never reassign a repo colour per view.

## Typography

**Body Font:** Inter (with the system UI font as fallback), stylistic sets cv11 and ss01 on.
**Label/Mono Font:** the system monospace, for branch names and thread names only.

**Character:** One family at small sizes, in three weights (400, 500, 600). Small and dense because the list is the product; size steps are small and weight does most of the work.

### Hierarchy
- **Display** (600, 26px, 1.25): plan percentages on cards and the full-page task title. Plan card figures tighten to -0.03em.
- **Headline** (600, 21px, 1.25): the task title in the detail panel. The progress figure over the stacked bar uses 22px.
- **Title** (500, 17px, 1.4): the question on a needs-me card; empty-state headings use 600.
- **Field** (400, 15px): the Jump input. The quick-add title is larger, 18px at 500.
- **Body** (400 or 500, 13px, 1.45): base size. Row titles at 500; everything else at 400. Buttons and property values use 12.5px.
- **Notes** (400, 13.5px, 1.65): task notes, the only long-form reading text.
- **Meta** (400, 12px): ids, times, counts, breadcrumbs, section headers in the detail panel (600).
- **Label** (500, 11.5px): sidebar section headers, pills, chips, legends, wave headings, menu shortcut hints. No uppercase anywhere.

### Named Rules
**The 11.5 Floor Rule.** No running text below 11.5px. Only key caps (11px) and actor initials (9–10px) go smaller.

**The Tabular Figures Rule.** Every number that can change (ids, counts, percentages, times) uses tabular figures so columns don't jitter.

## Layout

The window is a two-column grid: a 232px sidebar and the main area. The main area has a 44px header and a body that splits into the list and a 480px detail panel. The full task page swaps the panel for a centred document column (max 860px, 30px/40px padding) and a 280px properties rail on the right. Needs me is a two-pane inbox: a 420px list and the selected item's detail. Plans is a grid of cards, at least 340px wide, 12px apart.

Rows are 38px high and indent 22px per nesting level. Spacing steps are 4, 6, 8, 10, 12, 16, 18, 24px; sections in the detail panel are 26px apart. Properties use a two-column grid (104px labels in the panel, 86px in the page rail), 30px per line.

**Narrow widths.** The list is a size container and the row drops detail in two steps:
- At 820px of list width or less: repo pills become repo dots, and the time column narrows from 44px to 32px.
- At 680px or less: "waits on 144, 145" becomes the dependency icon plus a count, the progress fraction hides (the ring stays), and live chips lose their label and show only the dot.
- When the whole window is 980px or less, the detail panel narrows to 380px.

### Named Rules
**The Fixed Row Rule.** A task row is always exactly 38px. Nothing in a row wraps or grows; long titles truncate with an ellipsis. This is what lets a 2,000-task list be virtualised and scroll at 60 fps.

## Elevation & Depth

Flat by default. Panes are separated by hairlines and by tonal steps (sidebar darkest, main surface, raised grey). One shadow exists, and only things that float above the window use it.

### Shadow Vocabulary
- **Float** (`box-shadow: 0 16px 48px rgba(0,0,0,.55), 0 0 0 1px rgba(255,255,255,.08)`; light: `0 16px 48px rgba(0,0,0,.16), 0 0 0 1px rgba(0,0,0,.08)`): the window itself, menus, popovers, the Jump palette, quick add, the shortcut sheet, the menu-bar dropdown, toasts.
- **Focus halo** (`0 0 0 3px` accent-soft): a selected plan card or a focused quick-add property.
- **Keyboard outline** (`inset 0 0 0 1px` accent): the row or inbox item that has keyboard focus.

Modals sit on a scrim of black at 35% (12% in light).

### Named Rules
**The Float-Only Rule.** Shadows mean "this floats above the window". Cards, rows and panels inside the window never get one.

## Shapes

Small, even corners. Controls use 6px; tags and inline inputs 5px; key caps 4px; callouts, toasts and thread rows 8px; menus 9px; popovers 10px; the window, cards and modals 12px. Pills and live chips are fully rounded (half their height). Status icons, progress rings, repo dots and avatars are circles. Borders are always 1px hairlines.

## Components

Calm, flat and compact. Controls are quiet until hovered; colour appears only when there is a state behind it.

### Buttons
- **Shape:** gently rounded (6px), 28px tall, 12.5px medium text.
- **Primary:** Deep Indigo Fill with white text. One per view at most (Do it, New task, Create task). Hover brightens it by 10%.
- **Secondary:** Field Grey with an 11% hairline; hover goes to the selected wash.
- **Ghost:** no fill or border, secondary text; hover shows the hover wash and full text.
- **Chip button:** 26px, hairline border, used in the header for Jump.
- **Key hints:** buttons may show their shortcut as a key cap inside the label (Do it ⌘↩, Jump ⌘K, Create task ↩, Dismiss ⌘⌫). Inside a primary button the cap is white at 12% with no border.

### Status icons
16px, drawn on a 6.25px ring with a 1.6px stroke. Shape carries the meaning before colour does:
- **Todo:** a plain grey ring.
- **Waiting:** a grey ring with long dashes. Derived, never set: a todo task with any open blocker.
- **Proposed:** a grey ring with short dots. Set by an agent; the user accepts or rejects it.
- **In progress:** an amber ring with the right half filled.
- **In review:** a green ring filled three quarters.
- **Blocked:** an orange ring with a horizontal bar. Always shown with its reason.
- **Done:** a solid indigo disc with a white check.
- **Dropped:** a grey ring with an X.

The status picker (S, or click the icon) lists six choices in this order, each with its number key: 1 Todo, 2 In progress, 3 In review, 4 Blocked, 5 Done, 6 Dropped. Space also toggles Done. Waiting and Proposed are not in the picker. Blocked asks for a reason; Dropped asks to confirm and drops open subtasks too.

### Progress
Progress is done ÷ all leaf subtasks at any depth. Only leaves count, so nested parents are not counted twice. Dropped subtasks are left out of both numbers. Proposed subtasks count as todo.
- **Ring:** 13–14px in rows and the sidebar, 22px on plan cards. Indigo arc on a line-2 track; it turns green at 100%.
- **Stacked bar:** 6px (8px on the task page), in this order: done (indigo), in review (green), in progress (amber), blocked (orange). Todo is the empty track. In review, in progress and blocked show in the bar but add nothing to the percentage.
- **Legend:** 8px square swatches with counts, todo as an outlined swatch.
- Rows show the ring plus "done/total"; the page shows "35% · 7 of 20 done".

### Task rows
- 38px, fixed. Left to right: fold arrow, status icon, id (34px, tabular), title, progress ring and fraction, live chip, needs-me dot, a flexible gap, pills (Blocked, waits on, Ready), repo tags, actor avatar, time.
- Hover uses the hover wash; selection the selected wash; keyboard focus adds a 1px inset indigo outline.
- Done rows dim their title to secondary text.
- Repo pills (dot plus name) show every linked repo, up to two, then "+1". A task with no repo shows none.
- A parent with no repos of its own shows the union of its subtasks' repos.
- Rename happens in place: the title becomes an input with an indigo border.

### Pills and tags
- **Pill** (22px, fully rounded, hairline border, label type): Blocked (orange), Ready (green), "waits on 144, 145" (neutral, with the dependency icon), repo (with its dot; one per linked repo).
- **Tag** (22px, 5px corners, Field Grey): property values in the detail panel: repos, branch (mono), parent, waits-on and blocks (each with the other task's status icon), links.

### Repos are linked, one or several
- A task is linked to the repos it touches, usually known when it's created. One task can be linked to several (for example booking-web and booking-api). A task with no repo is rare: research, writing, a decision.
- The detail panel shows "Repos" as tags plus a "Link repo" control that opens a checklist of repos. R on a selected row and "Link repos…" in the right-click menu open the same checklist. A parent with none of its own shows its subtasks' repos with "· from subtasks".
- New task: "Repos" is a multi-select; ↩ or a click toggles each repo.
- Do it: one repo starts one thread there; several start one thread per repo on a shared branch; none opens a small menu, "Start in <repo>" for each repo plus "No repo: research only".

### Live agent chips
Shown after the title only while a thread is attached and the task is open. A 20px rounded chip with a 6px dot and a label:
- **Agent working:** green dot with a soft ring pulsing outward every 1.6s.
- **Waiting on you:** red dot. The same item also appears in Needs me.
- **Stopped:** grey dot. Also used when the thread finished without finishing the task.
In the Threads section of the detail panel the labels shorten to Running, Waiting, Stopped. At narrow widths the chip shows only its dot.

### Needs me
- **Inbox list:** grouped under 11.5px headings in a fixed order: Questions, Reviews, Failed runs, Stopped, Due, Proposed, Ready, Quiet. Each item: kind icon, task id and title, time, and one line of context.
- **Detail card:** raised, 12px corners. Kind icon in red, then "Question · 6m ago · 144 Web: slot picker", the question at 17px, and actions. The main action is primary and shows ↩; Dismiss sits at the right as a ghost button with ⌘⌫.
- **Actions per kind:** Question and Approval: Open thread. Review: Open PR. Failed run: Open thread. Agent stopped: Open thread, then Mark done (Space), Set in review, Snooze 3 days. Due: Open, then Mark done (Space), Snooze 3 days. Proposed subtasks: Accept, Edit or reject each one, or Accept all / Reject all. Ready to start: Do it. Quiet 5+ days: Open or Snooze 3 days.
- **Agent stopped:** a run that ended without a PR. The task is in progress, every agent on it has stopped (not failed: that's a Failed run), no PR is open, and nothing has changed for 10 minutes. Its icon is a circle with a pause, the line reads "refund-service stopped" with the time beside it, and the card's headline is "Agent stopped · no PR yet" ("no open PR" when its only PRs are closed). It sits right after Failed runs, since both are a run that ended, and before the calmer kinds. It leaves as soon as an agent runs again, a PR opens or the task moves on.
- **Due:** a task whose due date has come. Its icon is a calendar with a tick, the line reads "Due Tue 24 Nov", and the headline counts the days: "Due today", "Due yesterday", "Overdue by 3 days". It sits after Stopped and before Proposed. In the panel, the **Due** line shows the date as a tag ("Fri 4 Dec"), red-washed once the day has come; click it to pick another day, × to clear, or "None · set a date" to add one.
- A cleared item slides 10px left and fades over 180ms, then the list closes up.
- **Empty state:** a check, "Nothing needs you", when it was last checked, counts of in-progress and ready tasks, and a button to show ready tasks.
- **In the detail panel:** the same items appear as red-washed callouts above the properties.

### Run-order map
A parent's subtasks drawn as a map, on the full task page:
- **Columns** are waves, headed "Wave 1", "Wave 2", split by hairlines. Within a wave, tasks stack in sibling order, 80px apart. There are no repo lanes: repos are optional, so a station shows its repo dots in its subline instead.
- **Stations** are the subtasks' 16px status icons, ringed in the surface colour so lines stop cleanly. Under each: the title (12.5px, 500) and "id · state" (11.5px). State reads "ready" in green, "blocked" in orange, "waits on 202, 204" or "waits outside: 128", followed by the task's repo dots if it has any.
- **Lines** run from blocker to task, 1.75px with round caps, in neutral grey: solid (text-3) when the blocker is done, dashed (2 on, 5 off, text-2) while waiting. A line between neighbouring waves bends with a smooth curve just before the task. A line that skips a wave drops into the gap between rows, runs along it, and rises into the task, so it never crosses another station.
- **Outside blocker:** a short grey dashed stub (about 50px) into the station, with no source drawn.
- **Hover** a station to trace it: its lines thicken to 2.5px and all others fade to 18%. Click selects (indigo wash; ⌘↩ starts it); double-click opens it.
- A legend under the map explains solid and dashed lines.
- **Waves list:** the other form of the same data. "Wave 1 · can start now", "· in parallel" when a wave holds more than one task, then 32px rows with status, id, title, pills and any repo dots. The detail panel always uses the Waves list; the full page has a Waves / Map switch and opens on Map.
- **Large parents:** fall back to the Waves list. The prototype has no automatic threshold yet: the map is 460–780px wide and its columns just get narrower as waves are added, so pick the cut-off when building.

### Plans cards
Raised cards, 12px corners, hairline border; hover lifts 1px and darkens the border; the selected card gets an indigo border and focus halo. Each card: status icon, title, id; the percentage at 26px with "7 of 20 done" and a 22px ring; the stacked bar; then rows for Wave (with a 18×5px dot per wave: indigo done, amber current), Next up (Ready pills), Agents (a working chip and a blocked count), Repos.

### Sidebar and connections footer
- Navigation items are 28px with a 16px icon, label and a count in text-3. Needs me comes first and shows a red count badge (18px, fully rounded) instead of a plain count. Then Ready to start, Plans, In progress, All tasks.
- "Parents" lists open parent tasks with their status icon, percentage and a 13px ring. "Repos" lists repos with their colour dot.
- **Connections footer:** pinned to the bottom above a hairline. When everything works it reads "All connected" with a 7px green dot, in text-3. When something is off, the text and dot turn orange and say what is off in plain words, one service at a time (for example "Agents off"). Clicking opens a 300px popover with one line per service: its name and state, then what it does, or how to fix it if it's off.

### Menus, Jump and quick add
- **Menus** (status picker, right-click): raised, 9px corners, 30px items with an icon, label and a shortcut hint at the right. Arrow keys move, ↩ picks, Esc closes.
- **Jump (⌘K):** 640px, 13% from the top. A 52px input at 15px, then tasks and commands in 38px rows with their key caps. With no match, ↩ creates the text as a new task.
- **Quick add:** 680px. Title (18px), notes, then property pills (status, "Repos · optional" multi-select, parent). Tab opens repos, Shift-Tab parent. Pasting a chat link attaches it as a link and keeps the rest as the title.
- **Toasts:** bottom right, raised, with an indigo spark icon; they rise 6px in and leave after 2.8s.
- **Menu-bar dropdown:** 360px, the Needs me count and up to six items, then Open (⌘0) and Quick add (⌃⌥N).

### Keyboard map
Single-letter keys only work when no field has focus. Every shortcut also lives in the Task and View menus.

| Key | Action |
|---|---|
| ↑ ↓ | Move through rows, inbox items, plan cards |
| → ← | Open or fold; → on an open parent goes to its first subtask, ← goes to the parent |
| Return | Rename the task; in Needs me, run the main action; in Plans, open |
| Space | Mark done or not done (not on a parent, whose status follows its subtasks); in Needs me, Mark done (Agent stopped, Due) |
| S, then 1–6 | Change status |
| R | Link repos (checklist) |
| ⌘↩ | Do it (in Needs me: the main action) |
| ⇧⌘↩ | Plan it |
| ⌘K | Jump to a task or command |
| ⌘↓ / ⌘↑ | Open the full page / go back |
| Esc | Close the page, then clear map selection, then close the panel |
| C or ⌥N | New task |
| ⌃⌥N | Quick add from any app (changeable in Settings) |
| G then N, L, R, A, P | Needs me, Plans, Ready, All tasks, In progress |
| ⌘0 | Open Airboss on Needs me |
| ⌘O | Open the latest thread or PR |
| ⌘D | Add waits-on |
| ⌘L | Add link |
| ⌘B | Mark blocked |
| ⌘⌫ | Dismiss (Needs me); Drop (task menu) |
| ⇧⌘C | Copy task number |
| ? | Shortcut sheet |

### Motion
Short and ease-out (`cubic-bezier(.2,.8,.2,1)`). Most moves take 120–200ms:
- **Menus and popovers:** fade and drop 4px in, 100–120ms. **Modals:** fade, drop 4px and scale from 98.5%, 140ms.
- **Panel slide-in:** the detail panel fades in from 10px to the right, 180ms.
- **Status pop:** on a status change the icon scales 0.5 → 1.18 → 1, 300ms.
- **New-row highlight:** a new row starts with the indigo wash and fades it out over 600ms.
- **Progress:** rings and bars ease to their new value over 400ms.
- **Live pulse:** the working chip's ring pulses every 1.6s, the only looping motion.
- **Reduce Motion:** all animation and transitions turn off.

## Do's and Don'ts

### Do:
- **Do** keep red for "needs me" only: the count badge, the needs-me dot, the "Waiting on you" chip and needs-me cards.
- **Do** keep task rows at exactly 38px and truncate instead of wrapping.
- **Do** show status by shape and colour together; never colour alone.
- **Do** use tabular figures for every id, count, percentage and time.
- **Do** give each repo one colour and use it for its dots and tags. **Don't** colour lines or lanes by repo.
- **Do** show the shortcut on the primary button of a view, and list every shortcut in the Task and View menus.
- **Do** describe connection problems in plain words, with details in the popover.
- **Do** scope every component's class names. The prototype had real bugs from shared global names (`.plan`, `.stop`, `.empty`); in Svelte, keep styles in the component and avoid bare global classes.
- **Do** set font, size and colour on every text node in Paper. Text there doesn't inherit styles from its frame.

### Don't:
- **Don't** use red for errors, warnings, destructive actions or blocked work. Blocked and connection problems are orange.
- **Don't** add a second accent colour or gradients on surfaces.
- **Don't** put shadows on anything inside the window; only floating layers get the float shadow.
- **Don't** set text below 11.5px, except key caps and actor initials.
- **Don't** show ports, protocol names or acronyms in the main UI. Those belong in the connections popover at most.
- **Don't** start anything from the run-order map by itself; it only shows the order.
- **Don't** animate without a state change behind it; the working pulse is the only loop.

## Needs from the core data model

Things the design shows that SPEC-core.md doesn't store or expose yet. Owner in brackets.

- **Several repos per task** [core]: `tasks.repo_id` holds one repo or none, but a task can be linked to several repos (a `task_repos` table, with an event when a repo is linked or unlinked), and a parent's view is the union of its subtasks'. `launch` then starts one thread per linked repo on a shared branch.
- **A stable colour per repo** [app settings, or a column on `repos`]: `repos` stores name, path and a project id only, and the design gives each repo one colour everywhere.
- **Dismiss and snooze state for Needs me items, with an until-time** [attention]: core has no Needs me items at all; Dismiss and "Snooze 3 days" need to survive a restart.
- **History for a task and its subtasks together** [core]: the API has only `history(id)`, but a parent's activity feed shows its subtasks' events ("143 moved to Done").
- **A title on links** [core]: links store kind and ref only, and the design shows labels like "#booking thread" and "booking-flow.md".
- **PR state and diff stats on PR links** [sync]: rows and cards show "merged", "checks passed", "+412 −38"; sync owns observed PRs.
- **Live thread state per linked thread: running, waiting on you, stopped** [sync]: drives the live chips; core only stores the link.
- **The shared branch name per parent task** [launch]: the page shows a Branch property and Do it starts one thread per repo on that branch; core has no branch field.
- **Outside blockers' ids from `run_order`** [core]: `run_order` returns waves of tasks only; the map and Waves list show "waits outside: 128". `waits_on(id)` can supply them, but only with one call per subtask.
- **Every dependency in both directions, finished ones included** [core]: `waits_on` returns open blockers only, but the map draws solid lines from finished blockers and the panel lists Waits on (done ones too) and Blocks.
- **Notes on a task** [core]: the spec says agents add notes, and `set_status` takes a note, but the tasks table's columns aren't listed and nothing names an editable notes body. The panel shows one per task.
- **"Became ready at" and "quiet since" times** [attention, with core exposing them]: `ready()` says which tasks are ready but not since when, and `tree()` doesn't say when a task (or its subtree) last changed or who changed it. Each row shows a time and an actor, and "Quiet 5+ days" needs the latest event across a subtree. Both can be found from events, but not cheaply for 2,000 tasks unless core returns the last event time and actor with each tree node.
