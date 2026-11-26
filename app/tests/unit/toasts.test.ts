import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ACTION_TOAST_MS, isSticky, MAX_TOASTS, TOAST_MS, ToastStore } from '../../src/lib/state/toasts.svelte';
import type { UndoEntry } from '../../src/lib/types';

const run = () => {};
const retry = { text: "Couldn't start #1: scratch isn't a T3 project yet.", tone: 'warn' as const, actions: [{ label: 'Add to T3 and retry', run }] };
const undo = { id: 7 } as unknown as UndoEntry;

describe('toasts', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('an error whose fix is its action stays until it is closed', () => {
    const s = new ToastStore();
    const id = s.push(retry);
    expect(s.list[0]!.sticky).toBe(true);
    vi.advanceTimersByTime(60_000);
    expect(s.list.map((t) => t.id)).toEqual([id]);
    // Hovering and leaving doesn't start a countdown either.
    s.pause(id);
    s.resume(id);
    vi.advanceTimersByTime(60_000);
    expect(s.list).toHaveLength(1);
    s.dismiss(id);
    expect(s.list).toEqual([]);
  });

  it('only errors with an action stay', () => {
    expect(isSticky(retry)).toBe(true);
    expect(isSticky({ ...retry, tone: 'info' })).toBe(false);
    expect(isSticky({ ...retry, actions: [] })).toBe(false);
    // An undoable write isn't an error to fix: its Undo still times out.
    expect(isSticky({ ...retry, undo })).toBe(false);
  });

  it('other toasts keep their timing', () => {
    const s = new ToastStore();
    s.push({ text: 'Copied 146', tone: 'info', actions: [] });
    s.push({ text: 'Moved 146 to Done', tone: 'info', actions: [{ label: 'Undo', run }], undo });
    s.push({ text: 'Task 9 no longer exists.', tone: 'warn', actions: [] });
    s.push({ text: 'Started a thread for #1 in scratch', tone: 'info', actions: [{ label: 'Open thread', run }] });
    expect(s.list.some((t) => t.sticky)).toBe(false);
    vi.advanceTimersByTime(TOAST_MS);
    expect(s.list.map((t) => t.text)).toEqual(['Moved 146 to Done', 'Started a thread for #1 in scratch']);
    vi.advanceTimersByTime(ACTION_TOAST_MS - TOAST_MS);
    expect(s.list).toEqual([]);
  });

  it('a paused action toast still goes once it is left', () => {
    const s = new ToastStore();
    const id = s.push({ text: 'Started', tone: 'info', actions: [{ label: 'Open thread', run }] });
    s.pause(id);
    vi.advanceTimersByTime(ACTION_TOAST_MS * 2);
    expect(s.list).toHaveLength(1);
    s.resume(id);
    vi.advanceTimersByTime(ACTION_TOAST_MS);
    expect(s.list).toEqual([]);
  });

  it('a sticky toast still makes way when the stack is full', () => {
    const s = new ToastStore();
    s.push(retry);
    for (let i = 0; i < MAX_TOASTS; i++) s.push({ text: `n${i}`, tone: 'info', actions: [] });
    expect(s.list.map((t) => t.text)).toEqual(['n0', 'n1', 'n2', 'n3']);
    vi.advanceTimersByTime(TOAST_MS);
    expect(s.list).toEqual([]);
  });

  it('a sticky toast whose action stops being true goes', () => {
    const s = new ToastStore();
    s.push(retry, Date.now() + 1000);
    vi.advanceTimersByTime(1000);
    expect(s.list[0]!.actions).toEqual([]);
    expect(s.list[0]!.sticky).toBe(false);
    vi.advanceTimersByTime(1200);
    expect(s.list).toEqual([]);
  });
});
