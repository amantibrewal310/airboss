import type { Timestamp } from '../types';

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;
const WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

function parse(ts: Timestamp | null | undefined): number | null {
  if (!ts) return null;
  const n = Date.parse(ts);
  return Number.isNaN(n) ? null : n;
}

/** Row times: "now", "6m", "1h", "2d", "3w", then "4 Nov". */
export function relative(ts: Timestamp | null | undefined, now: number): string {
  const t = parse(ts);
  if (t === null) return '';
  const d = Math.max(0, now - t);
  if (d < MIN) return 'now';
  if (d < HOUR) return `${Math.floor(d / MIN)}m`;
  if (d < DAY) return `${Math.floor(d / HOUR)}h`;
  if (d < 7 * DAY) return `${Math.floor(d / DAY)}d`;
  if (d < 8 * 7 * DAY) return `${Math.floor(d / (7 * DAY))}w`;
  return dayMonth(t);
}

/**
 * A time in a sentence: "just now", "6m ago", "3w ago", then "on 4 Nov". Use this, never
 * `relative()` + " ago", which reads "now ago".
 */
export function ago(ts: Timestamp | null | undefined, now: number): string {
  const r = relative(ts, now);
  if (!r) return '';
  if (r === 'now') return 'just now';
  return /^\d+[mhdw]$/.test(r) ? `${r} ago` : `on ${r}`;
}

/** Spoken form for screen readers: "6 minutes ago". */
export function relativeLong(ts: Timestamp | null | undefined, now: number): string {
  const t = parse(ts);
  if (t === null) return '';
  const d = Math.max(0, now - t);
  const unit = (n: number, w: string) => `${n} ${w}${n === 1 ? '' : 's'} ago`;
  if (d < MIN) return 'just now';
  if (d < HOUR) return unit(Math.floor(d / MIN), 'minute');
  if (d < DAY) return unit(Math.floor(d / HOUR), 'hour');
  if (d < 7 * DAY) return unit(Math.floor(d / DAY), 'day');
  return `on ${dayMonth(t)}`;
}

/** Activity feed: "6m" and "2h" today, the weekday this week, else "25 Nov". */
export function feedTime(ts: Timestamp | null | undefined, now: number): string {
  const t = parse(ts);
  if (t === null) return '';
  const d = now - t;
  if (sameDay(t, now) || d < 6 * HOUR) return relative(ts, now);
  if (d < 6 * DAY) return WEEKDAYS[new Date(t).getDay()]!;
  return dayMonth(t);
}

/**
 * Clock times, for the native menu (it isn't redrawn as time passes) and "checked 10:44":
 * "10:32" today, "Mon 10:32" this week, "25 Nov" before that.
 */
export function clockTime(ts: Timestamp | null | undefined, now: number): string {
  const t = parse(ts);
  if (t === null) return '';
  const hm = new Date(t).toTimeString().slice(0, 5);
  if (sameDay(t, now)) return hm;
  if (now - t < 6 * DAY) return `${WEEKDAYS[new Date(t).getDay()]} ${hm}`;
  return dayMonth(t);
}

export function daysSince(ts: Timestamp | null | undefined, now: number): number {
  const t = parse(ts);
  return t === null ? 0 : Math.floor((now - t) / DAY);
}

function sameDay(a: number, b: number): boolean {
  const x = new Date(a);
  const y = new Date(b);
  return x.getFullYear() === y.getFullYear() && x.getMonth() === y.getMonth() && x.getDate() === y.getDate();
}

function dayMonth(t: number): string {
  const d = new Date(t);
  return `${d.getDate()} ${MONTHS[d.getMonth()]}`;
}
