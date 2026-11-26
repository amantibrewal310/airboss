/*
  The design's sample data (docs/design/prototype.html and the Paper file): Booking flow,
  Refunds, MCP server, Dark mode tokens and the rest, at Wed 25 Nov 2026 10:44 UTC.
  Used for the screenshot baselines so they can be compared with the design PNGs.
*/

import type { Connection } from '../../types';
import type { MockSeed } from '../model';
import { DAY, HOUR, MIN, SeedBuilder } from './build';

export const NOW = '2026-11-25T10:44:00.000Z';

/** Repo ids give the design's colours: repo-((id − 1) mod 4) + 1. */
export const WEB = 1;
export const API = 2;
export const AIRBOSS = 3;
export const DOTFILES = 4;

export const CONNECTED: Connection[] = [
  { name: 'T3 Code', ok: true, state: 'Connected', detail: 'Do it and Plan it start agent threads here.', fix: null },
  { name: 'GitHub', ok: true, state: 'Signed in', detail: 'Pull requests sync every 5 minutes. A merged PR finishes its task.', fix: null },
  { name: 'T3 Code', ok: true, state: 'Live state', detail: 'Shows which agents are working or waiting on you.', fix: null },
  { name: 'Agents', ok: true, state: 'Ready', detail: 'Agents report progress and ask questions through Airboss. Last report 6m ago.', fix: null },
];

