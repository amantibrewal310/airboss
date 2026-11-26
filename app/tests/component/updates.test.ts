import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { page, userEvent } from 'vitest/browser';
import App from '../../src/windows/main/App.svelte';
import { fast, startApp, wait } from './setup';
import { UPDATE_NOTES_URL } from '../../src/lib/mock/fixtures';
import type { MockBackend } from '../../src/lib/mock';
import type { FixtureName } from '../../src/lib/mock/fixtures';
import type { UpdateStatus } from '../../src/lib/types';

/** SPEC-updates: the sidebar's update card, Settings' switch and Check now, and ⌘K's command. */

const V = '0.1.0-alpha.5';
const q = <T extends HTMLElement>(sel: string) => document.querySelector<T>(sel);
const card = () => q('[data-update]');
const button = (label: string, root: ParentNode = document) =>
  [...root.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent!.trim() === label);

async function open(fixture: FixtureName = 'update', install = 120) {
  const { app, backend } = await startApp(fixture, { ...fast, install });
  await render(App, { app });
  await wait(40);
  return { app, backend };
}

const emit = (backend: MockBackend, s: UpdateStatus) => {
  backend.s.updateStatus = s;
  backend.emit('update:changed', s);
};
const calls = (backend: MockBackend, command: string) => backend.calls.filter((c) => c.command === command);

