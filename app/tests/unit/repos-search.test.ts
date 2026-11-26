import { describe, expect, it } from 'vitest';
import { filterRepos } from '../../src/lib/logic/repos';

const r = (name: string, path: string) => ({ name, path });
const ITEMS = [
  r('airboss', '/Users/me/Workspace/Personal/Projects/airboss'),
  r('ivy-hub', '/Users/me/Workspace/Work/Projects/ivy-hub'),
  r('hub-ui', '/Users/me/Workspace/Work/Projects/ivy-hub/hub-ui'),
  r('goose', '/Users/me/Workspace/Work/Projects/goose'),
  r('ivyhomes_web', '/Users/me/Workspace/Work/Projects/ivy-web/ivyhomes_web'),
];
const names = (q: string) => filterRepos(ITEMS, q).map((x) => x.name);

describe("Add repos' search", () => {
  it('shows everything with no query', () => {
    expect(names('')).toEqual(ITEMS.map((x) => x.name));
    expect(names('   ')).toEqual(ITEMS.map((x) => x.name));
  });

  it('matches the name or the path, ignoring case', () => {
    expect(names('GOOSE')).toEqual(['goose']);
    expect(names('personal')).toEqual(['airboss']);
  });

  it('puts name matches before path-only matches, keeping their order', () => {
    // hub-ui's name holds "hub"; ivy-hub too; ivyhomes_web doesn't.
    expect(names('hub')).toEqual(['ivy-hub', 'hub-ui']);
    // "ivy": ivy-hub and ivyhomes_web by name, hub-ui only by its path.
    expect(names('ivy')).toEqual(['ivy-hub', 'ivyhomes_web', 'hub-ui']);
  });

  it('needs every word', () => {
    expect(names('ivy web')).toEqual(['ivyhomes_web']);
    expect(names('work goose')).toEqual(['goose']);
    expect(names('nothing-like-this')).toEqual([]);
  });
});
