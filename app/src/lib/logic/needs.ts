import type { NeedsGroup, NeedsItem, NeedsKind } from '../types';
import type { IconName } from '../components/icons';
import { ago } from './time';

export const KIND_LABEL: Record<NeedsKind, string> = {
  question: 'Question',
  approval: 'Approval',
  review: 'Review',
  failed_run: 'Failed run',
  agent_stopped: 'Agent stopped',
  due: 'Due',
  proposed: 'Proposed subtasks',
  ready: 'Ready to start',
  quiet: 'Quiet 5+ days',
};

/** The card's and callout's lead: "Question · just now", "Agent stopped · 12m ago". */
export function needsLead(item: Pick<NeedsItem, 'kind' | 'since'>, now: number): string {
  return `${KIND_LABEL[item.kind]} · ${ago(item.since, now)}`;
}

/** The headline as the card shows it, under the kind label: "Ready to start: 143 is done" → "143 is done". */
export function headlineOf(item: { kind: NeedsKind; headline: string }): string {
  const lead = `${KIND_LABEL[item.kind]}: `;
  if (!item.headline.startsWith(lead)) return item.headline;
  const rest = item.headline.slice(lead.length);
  return rest.charAt(0).toUpperCase() + rest.slice(1);
}

export const KIND_ICON: Record<NeedsKind, IconName> = {
  question: 'question',
  approval: 'shield',
  review: 'pr',
  failed_run: 'failed',
  agent_stopped: 'stopped',
  due: 'calendar',
  proposed: 'proposed',
  ready: 'play',
  quiet: 'moon',
};

export const GROUP_LABEL: Record<NeedsGroup, string> = {
  questions: 'Questions',
  reviews: 'Reviews',
  failed_runs: 'Failed runs',
  stopped: 'Stopped',
  due: 'Due',
  proposed: 'Proposed',
  ready: 'Ready',
  quiet: 'Quiet',
};

export const GROUP_ORDER: NeedsGroup[] = ['questions', 'reviews', 'failed_runs', 'stopped', 'due', 'proposed', 'ready', 'quiet'];

/** The main action (↩ / ⌘↩) for each kind. */
export function mainLabel(kind: NeedsKind): string {
  switch (kind) {
    case 'question':
    case 'approval':
    case 'failed_run':
    case 'agent_stopped':
      return 'Open thread';
    case 'review':
      return 'Open PR';
    case 'proposed':
      return 'Accept all';
    case 'ready':
      return 'Do it';
    case 'due':
    case 'quiet':
      return 'Open';
  }
}

/** Kinds whose card offers Mark done (Space in Needs me). */
export function canMarkDone(kind: NeedsKind): boolean {
  return kind === 'agent_stopped' || kind === 'due';
}

/** The quieter action beside it: Dismiss for most, Snooze 3 days, Reject all. */
export function otherLabel(kind: NeedsKind): string {
  return kind === 'quiet' ? 'Snooze 3 days' : kind === 'proposed' ? 'Reject all' : 'Dismiss';
}

/** Items grouped in the fixed order, each group with its items. */
export function grouped(items: NeedsItem[]): { group: NeedsGroup; label: string; items: NeedsItem[] }[] {
  return GROUP_ORDER.map((group) => ({ group, label: GROUP_LABEL[group], items: items.filter((n) => n.group === group) })).filter(
    (g) => g.items.length,
  );
}
