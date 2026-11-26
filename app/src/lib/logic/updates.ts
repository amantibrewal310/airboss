/*
  The sidebar's update card (SPEC-updates "What you see"): what it says for each UpdateStatus,
  and whether Later has hidden it.
*/

import type { UpdateStatus } from '../types';

export type UpdateCard =
  | { kind: 'available'; title: string; version: string; notesUrl: string }
  | { kind: 'downloading'; title: string; version: string; percent: number | null }
  | { kind: 'installing'; title: string; version: string }
  | { kind: 'restarting'; title: string; version: string }
  | { kind: 'failed'; title: string; version: string; message: string };

/** The version a status is about, or null when there's nothing to show. */
export function versionOf(s: UpdateStatus): string | null {
  return s.state === 'idle' ? null : s.version;
}

/** An install is under way: Later doesn't apply, and a Later from before is forgotten. */
export function isInstalling(s: UpdateStatus): boolean {
  return s.state === 'downloading' || s.state === 'installing' || s.state === 'restarting';
}

/** A whole percent from 0 to 100, or null while the size isn't known. */
export function wholePercent(p: number | null): number | null {
  if (p === null || !Number.isFinite(p)) return null;
  return Math.max(0, Math.min(100, Math.round(p)));
}

/**
 * What the card shows, or null when it's hidden: nothing to say (`idle`), or the offer or the
 * failure for the version Later was chosen on. A newer version shows again.
 */
export function updateCard(s: UpdateStatus, later: string | null): UpdateCard | null {
  switch (s.state) {
    case 'idle':
      return null;
    case 'available':
      if (later === s.version) return null;
      return { kind: 'available', title: `Airboss ${s.version} is available`, version: s.version, notesUrl: s.notes_url };
    case 'downloading':
      return { kind: 'downloading', title: `Downloading Airboss ${s.version}…`, version: s.version, percent: wholePercent(s.percent) };
    case 'installing':
      return { kind: 'installing', title: `Installing Airboss ${s.version}…`, version: s.version };
    case 'restarting':
      return { kind: 'restarting', title: 'Restarting…', version: s.version };
    case 'failed':
      if (later === s.version) return null;
      return { kind: 'failed', title: `Couldn't update to ${s.version}.`, version: s.version, message: s.message };
  }
}