describe('the update card', () => {
  it('stays out of the sidebar while there is nothing to say', async () => {
    await open('design');
    expect(card()).toBeNull();
    // The live region is there already, so the card is announced when it comes.
    expect(q('aside[data-zone="sidebar"] [aria-live="polite"]')).toBeTruthy();
  });

  it('offers an available update, above the connections line', async () => {
    await open();
    const c = card()!;
    expect(c.dataset.update).toBe('available');
    expect(c.querySelector('#update-title')!.textContent).toBe('Airboss 0.1.0-alpha.5 is available');
    expect([...c.querySelectorAll('button')].map((b) => b.textContent!.trim())).toEqual(['Update and restart', "What's new", 'Later']);
    expect(c.getAttribute('role')).toBe('group');
    // Directly above the footer, in the sidebar's order, and never wider than the sidebar.
    const side = q('aside[data-zone="sidebar"]')!;
    const live = c.parentElement!;
    expect(live.nextElementSibling!.classList.contains('conn')).toBe(true);
    expect(side.scrollWidth).toBeLessThanOrEqual(side.clientWidth);
    expect(c.getBoundingClientRect().right).toBeLessThanOrEqual(side.getBoundingClientRect().right);
  });

  it('Update and restart shows the download, then Installing… and Restarting…', async () => {
    const { backend } = await open('update', 400);
    const seen: string[] = [q('#update-title')!.textContent!];
    const values: (string | null)[] = [];
    const watch = new MutationObserver(() => {
      const t = q('#update-title')?.textContent;
      if (t && seen.at(-1) !== t) seen.push(t);
      const bar = q('[role="progressbar"]');
      if (bar && values.at(-1) !== bar.getAttribute('aria-valuenow')) values.push(bar.getAttribute('aria-valuenow'));
    });
    watch.observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true });
    const go = button('Update and restart')!;
    go.focus();
    go.click();
    await wait(30);
    expect(calls(backend, 'install_update')).toHaveLength(1);
    // The buttons are gone; focus stays on the card, not <body>.
    expect(document.activeElement).toBe(card());
    await wait(550);
    watch.disconnect();
    expect(seen).toEqual(['Airboss 0.1.0-alpha.5 is available', 'Downloading Airboss 0.1.0-alpha.5…', 'Installing Airboss 0.1.0-alpha.5…', 'Restarting…']);
    expect(values).toEqual(['0', '25', '50', '75', '100']);
    expect(card()!.dataset.update).toBe('restarting');
    expect(card()!.querySelectorAll('button')).toHaveLength(0);
  });

  it('gives the progress bar its value, label and range, and the percent beside it', async () => {
    const { backend } = await open();
    emit(backend, { state: 'downloading', version: V, percent: 42.4 });
    await wait(30);
    const bar = q('[role="progressbar"]')!;
    expect(bar.getAttribute('aria-label')).toBe('Downloading Airboss 0.1.0-alpha.5');
    expect(bar.getAttribute('aria-valuemin')).toBe('0');
    expect(bar.getAttribute('aria-valuemax')).toBe('100');
    expect(bar.getAttribute('aria-valuenow')).toBe('42');
    expect(card()!.querySelector('.pc')!.textContent).toBe('42%');
    expect(q('#update-title')!.textContent).toBe('Downloading Airboss 0.1.0-alpha.5…');
  });

  it('shows a download of unknown size as busy, without a value', async () => {
    const { backend } = await open();
    emit(backend, { state: 'downloading', version: V, percent: null });
    await wait(30);
    const bar = q('[role="progressbar"]')!;
    expect(bar.hasAttribute('aria-valuenow')).toBe(false);
    expect(bar.getAttribute('aria-valuemax')).toBe('100');
    expect(bar.classList.contains('busy')).toBe(true);
    expect(card()!.querySelector('.pc')).toBeNull();
  });

  it('says Installing… and Restarting… with nothing to press', async () => {
    const { backend } = await open();
    emit(backend, { state: 'installing', version: V });
    await wait(30);
    expect(q('#update-title')!.textContent).toBe('Installing Airboss 0.1.0-alpha.5…');
    expect(card()!.querySelectorAll('button, [role="progressbar"]')).toHaveLength(0);
    emit(backend, { state: 'restarting', version: V });
    await wait(30);
    expect(q('#update-title')!.textContent).toBe('Restarting…');
    expect(card()!.querySelectorAll('button')).toHaveLength(0);
  });

  it('says why an update failed, with Try again and Later', async () => {
    const { backend } = await open();
    emit(backend, { state: 'failed', version: V, message: "The download's signature didn't match, so it wasn't installed." });
    await wait(30);
    const c = card()!;
    expect(c.dataset.update).toBe('failed');
    expect(q('#update-title')!.textContent).toBe("Couldn't update to 0.1.0-alpha.5.");
    expect(c.querySelector('.msg')!.textContent).toBe("The download's signature didn't match, so it wasn't installed.");
    expect([...c.querySelectorAll('button')].map((b) => b.textContent!.trim())).toEqual(['Try again', 'Later']);
    button('Try again', c)!.click();
    await wait(30);
    expect(calls(backend, 'install_update')).toHaveLength(1);
    expect(card()!.dataset.update).toBe('downloading');
  });

  it('a failed install from the mock ends on the card', async () => {
    const { backend } = await open();
    backend.s.update = { ...backend.s.update!, installError: 'There isn’t enough space on the disk.' };
    button('Update and restart')!.click();
    await wait(250);
    expect(card()!.dataset.update).toBe('failed');
    expect(card()!.querySelector('.msg')!.textContent).toBe('There isn’t enough space on the disk.');
  });

  it('names every state as one plain sentence, with its spaces', async () => {
    const { backend } = await open();
    const cases: [UpdateStatus, string][] = [
      [{ state: 'available', version: '0.1.0-alpha.4.1', notes_url: UPDATE_NOTES_URL }, 'Airboss 0.1.0-alpha.4.1 is available'],
      [{ state: 'downloading', version: '0.1.0-alpha.4.1', percent: 10 }, 'Downloading Airboss 0.1.0-alpha.4.1…'],
      [{ state: 'installing', version: '0.1.0-alpha.4.1' }, 'Installing Airboss 0.1.0-alpha.4.1…'],
      [{ state: 'restarting', version: '0.1.0-alpha.4.1' }, 'Restarting…'],
      [{ state: 'failed', version: '0.1.0-alpha.4.1', message: 'The disk is full.' }, "Couldn't update to 0.1.0-alpha.4.1."],
    ];
    for (const [status, name] of cases) {
      emit(backend, status);
      await wait(30);
      // The computed name, as assistive technology gets it.
      await expect.element(page.getByRole('group', { name, exact: true })).toBeVisible();
      expect(card()!.getAttribute('aria-label')).toBe(name);
      // The live region reads the same sentence once: the split, visible title is hidden from it.
      expect(card()!.querySelector('.sr')!.textContent).toBe(name);
      expect(q('#update-title')!.getAttribute('aria-hidden')).toBe('true');
    }
  });

  it('lines up the text of a row’s first ghost button with the content edge', async () => {
    const { backend } = await open();
    const textLeft = (b: HTMLElement) => b.getBoundingClientRect().left + parseFloat(getComputedStyle(b).paddingLeft);
    const edge = q('#update-title')!.getBoundingClientRect().left;
    expect(button('Update and restart')!.getBoundingClientRect().left).toBeCloseTo(edge, 0);
    expect(textLeft(button("What's new")!)).toBeCloseTo(edge, 0);
    // The hit area keeps its padding.
    expect(parseFloat(getComputedStyle(button("What's new")!).paddingLeft)).toBe(8);
    emit(backend, { state: 'failed', version: V, message: 'The disk is full.' });
    await wait(30);
    expect(button('Try again')!.getBoundingClientRect().left).toBeCloseTo(q('#update-title')!.getBoundingClientRect().left, 0);
    expect(button('Later')!.getBoundingClientRect().left).toBeGreaterThan(button('Try again')!.getBoundingClientRect().right);
  });

  it('a check that returns after newer events never steps the card back', async () => {
    const { app, backend } = await startApp('update', { ...fast, lookup: 120 });
    await render(App, { app });
    await wait(40);
    const check = app.checkForUpdates();
    await wait(30);
    // An install started meanwhile reports progress before the check answers "available".
    backend.emit('update:changed', { state: 'downloading', version: V, percent: 50 });
    await check;
    await wait(30);
    expect(card()!.dataset.update).toBe('downloading');
    expect(q('[role="progressbar"]')!.getAttribute('aria-valuenow')).toBe('50');
  });

  it('moves focus to the connections line when the card goes while focus is in it', async () => {
    const { backend } = await open();
    button("What's new")!.focus();
    emit(backend, { state: 'idle' });
    await wait(40);
    expect(card()).toBeNull();
    expect(document.activeElement).toBe(q('.conn button'));
    expect(document.activeElement).not.toBe(document.body);
  });

  it('leaves focus alone when the card goes while focus is elsewhere', async () => {
    const { backend } = await open();
    const conn = q<HTMLButtonElement>('.conn button')!;
    const search = q<HTMLButtonElement>('button[aria-label="Jump"]')!;
    search.focus();
    emit(backend, { state: 'idle' });
    await wait(40);
    expect(card()).toBeNull();
    expect(document.activeElement).toBe(search);
    expect(document.activeElement).not.toBe(conn);
  });

  it('What’s new opens the release’s GitHub page through open_url', async () => {
    const { backend } = await open();
    button("What's new")!.click();
    await wait(30);
    expect(calls(backend, 'open_url').map((c) => c.args)).toEqual([{ target: { url: UPDATE_NOTES_URL } }]);
    expect(backend.opened).toContain(UPDATE_NOTES_URL);
    expect(UPDATE_NOTES_URL).toBe('https://github.com/amantibrewal310/airboss/releases/tag/v0.1.0-alpha.5');
  });

  it('Later hides it for this version, moving focus on to the connections line', async () => {
    const { app, backend } = await open();
    const later = button('Later', card()!)!;
    later.focus();
    later.click();
    await wait(30);
    expect(card()).toBeNull();
    expect(document.activeElement).toBe(q('.conn button'));
    // The same version again (another automatic check) stays hidden…
    emit(backend, { state: 'available', version: V, notes_url: UPDATE_NOTES_URL });
    await wait(30);
    expect(card()).toBeNull();
    expect(app.updates.later).toBe(V);
    // …a newer one shows.
    emit(backend, { state: 'available', version: '0.1.0-alpha.6', notes_url: UPDATE_NOTES_URL });
    await wait(30);
    expect(q('#update-title')!.textContent).toBe('Airboss 0.1.0-alpha.6 is available');
  });

  it('Later on a failure hides it too', async () => {
    const { backend } = await open();
    emit(backend, { state: 'failed', version: V, message: 'The disk is full.' });
    await wait(30);
    button('Later', card()!)!.click();
    await wait(30);
    expect(card()).toBeNull();
  });

  it('Space on a card button presses it, and never marks the selected task done', async () => {
    const { app, backend } = await open();
    app.go('all');
    app.select(144);
    await wait(30);
    const later = button('Later', card()!)!;
    later.focus();
    await userEvent.keyboard(' ');
    await wait(30);
    expect(card()).toBeNull();
    expect(calls(backend, 'toggle_done')).toHaveLength(0);
  });

  it('reports an install that could not start, as other errors are', async () => {
    const { app, backend } = await open('design');
    // The card shows an offer the backend no longer has.
    app.updates.set({ state: 'available', version: V, notes_url: UPDATE_NOTES_URL });
    await wait(30);
    button('Update and restart')!.click();
    await wait(30);
    expect(calls(backend, 'install_update')).toHaveLength(1);
    expect(app.toasts.list.at(-1)).toMatchObject({ text: "There's no update to install.", tone: 'warn' });
  });

  it('the Update and restart on Rust’s toast installs', async () => {
    const { app, backend } = await open('design');
    backend.s.update = { current: '0.1.0-alpha.4', latest: { version: V, notes_url: UPDATE_NOTES_URL } };
    await app.checkForUpdates();
    await wait(30);
    const t = app.toasts.list.at(-1)!;
    expect(t.text).toBe('Airboss 0.1.0-alpha.5 is available.');
    expect(card()!.dataset.update).toBe('available');
    t.actions.find((a) => a.label === 'Update and restart')!.run();
    await wait(60);
    expect(calls(backend, 'install_update')).toHaveLength(1);
    expect(card()!.dataset.update).toBe('downloading');
  });
});

