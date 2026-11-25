import { describe, expect, it } from 'vitest';
import { classify, extractLinks, linkLabel } from '../../src/lib/logic/links';

/** SPEC-app "Pasting": every row of the table. */
describe('links.ts', () => {
  it('github.com/<o>/<r>/pull/<n> is a PR, ref o/r#n in lowercase', () => {
    expect(classify('https://github.com/Acme/Booking-API/pull/58')).toMatchObject({ kind: 'pr', ref: 'acme/booking-api#58' });
    expect(classify('https://github.com/acme/web/pull/7/files?diff=split')).toMatchObject({ kind: 'pr', ref: 'acme/web#7' });
  });
  it('github.com/<o>/<r>/issues/<n> is a GitHub issue', () => {
    expect(classify('https://github.com/acme/web/issues/12')).toMatchObject({ kind: 'github_issue', ref: 'acme/web#12' });
  });
  it('*.slack.com/archives/… is Slack', () => {
    const c = classify('https://acme.slack.com/archives/C04BOOKING/p1732012345');
    expect(c).toMatchObject({ kind: 'slack', url: 'https://acme.slack.com/archives/C04BOOKING/p1732012345' });
  });
  it('t3code://threads/<environment>/<thread> is a T3 thread, ref <thread>', () => {
    expect(classify('t3code://threads/env-local/slot-picker')).toMatchObject({ kind: 't3_thread', ref: 'slot-picker' });
  });
  it('an absolute or ~ path is a doc', () => {
    expect(classify('~/Code/web/docs/booking-flow.md')).toMatchObject({ kind: 'doc', ref: '~/Code/web/docs/booking-flow.md', url: null, label: 'booking-flow.md' });
    expect(classify('/Users/me/notes.md')).toMatchObject({ kind: 'doc', label: 'notes.md' });
  });
  it('any other https URL is a doc, with its URL', () => {
    expect(classify('https://example.com/spec/booking?x=1')).toMatchObject({ kind: 'doc', url: 'https://example.com/spec/booking?x=1', label: 'booking' });
  });
  it('a plain http URL stays text: Airboss opens only https links', () => {
    expect(classify('http://example.com/spec')).toBeNull();
    expect(extractLinks('see http://example.com/spec')).toEqual({ links: [], rest: 'see http://example.com/spec' });
  });
  it('anything else is not a link', () => {
    expect(classify('fix the slot picker')).toBeNull();
  });

  it('lifts links out of pasted text and keeps the rest', () => {
    const { links, rest } = extractLinks('Double booking https://acme.slack.com/archives/C1/p2 see thread');
    expect(links.map((l) => l.kind)).toEqual(['slack']);
    expect(rest).toBe('Double booking see thread');
    expect(extractLinks('~/Code/web/notes.md')).toMatchObject({ links: [{ kind: 'doc' }], rest: '' });
    expect(extractLinks('a / b and c/d')).toEqual({ links: [], rest: 'a / b and c/d' });
    expect(extractLinks('(t3code://threads/e/t1).').links[0]).toMatchObject({ kind: 't3_thread', ref: 't1' });
  });

  it('labels a link by its title, else by kind and ref', () => {
    expect(linkLabel({ kind: 'pr', ref: 'acme/web#61', title: null, url: null })).toBe('PR #61');
    expect(linkLabel({ kind: 't3_thread', ref: 'slot-picker', title: null, url: null })).toBe('T3 · slot-picker');
    expect(linkLabel({ kind: 'slack', ref: 'x', title: 'Slack · #booking', url: null })).toBe('Slack · #booking');
  });
});
