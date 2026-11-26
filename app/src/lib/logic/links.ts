import type { Link, LinkKind } from '../types';

/** What a pasted link or path becomes (SPEC-app "Pasting"). */
export interface Classified {
  kind: LinkKind;
  ref: string;
  url: string | null;
  /** A label the UI can show before any title is known. */
  label: string;
}

const PR = /^https?:\/\/(?:www\.)?github\.com\/([A-Za-z0-9_.-]+)\/([A-Za-z0-9_.-]+)\/pull\/(\d+)(?:[/?#].*)?$/i;
const ISSUE = /^https?:\/\/(?:www\.)?github\.com\/([A-Za-z0-9_.-]+)\/([A-Za-z0-9_.-]+)\/issues\/(\d+)(?:[/?#].*)?$/i;
const SLACK = /^https?:\/\/[A-Za-z0-9-]+\.slack\.com\/archives\/([A-Za-z0-9]+)(?:\/[^\s]*)?$/i;
const T3 = /^t3code:\/\/threads\/([^/\s]+)\/([^/\s?#]+)\/?$/i;
/** Any other web link: https only, since Airboss opens nothing else (SPEC-app "Opening links"). */
const HTTP = /^https:\/\/[^\s]+$/i;
const PATH = /^(?:~\/|\/)[^\s]*$/;

/** Sorts one link or path by its form; null when it's neither. */
export function classify(input: string): Classified | null {
  const text = input.trim();
  let m: RegExpMatchArray | null;
  if ((m = text.match(PR))) {
    const ref = `${m[1]}/${m[2]}#${Number(m[3])}`.toLowerCase();
    return { kind: 'pr', ref, url: text, label: `PR #${Number(m[3])}` };
  }
  if ((m = text.match(ISSUE))) {
    const ref = `${m[1]}/${m[2]}#${Number(m[3])}`.toLowerCase();
    return { kind: 'github_issue', ref, url: text, label: `Issue #${Number(m[3])}` };
  }
  if ((m = text.match(SLACK))) {
    return { kind: 'slack', ref: text, url: text, label: 'Slack thread' };
  }
  if ((m = text.match(T3))) {
    return { kind: 't3_thread', ref: m[2]!, url: text, label: `T3 · ${m[2]}` };
  }
  if (HTTP.test(text)) {
    return { kind: 'doc', ref: text, url: text, label: basename(text.replace(/[?#].*$/, '')) || text };
  }
  if (PATH.test(text)) {
    return { kind: 'doc', ref: text, url: null, label: basename(text) || text };
  }
  return null;
}

const TOKEN = /(?:https?:\/\/|t3code:\/\/)[^\s<>"']+/gi;

/**
 * Splits pasted text into the links it holds and the rest. A paste that is only a path is a
 * doc link; otherwise only URLs are lifted out, so prose with slashes stays prose.
 */
export function extractLinks(pasted: string): { links: Classified[]; rest: string } {
  const trimmed = pasted.trim();
  if (PATH.test(trimmed)) {
    const c = classify(trimmed);
    return c ? { links: [c], rest: '' } : { links: [], rest: pasted };
  }
  const links: Classified[] = [];
  const rest = pasted.replace(TOKEN, (raw) => {
    const url = raw.replace(/[).,;:!?]+$/, '');
    const c = classify(url);
    if (!c) return raw;
    links.push(c);
    return raw.slice(url.length);
  });
  return { links, rest: rest.replace(/[ \t]{2,}/g, ' ').trim() };
}

function basename(path: string): string {
  const parts = path.replace(/\/+$/, '').split('/');
  return parts[parts.length - 1] ?? '';
}

/** The label a link tag shows: its title, else its kind and ref. */
export function linkLabel(link: Pick<Link, 'kind' | 'ref' | 'title' | 'url'>): string {
  if (link.title) return link.title;
  switch (link.kind) {
    case 'pr': {
      const n = link.ref.split('#')[1];
      return n ? `PR #${n}` : link.ref;
    }
    case 'github_issue': {
      const n = link.ref.split('#')[1];
      return n ? `Issue #${n}` : link.ref;
    }
    case 't3_thread':
      return `T3 · ${link.ref}`;
    case 'herdr_terminal':
      return `Herdr · ${link.ref}`;
    case 'claude_session':
      return `Claude · ${basename(link.ref) || link.ref}`;
    case 'slack':
      return 'Slack thread';
    case 'doc':
      return basename((link.url ?? link.ref).replace(/[?#].*$/, '')) || link.ref;
  }
}
