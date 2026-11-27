/*
  The run-order map's layout (DESIGN "Run-order map"): waves as columns, stations 80px apart in
  sibling order, lines from blocker to task that bend just before the task, wave skips that drop
  into the gap between rows, and short dashed stubs for outside blockers.
*/

import type { RunMap, TaskId } from '../types';

export const MAP_MIN_WIDTH = 460;
export const MAP_MAX_WIDTH = 780;
/** Columns narrow to this before the map scrolls sideways; four waves fit a 1100px window's page. */
export const MIN_COLUMN = 120;
export const ROW_GAP = 80;
export const FIRST_Y = 40;
/** Above this, the page opens on the Waves list and the Map tab says why. */
export const MAX_NODES = 40;
export const MAX_WAVES = 8;

export interface Station {
  id: TaskId;
  wave: number;
  slot: number;
  /** The station's centre. */
  x: number;
  y: number;
}

export interface MapPath {
  from: TaskId | null;
  to: TaskId;
  d: string;
  done: boolean;
  outside: boolean;
}

export interface MapLayout {
  width: number;
  height: number;
  column: number;
  stations: Station[];
  paths: MapPath[];
  heads: { x: number; label: string }[];
  dividers: number[];
}

export function mapFits(map: Pick<RunMap, 'nodes' | 'waves'>): boolean {
  return map.nodes.length <= MAX_NODES && map.waves.length <= MAX_WAVES;
}

export function layoutRunMap(map: RunMap, available: number): MapLayout {
  const count = Math.max(1, map.waves.length);
  let width = Math.max(MAP_MIN_WIDTH, Math.min(MAP_MAX_WIDTH, available));
  if (width / count < MIN_COLUMN) width = count * MIN_COLUMN;
  const column = width / count;
  const stations = new Map<TaskId, Station>();
  let rows = 0;
  map.waves.forEach((ids, wave) => {
    ids.forEach((id, slot) => {
      stations.set(id, { id, wave, slot, x: wave * column + 14, y: FIRST_Y + slot * ROW_GAP });
    });
    rows = Math.max(rows, ids.length);
  });
  const height = 30 + Math.max(1, rows) * ROW_GAP + 6;
  const paths: MapPath[] = [];
  for (const e of map.edges) {
    const a = stations.get(e.from);
    const b = stations.get(e.to);
    if (!a || !b || b.wave <= a.wave) continue;
    paths.push({ from: e.from, to: e.to, done: e.done, outside: false, d: edgePath(a, b, column) });
  }
  for (const o of map.outside) {
    const b = stations.get(o.task);
    if (!b) continue;
    paths.push({ from: null, to: o.task, done: false, outside: true, d: `M${b.x - 52} ${b.y} H${b.x - 9}` });
  }
  return {
    width,
    height,
    column,
    stations: [...stations.values()],
    paths,
    heads: map.waves.map((_, i) => ({ x: i * column + 6, label: ordinal(i + 1) })),
    dividers: Array.from({ length: count - 1 }, (_, i) => (i + 1) * column),
  };
}

const r = (n: number) => Math.round(n * 100) / 100;

/** A column's place in run order, "1st", "2nd", "3rd", "4th"…; the UI doesn't say "wave". */
export function ordinal(n: number): string {
  const tens = n % 100;
  const suffix = tens >= 11 && tens <= 13 ? 'th' : ({ 1: 'st', 2: 'nd', 3: 'rd' } as Record<number, string>)[n % 10] ?? 'th';
  return `${n}${suffix}`;
}

/** A line from blocker `a` to task `b`. */
export function edgePath(a: Station, b: Station, column: number): string {
  const ax = a.x;
  const bx = b.x;
  const start = `M${r(ax + 9)} ${a.y}`;
  if (b.wave - a.wave > 1) {
    // Skip a wave: drop into the gap between rows, run along it, rise into the task.
    const lane = Math.max(a.y, b.y) + 62;
    return `${start} H${r(ax + column - 46)} C${r(ax + column - 24)} ${a.y} ${r(ax + column - 30)} ${lane} ${r(ax + column - 8)} ${lane} H${r(bx - 46)} C${r(bx - 24)} ${lane} ${r(bx - 30)} ${b.y} ${r(bx - 9)} ${b.y}`;
  }
  if (a.y === b.y) return `${start} H${r(bx - 9)}`;
  // Neighbouring waves: bend with a smooth curve just before the task.
  return `${start} H${r(bx - 44)} C${r(bx - 22)} ${a.y} ${r(bx - 28)} ${b.y} ${r(bx - 9)} ${b.y}`;
}

/** Map keys: ←→ across waves (to the nearest row), ↑↓ within a wave. */
export function moveOnMap(map: RunMap, current: TaskId | null, key: 'up' | 'down' | 'left' | 'right'): TaskId | null {
  const waves = map.waves.filter((w) => w.length);
  if (!waves.length) return null;
  if (current === null) return waves[0]![0]!;
  const w = map.waves.findIndex((ids) => ids.includes(current));
  if (w === -1) return waves[0]![0]!;
  const slot = map.waves[w]!.indexOf(current);
  if (key === 'up' || key === 'down') {
    const ids = map.waves[w]!;
    return ids[Math.max(0, Math.min(ids.length - 1, slot + (key === 'down' ? 1 : -1)))]!;
  }
  for (let next = w + (key === 'right' ? 1 : -1); next >= 0 && next < map.waves.length; next += key === 'right' ? 1 : -1) {
    const ids = map.waves[next]!;
    if (ids.length) return ids[Math.min(slot, ids.length - 1)]!;
  }
  return current;
}
