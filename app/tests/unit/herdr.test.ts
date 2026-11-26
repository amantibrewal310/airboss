import { describe, expect, it } from 'vitest';
import { parseLinkBody, phrase } from '../../src/lib/logic/activity';
import { launcherMissingText, mergeConnections } from '../../src/lib/logic/connections';
import { threadSummary } from '../../src/lib/logic/copy';
import { linkLabel } from '../../src/lib/logic/links';
import { MockBackend } from '../../src/lib/mock/backend';
import { loadFixture } from '../../src/lib/mock/fixtures';
import type { Connection, HistoryEntry, ThreadDto } from '../../src/lib/types';

/** SPEC-herdr "app": how Herdr terminals and the launcher read. */

const text = (segs: { text: string }[]) => segs.map((s) => s.text).join('');

describe('Herdr terminals read like threads', () => {
  it('labels a link by its address, or its title', () => {
    expect(linkLabel({ kind: 'herdr_terminal', ref: 't4p', title: null, url: null })).toBe('Herdr · t4p');
    expect(linkLabel({ kind: 'herdr_terminal', ref: 't4p', title: 'Herdr · 179 Tmux', url: null })).toBe('Herdr · 179 Tmux');
  });

  it("reads core's link events and a launch into Herdr", () => {
    expect(parseLinkBody('herdr_terminal vps/t7')).toEqual({ kind: 'herdr_terminal', ref: 'vps/t7' });
    const e: HistoryEntry = { id: 1, task: 179, kind: 'linked', to_status: null, body: 'herdr_terminal t4p', actor: 'launch', at: '', thread_title: null };
    expect(text(phrase(e, 179, { threadTitle: (r) => (r === 't4p' ? '179 Tmux status line' : null), repoOf: () => 'dotfiles' }))).toBe(
      'Started thread 179 Tmux status line in dotfiles',
    );
    // Linked by me, it's a link like any other.
    expect(text(phrase({ ...e, actor: 'me' }, 179))).toBe('You linked Herdr · t4p');
  });

  it('says a terminal that finished is no longer in Herdr', () => {
    const t: ThreadDto = {
      key: { source: 'herdr', ref: 't4p' },
      state: 'gone',
      chip: null,
      title: null,
      request: null,
      error: null,
      project_path: null,
      url: null,
      last_activity_at: null,
      stale: false,
    };
    expect(threadSummary(t, 0)).toBe('no longer in Herdr');
    expect(threadSummary({ ...t, key: { source: 't3', ref: 'x' } }, 0)).toBe('no longer in T3');
  });
});

describe('connections with Herdr', () => {
  const c = (name: string, ok: boolean, state: string): Connection => ({ name, ok, state, detail: `${name} detail`, fix: ok ? null : 'fix it' });

  it('puts Herdr after T3 Code', () => {
    const merged = mergeConnections([c('GitHub', true, 'Signed in'), c('Herdr', true, 'Ready'), c('T3 Code', true, 'Found')], 'unknown');
    expect(merged.map((m) => m.name)).toEqual(['T3 Code', 'Herdr', 'GitHub']);
  });

  it("doesn't put back a T3 Code row Rust left out for someone on Herdr", () => {
    const missing = { missing: { reason: 'T3 Code isn’t installed.' } };
    expect(mergeConnections([c('Herdr', true, 'Ready')], missing, { launcher: 'herdr' }).map((m) => m.name)).toEqual(['Herdr']);
    // On T3 Code, its missing row is still added.
    expect(mergeConnections([c('Herdr', true, 'Ready')], missing, { launcher: 't3' }).map((m) => m.name)).toEqual(['T3 Code', 'Herdr']);
  });

  it('names the launcher when it is missing', () => {
    expect(launcherMissingText('herdr')).toBe('Do it and Plan it need Herdr. Install it, then choose Check again.');
    expect(launcherMissingText('t3')).toBe('Do it and Plan it need T3 Code. Install it, then choose Check again.');
  });
});

describe('the mock picks the launcher as Rust does', () => {
  it('starts in Herdr when T3 Code is missing and Herdr is ready, with a herdr_terminal link', async () => {
    const m = new MockBackend(loadFixture('herdr'), { delays: { launch: 0, attention: 0, reject: 0, lookup: 0 } });
    const boot = await m.invoke('bootstrap', {});
    expect(boot.launcher).toBe('herdr');
    expect(boot.connections.map((r) => r.name)).not.toContain('T3 Code');
    const { id } = await m.invoke('create_task', { task: { title: 'Try Herdr', body: null, parent: null, repos: [], links: [], then: 'none' } });
    const res = await m.invoke('do_it', { id, place: 'no_repo', again: false });
    if (!('started' in res)) throw new Error('expected a start');
    const started = res.started.report.started[0]!;
    expect(started.key.source).toBe('herdr');
    expect(m.s.links.find((l) => l.ref === started.key.ref)!.kind).toBe('herdr_terminal');
    // The user picks T3 Code: that's the launcher, and its missing error comes back.
    await m.invoke('set_settings', { settings: { ...m.s.settings, start_agents_in: 't3' } });
    await expect(m.invoke('do_it', { id, place: 'no_repo', again: true })).rejects.toMatchObject({ code: 't3_missing' });
  });
});
