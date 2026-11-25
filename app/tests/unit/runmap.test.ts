import { describe, expect, it } from 'vitest';
import { edgePath, layoutRunMap, mapFits, moveOnMap } from '../../src/lib/logic/runMapLayout';
import { phrase } from '../../src/lib/logic/activity';
import type { HistoryEntry, RunMap } from '../../src/lib/types';

/** DESIGN's Refunds example, as the Paper file draws it at 780px. */
const refunds: RunMap = {
  nodes: [201, 202, 203, 204, 205, 206, 207, 208],
  waves: [[201, 204, 206], [202, 203], [205], [207, 208]],
  edges: [
    { from: 201, to: 202, done: true },
    { from: 201, to: 203, done: true },
    { from: 202, to: 205, done: false },
    { from: 204, to: 205, done: false },
    { from: 205, to: 207, done: false },
  ],
  outside: [{ task: 208, blockers: [128] }],
  tangled: [],
};

describe('runMapLayout', () => {
  const l = layoutRunMap(refunds, 780);

  it('lays waves out as columns, stations 80px apart, at the design’s size', () => {
    expect(l.width).toBe(780);
    expect(l.height).toBe(276);
    expect(l.column).toBe(195);
    expect(l.dividers).toEqual([195, 390, 585]);
    expect(l.stations.find((s) => s.id === 204)).toMatchObject({ x: 14, y: 120 });
    expect(l.stations.find((s) => s.id === 208)).toMatchObject({ x: 599, y: 120 });
  });

  it('draws the Paper file’s paths: straight, curved before the task, and skipping a wave', () => {
    const d = (from: number, to: number) => l.paths.find((p) => p.from === from && p.to === to)!.d;
    expect(d(201, 202)).toBe('M23 40 H200');
    expect(d(201, 203)).toBe('M23 40 H165 C187 40 181 120 200 120');
    expect(d(204, 205)).toBe('M23 120 H163 C185 120 179 182 201 182 H358 C380 182 374 40 395 40');
    expect(l.paths.find((p) => p.outside)).toMatchObject({ to: 208, d: 'M547 120 H590', done: false });
  });

  it('keeps columns 120px or wider, scrolling instead of squeezing', () => {
    const wide: RunMap = { ...refunds, waves: Array.from({ length: 8 }, (_, i) => [i + 1]), edges: [], outside: [] };
    expect(layoutRunMap(wide, 780).column).toBe(120);
    expect(layoutRunMap(wide, 780).width).toBe(960);
    // Four waves in a 1100px window's page column (508px) fit without scrolling.
    expect(layoutRunMap(refunds, 508).width).toBe(508);
    expect(layoutRunMap(refunds, 508).column).toBe(127);
  });

  it('falls back to the Waves list above 40 subtasks or 8 waves', () => {
    expect(mapFits(refunds)).toBe(true);
    expect(mapFits({ nodes: Array.from({ length: 41 }, (_, i) => i), waves: [[1]] })).toBe(false);
    expect(mapFits({ nodes: [1], waves: Array.from({ length: 9 }, () => [1]) })).toBe(false);
  });

  it('moves across waves to the nearest row and within a wave', () => {
    expect(moveOnMap(refunds, null, 'down')).toBe(201);
    expect(moveOnMap(refunds, 204, 'right')).toBe(203);
    expect(moveOnMap(refunds, 206, 'right')).toBe(203);
    expect(moveOnMap(refunds, 205, 'left')).toBe(202);
    expect(moveOnMap(refunds, 201, 'down')).toBe(204);
    expect(moveOnMap(refunds, 201, 'up')).toBe(201);
    expect(moveOnMap(refunds, 207, 'right')).toBe(207);
  });

  it('bends a curve upwards as well as down', () => {
    expect(edgePath({ id: 1, wave: 0, slot: 1, x: 14, y: 120 }, { id: 2, wave: 1, slot: 0, x: 209, y: 40 }, 195)).toBe(
      'M23 120 H165 C187 120 181 40 200 40',
    );
  });
});

describe('activity phrases', () => {
  const e = (x: Partial<HistoryEntry>): HistoryEntry => ({
    id: 1, task: 144, kind: 'created', to_status: null, body: null, actor: 'me', at: '', thread_title: null, ...x,
  });
  const text = (segs: { text: string }[]) => segs.map((s) => s.text).join('');

  it('names the actor and the change', () => {
    expect(text(phrase(e({ kind: 'status', to_status: 'doing' }), 144))).toBe('You moved this to In progress');
    expect(text(phrase(e({ kind: 'note', actor: 'agent:slot-picker', thread_title: 'slot-picker', body: 'Asked about the picker' }), 144))).toBe(
      'Agent in slot-picker: Asked about the picker',
    );
    expect(text(phrase(e({ kind: 'linked', actor: 'launch', body: 'T3 · slot-picker' }), 144))).toBe('Started thread slot-picker');
  });

  it('starts a subtask’s events with its id', () => {
    expect(text(phrase(e({ task: 143, kind: 'status', to_status: 'done', actor: 'sync:gh', body: 'PR #58 merged' }), 142))).toBe(
      '143 moved to Done · PR #58 merged',
    );
  });
});