export function designBuilder(): SeedBuilder {
  const b = new SeedBuilder(NOW);
  b.repo(AIRBOSS, 'airboss').repo(WEB, 'booking-web').repo(API, 'booking-api').repo(DOTFILES, 'dotfiles');

  // Booking flow
  b.task({
    id: 142,
    title: 'Booking flow',
    status: 'doing',
    branch: 'task-142-booking-flow',
    body: 'Let people pick a slot and get a confirmation. Split across the API and the web app. The Slack thread has the product asks; the plan has the phases.',
    created: 4 * DAY,
  });
  b.task({ id: 143, title: 'API: slots endpoint', status: 'done', parent: 142, repos: [API], created: 3 * DAY });
  b.task({
    id: 144,
    title: 'Web: slot picker',
    status: 'doing',
    parent: 142,
    repos: [WEB],
    body: 'Keyboard support is a must: arrows move between slots, Return books. Use the design tokens from the dark mode work.',
    created: 3 * DAY,
  });
  b.task({ id: 145, title: 'Web: confirm screen', parent: 142, repos: [WEB], created: 3 * DAY });
  b.task({
    id: 146,
    title: 'Confirmation email',
    parent: 142,
    repos: [API],
    body: 'Send the confirmation once a slot is held. Use the existing mail queue, not a new provider. Include the .ics file and a cancel link.',
    created: 3 * DAY,
  });
  b.task({ id: 147, title: 'End-to-end tests', parent: 142, repos: [WEB, API], created: 3 * DAY });
  b.waits(145, 144).waits(146, 143).waits(147, 144, 145, 146);
  b.link(142, 'slack', 'https://acme.slack.com/archives/C04BOOKING/p1732012345', {
    title: 'Slack · #booking',
    url: 'https://acme.slack.com/archives/C04BOOKING/p1732012345',
    ago: 4 * DAY,
  });
  b.link(142, 'doc', '~/Code/booking-web/docs/booking-flow.md', { title: 'booking-flow.md', ago: 4 * DAY });
  b.link(144, 'slack', 'https://acme.slack.com/archives/C04BOOKING/p1732012345', {
    title: 'Slack · #booking',
    url: 'https://acme.slack.com/archives/C04BOOKING/p1732012345',
    ago: 3 * DAY,
  });
  b.moved(142, 'doing', 2 * DAY + 2 * HOUR, 'me');
  b.event(142, 'note', 2 * DAY + 3 * HOUR, 'plan', { body: 'Plan added 5 subtasks and 5 waits-on.' });
  b.thread(144, 'slot-picker', 'needs_me', {
    repo: 'booking-web',
    launchedAgo: 2 * DAY,
    ago: 6 * MIN,
    ask: { id: 'r-slot-1', kind: 'question', text: 'Use the existing date picker or build one? It needs full keyboard support.', ago: 6 * MIN },
  });
  b.moved(144, 'doing', 2 * DAY, 'launch');
  b.event(144, 'note', 6 * MIN, 'agent:slot-picker', { body: 'Asked whether to reuse the date picker.' });
  b.pr(143, 'acme/booking-api#58', 'merged', { title: 'Slots endpoint', add: 214, del: 12, ago: HOUR });
  b.moved(143, 'done', HOUR, 'sync:gh', 'PR #58 merged');
  b.event(145, 'waits_on_added', 2 * DAY, 'plan', { body: 'waits on #144' });
  b.event(147, 'waits_on_added', 2 * DAY, 'plan', { body: 'waits on #144, #145, #146' });
  b.readySince[146] = b.at(HOUR);

  // Refunds
  b.task({ id: 200, title: 'Refunds', status: 'doing', branch: 'task-200-refunds', body: 'Refunds from the booking page, end to end.', created: 3 * DAY + 4 * HOUR });
  b.task({ id: 201, title: 'Schema: refunds table', status: 'done', parent: 200, repos: [API], created: 3 * DAY });
  b.task({ id: 202, title: 'Refund service', status: 'doing', parent: 200, repos: [API], created: 3 * DAY });
  b.task({ id: 203, title: 'Payment webhook', status: 'blocked', parent: 200, repos: [API], blocked: 'Needs test keys from the payments dashboard', created: 3 * DAY });
  b.task({ id: 204, title: 'Refund button', status: 'in_review', parent: 200, repos: [WEB], created: 3 * DAY });
  b.task({ id: 205, title: 'Refund status page', parent: 200, repos: [WEB], created: 3 * DAY });
  b.task({ id: 206, title: 'Write refund policy copy', parent: 200, created: 3 * DAY });
  b.task({ id: 207, title: 'Announce to support', parent: 200, created: 3 * DAY });
  b.task({ id: 208, title: 'Backfill old refunds', parent: 200, repos: [API], created: 3 * DAY });
  b.waits(202, 201).waits(203, 201).waits(205, 202, 204).waits(207, 205).waits(208, 128);
  b.moved(201, 'done', DAY, 'sync:gh', 'PR #57 merged');
  b.thread(202, 'refund-service', 'running', { repo: 'booking-api', launchedAgo: DAY, ago: 2 * MIN });
  b.moved(202, 'doing', DAY, 'launch');
  b.event(202, 'note', 20 * MIN, 'agent:refund-service', { body: 'Refund rows are written; wiring the payment provider next.' });
  b.moved(203, 'blocked', 5 * HOUR, 'agent:payment-webhook', 'Needs test keys from the payments dashboard');
  b.pr(204, 'acme/booking-web#61', 'open', { title: 'Refund button', add: 96, del: 4, checks: 'passing', ago: 2 * HOUR });
  b.moved(204, 'in_review', 2 * HOUR, 'agent:refund-button');
  b.moved(200, 'doing', DAY, 'me');
  b.readySince[206] = b.at(DAY);

  // Top-level work
  b.task({ id: 128, title: 'MCP server', status: 'in_review', repos: [AIRBOSS], created: 6 * DAY });
  b.pr(128, 'acme/airboss#31', 'open', { title: 'list_tasks and get_task', add: 412, del: 38, checks: 'passing', review: 'required', ago: 40 * MIN });
  b.moved(128, 'in_review', 39 * MIN, 'agent:mcp-server');

  b.task({ id: 150, title: 'Worktree cleanup research', repos: [AIRBOSS], created: 2 * DAY });
  b.task({ id: 153, title: 'List stale worktrees older than 14 days', status: 'proposed', parent: 150, repos: [AIRBOSS], origin: 'agent', created: 3 * HOUR });
  b.task({ id: 154, title: 'Find worktrees whose branch has merged', status: 'proposed', parent: 150, repos: [AIRBOSS], origin: 'agent', created: 3 * HOUR });
  b.task({ id: 155, title: 'Write a dry-run cleanup command', status: 'proposed', parent: 150, repos: [AIRBOSS], origin: 'agent', created: 3 * HOUR });

  b.task({ id: 110, title: 'Dark mode tokens', status: 'doing', repos: [WEB], created: 8 * DAY });
  for (const [id, title] of [
    [111, 'Colour tokens'],
    [112, 'Swap hard-coded colours'],
    [113, 'Dark screenshots'],
    [114, 'Contrast check'],
  ] as const) {
    b.task({ id, title, status: 'done', parent: 110, repos: [WEB], created: 8 * DAY });
    b.moved(id, 'done', HOUR + (114 - id) * 3 * HOUR, 'sync:gh');
  }

  b.task({ id: 151, title: 'Rotate local MCP token', repos: [AIRBOSS, DOTFILES], created: 2 * DAY });
  b.task({ id: 152, title: 'Read sortie prior art', created: 4 * DAY });
  b.readySince[151] = b.at(2 * DAY);
  b.readySince[152] = b.at(4 * DAY);

  b.task({ id: 133, title: 'Menu-bar count', status: 'doing', repos: [AIRBOSS], created: 2 * DAY });
  b.thread(133, 'tray-count', 'needs_me', {
    repo: 'airboss',
    launchedAgo: DAY,
    ago: 12 * MIN,
    ask: { id: 'r-tray-1', kind: 'approval', text: 'cargo add tray-icon', ago: 12 * MIN },
  });
  b.moved(133, 'doing', DAY, 'launch');
  b.event(133, 'note', 12 * MIN, 'agent:tray-count', { body: 'Needs approval to add the tray-icon crate.' });

  b.task({ id: 131, title: 'Fix flaky login test', status: 'doing', repos: [WEB], created: 2 * DAY });
  b.thread(131, 'fix-flaky-login', 'errored', { repo: 'booking-web', launchedAgo: 5 * HOUR, ago: HOUR, error: '3 tests failed' });
  b.moved(131, 'doing', 5 * HOUR, 'launch');
  b.event(131, 'note', HOUR, 'sync:t3', { body: 'Run failed: 3 tests failed' });

  b.task({ id: 120, title: 'Payments refactor stack', status: 'in_review', repos: [API, WEB], created: 20 * DAY });
  b.pr(120, 'acme/booking-api#44', 'open', { title: 'Split the payment client', add: 380, del: 210, checks: 'passing', ago: 6 * DAY });
  b.pr(120, 'acme/booking-web#52', 'open', { title: 'Use the new payment client', add: 120, del: 96, checks: 'pending', ago: 6 * DAY });
  b.moved(120, 'in_review', 6 * DAY, 'agent:payments');

  b.task({ id: 98, title: 'New laptop setup', status: 'done', repos: [DOTFILES], created: 21 * DAY });
  b.moved(98, 'done', 14 * DAY, 'me');

  // Needs me: one item of every kind, in the design's order.
  b.need({
    key: 'question:r-slot-1',
    kind: 'question',
    task: 144,
    since: b.at(6 * MIN),
    headline: 'Use the existing date picker or build one? It needs full keyboard support.',
    context: 'Agent in slot-picker asks',
    target: { thread: { source: 't3', ref: 'slot-picker' } },
  });
  b.need({
    key: 'approval:r-tray-1',
    kind: 'approval',
    task: 133,
    since: b.at(12 * MIN),
    headline: 'Allow: cargo add tray-icon',
    context: 'Agent in tray-count wants approval',
    target: { thread: { source: 't3', ref: 'tray-count' } },
  });
  b.need({
    key: 'review:acme/airboss#31',
    kind: 'review',
    task: 128,
    since: b.at(40 * MIN),
    headline: 'PR #31 · list_tasks and get_task',
    context: '+412 −38 · checks passing · review required',
    target: { pr: { ref: 'acme/airboss#31', url: 'https://github.com/acme/airboss/pull/31' } },
  });
  b.need({
    key: 'failed:run-fix-flaky-login',
    kind: 'failed_run',
    task: 131,
    since: b.at(HOUR),
    headline: 'fix-flaky-login stopped: 3 tests failed',
    context: 'Thread stopped · 3 tests failed',
    target: { thread: { source: 't3', ref: 'fix-flaky-login' } },
  });
  b.need({
    key: 'proposed:150:155',
    kind: 'proposed',
    task: 150,
    since: b.at(3 * HOUR),
    headline: 'The agent proposed 3 subtasks',
    context: '3 proposed subtasks',
    target: { task: 150 },
    proposed: [
      [153, 'List stale worktrees older than 14 days'],
      [154, 'Find worktrees whose branch has merged'],
      [155, 'Write a dry-run cleanup command'],
    ],
  });
  b.need({
    key: `ready:146:${b.at(HOUR)}`,
    kind: 'ready',
    task: 146,
    since: b.at(HOUR),
    headline: 'Ready to start: 143 is done',
    context: '143 merged · ready to start',
    target: { task: 146 },
  });
  b.need({
    key: `finish:110:${b.at(HOUR)}`,
    kind: 'finish_it',
    task: 110,
    since: b.at(HOUR),
    headline: 'All 4 subtasks are done. Mark the parent done?',
    context: 'All 4 subtasks done',
    target: { task: 110 },
  });
  b.need({
    key: `quiet:120:${b.at(6 * DAY)}`,
    kind: 'quiet',
    task: 120,
    since: b.at(6 * DAY),
    headline: 'No activity for 6 days. 2 open PRs.',
    context: 'Quiet for 6 days',
    target: { task: 120 },
  });
  return b;
}

