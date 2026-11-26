import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { isInstalling, updateCard, versionOf, wholePercent } from '../../src/lib/logic/updates';
import { UpdateStore } from '../../src/lib/state/updates.svelte';
import { MockBackend } from '../../src/lib/mock';
import { loadFixture, UPDATE_NOTES_URL, UPDATE_VERSION } from '../../src/lib/mock/fixtures';
import type { MockUpdate } from '../../src/lib/mock/model';
import type { ToastEvent, UpdateStatus } from '../../src/lib/types';

const V = '0.1.0-alpha.5';
const NOTES = `https://github.com/amantibrewal310/airboss/releases/tag/v${V}`;
const available: UpdateStatus = { state: 'available', version: V, notes_url: NOTES };
const failed: UpdateStatus = { state: 'failed', version: V, message: "The download's signature didn't match, so it wasn't installed." };

/** SPEC-updates decision 5: the card's words in each state. */
describe('the update card', () => {
  it('says nothing while idle', () => {
    expect(updateCard({ state: 'idle' }, null)).toBeNull();
  });

  it('words every state as the spec does', () => {
    expect(updateCard(available, null)).toEqual({ kind: 'available', title: 'Airboss 0.1.0-alpha.5 is available', version: V, notesUrl: NOTES });
    expect(updateCard({ state: 'downloading', version: V, percent: 42 }, null)).toEqual({ kind: 'downloading', title: 'Downloading Airboss 0.1.0-alpha.5…', version: V, percent: 42 });
    expect(updateCard({ state: 'installing', version: V }, null)?.title).toBe('Installing Airboss 0.1.0-alpha.5…');
    expect(updateCard({ state: 'restarting', version: V }, null)?.title).toBe('Restarting…');
    expect(updateCard(failed, null)).toEqual({ kind: 'failed', title: "Couldn't update to 0.1.0-alpha.5.", version: V, message: "The download's signature didn't match, so it wasn't installed." });
  });

  it('shows a whole percent, or none while the size is unknown', () => {
    expect(updateCard({ state: 'downloading', version: V, percent: null }, null)).toMatchObject({ percent: null });
    expect(updateCard({ state: 'downloading', version: V, percent: 33.6 }, null)).toMatchObject({ percent: 34 });
    expect(wholePercent(-3)).toBe(0);
    expect(wholePercent(140)).toBe(100);
    expect(wholePercent(Number.NaN)).toBeNull();
  });

  it('Later hides the offer and the failure for that version only', () => {
    expect(updateCard(available, V)).toBeNull();
    expect(updateCard(failed, V)).toBeNull();
    // A newer version shows again.
    expect(updateCard({ ...available, version: '0.1.0-alpha.6' }, V)?.title).toBe('Airboss 0.1.0-alpha.6 is available');
    // An install that runs is always shown.
    expect(updateCard({ state: 'downloading', version: V, percent: 10 }, V)).not.toBeNull();
    expect(updateCard({ state: 'restarting', version: V }, V)).not.toBeNull();
  });

  it('knows each status’s version, and which ones are an install under way', () => {
    expect(versionOf({ state: 'idle' })).toBeNull();
    expect(versionOf(failed)).toBe(V);
    expect([available, failed, { state: 'idle' } as UpdateStatus].some(isInstalling)).toBe(false);
    expect(isInstalling({ state: 'installing', version: V })).toBe(true);
  });
});

describe('the update store', () => {
  it('Later hides until a manual check, a newer version, or an install', () => {
    const s = new UpdateStore();
    s.set(available);
    expect(s.card?.kind).toBe('available');
    s.hide();
    expect(s.later).toBe(V);
    expect(s.card).toBeNull();
    s.unhide();
    expect(s.card?.kind).toBe('available');

    s.hide();
    s.set({ ...available, version: '0.1.0-alpha.6' });
    expect(s.card?.title).toBe('Airboss 0.1.0-alpha.6 is available');

    // Update and restart from the toast after Later: the progress shows, and so does a failure after it.
    s.set(available);
    s.hide();
    s.set({ state: 'downloading', version: V, percent: 0 });
    expect(s.later).toBeNull();
    s.set(failed);
    expect(s.card?.kind).toBe('failed');
  });

  it('Later on nothing hides nothing', () => {
    const s = new UpdateStore();
    s.hide();
    expect(s.later).toBeNull();
  });
});

