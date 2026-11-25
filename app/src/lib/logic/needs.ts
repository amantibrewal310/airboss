import type { NeedsGroup, NeedsItem, NeedsKind } from '../types';
import type { IconName } from '../components/icons';

export const KIND_LABEL: Record<NeedsKind, string> = {
  question: 'Question',
  approval: 'Approval',
  review: 'Review',
  failed_run: 'Failed run',
  proposed: 'Proposed subtasks',
  ready: 'Ready to start',
  finish_it: 'Finish it?',
  quiet: 'Quiet 5+ days',
};

export const KIND_ICON: Record<NeedsKind, IconName> = {
  question: 'question',
  approval: 'shield',
  review: 'pr',
  failed_run: 'failed',
  proposed: 'proposed',
  ready: 'play',
  finish_it: 'flag',
  quiet: 'moon',
};

export const GROUP_LABEL: Record<NeedsGroup, string> = {
  questions: 'Questions',
  reviews: 'Reviews',
  failed_runs: 'Failed runs',
  proposed: 'Proposed',
  ready: 'Ready',
  finish_it: 'Finish it?',
  quiet: 'Quiet',
};

export const GROUP_ORDER: NeedsGroup[] = ['questions', 'reviews', 'failed_runs', 'proposed', 'ready', 'finish_it', 'quiet'];

/** The main action (↩ / ⌘↩) for each kind. */
export function mainLabel(kind: NeedsKind): string {
  switch (kind) {
    case 'question':
    case 'approval':
    case 'failed_run':
      return 'Open thread';
    case 'review':
      return 'Open PR';
    case 'proposed':
      return 'Accept all';
    case 'ready':
      return 'Do it';
    case 'finish_it':
      return 'Mark done';
    case 'quiet':
      return 'Open';
  }
}

/** The quieter action beside it: Dismiss for most, Keep open, Snooze 3 days, Reject all. */
export function otherLabel(kind: NeedsKind): string {
  return kind === 'finish_it' ? 'Keep open' : kind === 'quiet' ? 'Snooze 3 days' : kind === 'proposed' ? 'Reject all' : 'Dismiss';
}

/** Items grouped in the fixed order, each group with its items. */
export function grouped(items: NeedsItem[]): { group: NeedsGroup; label: string; items: NeedsItem[] }[] {
  return GROUP_ORDER.map((group) => ({ group, label: GROUP_LABEL[group], items: items.filter((n) => n.group === group) })).filter(
    (g) => g.items.length,
  );
}