/** Positions follow the design's list order, not the order the builder added them. */
const DESIGN_ORDER = [142, 200, 128, 150, 110, 151, 152, 133, 131, 120, 98];

export function seedDefaults(b: SeedBuilder, name: string, folded: number[]): MockSeed {
  return b.build({
    name,
    now: NOW,
    connections: CONNECTED,
    availability: { ready: { version: '0.9.4' } },
    settings: { hotkey: 'Control+Alt+N', open_at_login: true, appearance: 'system', notifications: true, folded, start_agents_in: 'auto', herdr_agent: 'claude' },
    cli: { installed: true, path: '~/.local/bin/airboss', on_path: true },
    agents: { claude: true, codex: false },
    t3Projects: [
      { id: 'p-airboss', name: 'airboss', path: '~/Code/airboss' },
      { id: 'p-web', name: 'booking-web', path: '~/Code/booking-web' },
      { id: 'p-api', name: 'booking-api', path: '~/Code/booking-api' },
      { id: 'p-notes', name: 'notes', path: '~/Documents/notes' },
      { id: 'p-site', name: 'personal-site', path: '~/Code/personal-site' },
    ],
    gitFolders: ['~/Code/airboss', '~/Code/booking-web', '~/Code/booking-api', '~/Code/personal-site', '~/Code/dotfiles'],
    notProjects: [DOTFILES],
  });
}

export function designSeed(): MockSeed {
  const b = designBuilder();
  for (const t of b.tasks) {
    if (t.parent === null) t.position = DESIGN_ORDER.indexOf(t.id);
  }
  return seedDefaults(b, 'design', [200, 110, 150]);
}