describe('Settings › Check for updates automatically', () => {
  it('is on by default, and saves check_for_updates when turned off and on', async () => {
    const { app, backend } = await open('design');
    app.open({ kind: 'settings' });
    await wait(40);
    const toggle = q<HTMLButtonElement>('[role="switch"][aria-label="Check for updates automatically"]')!;
    expect(toggle.getAttribute('aria-checked')).toBe('true');
    expect(toggle.closest('.row')!.querySelector('.lab span')!.textContent).toBe('Airboss looks on GitHub for a new version every 6 hours. Nothing about you is sent.');
    toggle.click();
    await wait(30);
    expect(app.settings.check_for_updates).toBe(false);
    expect(toggle.getAttribute('aria-checked')).toBe('false');
    const saved = calls(backend, 'set_settings').at(-1)!.args as { settings: { check_for_updates: boolean } };
    expect(saved.settings.check_for_updates).toBe(false);
    toggle.click();
    await wait(30);
    expect(app.settings.check_for_updates).toBe(true);
  });

  it('Check now checks, says Checking… meanwhile, and Rust’s toast says the outcome', async () => {
    const { app, backend } = await startApp('design', { ...fast, lookup: 150 });
    await render(App, { app });
    await wait(40);
    app.open({ kind: 'settings' });
    await wait(40);
    const now = button('Check now')!;
    now.click();
    await wait(30);
    expect(now.textContent!.trim()).toBe('Checking…');
    expect(now.getAttribute('aria-disabled')).toBe('true');
    // A second press while it runs doesn't check twice.
    now.click();
    await wait(200);
    expect(calls(backend, 'check_for_updates')).toHaveLength(1);
    expect(now.textContent!.trim()).toBe('Check now');
    expect(app.toasts.list.at(-1)!.text).toBe('You have the newest Airboss, 0.1.0-alpha.4.');
    // Checking doesn't change the setting.
    expect(calls(backend, 'set_settings')).toHaveLength(0);
  });

  it('Check now after Later shows the card again', async () => {
    const { app } = await open();
    button('Later', card()!)!.click();
    await wait(30);
    expect(card()).toBeNull();
    app.open({ kind: 'settings' });
    await wait(40);
    button('Check now')!.click();
    await wait(60);
    expect(card()!.dataset.update).toBe('available');
  });
});

describe('⌘K › Check for updates', () => {
  it('is a palette command with no key, that checks and shows the card again', async () => {
    const { app, backend } = await open();
    button('Later', card()!)!.click();
    await wait(30);
    app.open({ kind: 'jump', mode: 'jump' });
    await wait(40);
    await userEvent.keyboard('check for updates');
    await wait(30);
    const options = [...document.querySelectorAll<HTMLElement>('[role="option"]')];
    const at = options.findIndex((o) => o.textContent!.trim() === 'Check for updates');
    expect(at).toBeGreaterThan(-1);
    // Palette only: no key beside it.
    expect(options[at]!.querySelector('kbd')).toBeNull();
    for (let i = 0; i < at; i++) await userEvent.keyboard('{ArrowDown}');
    expect(options[at]!.getAttribute('aria-selected')).toBe('true');
    await userEvent.keyboard('{Enter}');
    await wait(60);
    expect(calls(backend, 'check_for_updates')).toHaveLength(1);
    expect(card()!.dataset.update).toBe('available');
    expect(app.toasts.list.at(-1)!.text).toBe('Airboss 0.1.0-alpha.5 is available.');
  });
});