/** The mock answers the three commands as commands/updates.rs does. */
describe('mock updates', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  function backend(update?: MockUpdate, install = 600) {
    const seed = loadFixture('design');
    if (update) seed.update = update;
    const m = new MockBackend(seed, { delays: { launch: 0, attention: 0, reject: 50, lookup: 0, install } });
    const toasts: ToastEvent[] = [];
    const statuses: UpdateStatus[] = [];
    void m.listen('toast', (t) => toasts.push(t));
    void m.listen('update:changed', (s) => statuses.push(s));
    return { m, toasts, statuses };
  }
  const latest = { version: V, notes_url: NOTES };

  it('the update fixture starts with 0.1.0-alpha.5 available', async () => {
    const m = new MockBackend(loadFixture('update'));
    expect(await m.invoke('update_status', {})).toEqual({ state: 'available', version: UPDATE_VERSION, notes_url: UPDATE_NOTES_URL });
    expect(UPDATE_NOTES_URL).toBe(NOTES);
    // Every other fixture has nothing to say.
    expect(await new MockBackend(loadFixture('design')).invoke('update_status', {})).toEqual({ state: 'idle' });
  });

  it('a check that finds nothing says this is the newest', async () => {
    const { m, toasts, statuses } = backend();
    expect(await m.invoke('check_for_updates', {})).toEqual({ state: 'idle' });
    await vi.runAllTimersAsync();
    expect(toasts).toEqual([{ text: 'You have the newest Airboss, 0.1.0-alpha.4.', actions: [], undo: null }]);
    expect(statuses).toEqual([]);
  });

  it('a check that finds a release toasts it with Update and restart, and sends the status', async () => {
    const { m, toasts, statuses } = backend({ current: '0.1.0-alpha.4', latest });
    expect(await m.invoke('check_for_updates', {})).toEqual(available);
    await vi.runAllTimersAsync();
    expect(toasts).toEqual([
      { text: 'Airboss 0.1.0-alpha.5 is available.', actions: [{ label: 'Update and restart', command: 'install_update', args: {} }], undo: null },
    ]);
    expect(statuses).toEqual([available]);
  });

  it('a failed check warns with the reason; a development build says where updates work', async () => {
    const off = backend({ current: '0.1.0-alpha.4', latest, checkError: 'the network is offline.' });
    expect(await off.m.invoke('check_for_updates', {})).toEqual({ state: 'idle' });
    const dev = backend({ current: '0.1.0-alpha.4', latest, unavailable: true });
    await dev.m.invoke('check_for_updates', {});
    await vi.runAllTimersAsync();
    expect(off.toasts).toEqual([{ text: "Couldn't check for updates: the network is offline.", actions: [], undo: null, tone: 'warning' }]);
    expect(dev.toasts.map((t) => t.text)).toEqual(['Updates work only in the installed Airboss app.']);
    expect([...off.statuses, ...dev.statuses]).toEqual([]);
  });

  it('installs in steps: downloading 0→100, installing, restarting; a second install does nothing', async () => {
    const { m, statuses } = backend({ current: '0.1.0-alpha.4', latest, status: available });
    expect(await m.invoke('install_update', {})).toBeNull();
    expect(await m.invoke('install_update', {})).toBeNull();
    await vi.advanceTimersByTimeAsync(300);
    expect(statuses.at(-1)).toMatchObject({ state: 'downloading' });
    // A check while it runs changes nothing.
    await m.invoke('check_for_updates', {});
    await vi.runAllTimersAsync();
    expect(statuses).toEqual([
      { state: 'downloading', version: V, percent: 0 },
      { state: 'downloading', version: V, percent: 25 },
      { state: 'downloading', version: V, percent: 50 },
      { state: 'downloading', version: V, percent: 75 },
      { state: 'downloading', version: V, percent: 100 },
      { state: 'installing', version: V },
      { state: 'restarting', version: V },
    ]);
    expect(await m.invoke('update_status', {})).toEqual({ state: 'restarting', version: V });
  });

  it('takes about 1.5 s by default', async () => {
    const seed = loadFixture('update');
    const m = new MockBackend(seed);
    const statuses: UpdateStatus[] = [];
    void m.listen('update:changed', (s) => statuses.push(s));
    await m.invoke('install_update', {});
    await vi.advanceTimersByTimeAsync(1400);
    expect(statuses.at(-1)?.state).not.toBe('restarting');
    await vi.advanceTimersByTimeAsync(200);
    expect(statuses.at(-1)?.state).toBe('restarting');
  });

  it('a failed install ends on failed, and can be tried again', async () => {
    const { m, statuses } = backend({ current: '0.1.0-alpha.4', latest, status: available, installError: 'The disk is full.' });
    await m.invoke('install_update', {});
    await vi.runAllTimersAsync();
    expect(statuses.at(-1)).toEqual({ state: 'failed', version: V, message: 'The disk is full.' });
    await m.invoke('install_update', {});
    await vi.runAllTimersAsync();
    expect(statuses.filter((s) => s.state === 'downloading' && s.percent === 0)).toHaveLength(2);
  });

  it('with nothing to install, install_update is not_found', async () => {
    const { m } = backend();
    await expect(m.invoke('install_update', {})).rejects.toMatchObject({ code: 'not_found' });
  });
});
