/*
  About 60 tasks over four repos: the design's data plus another plan with nested subtasks and
  a loop, dotfiles chores, a Claude Code session, a stale question, PRs in every state, done and
  dropped tasks, research with no repo, and a very long title. The default mock fixture.
*/

import type { MockSeed } from '../model';
import { DAY, HOUR, MIN } from './build';
import { AIRBOSS, API, DOTFILES, WEB, designBuilder, seedDefaults } from './design';

export function fullSeed(): MockSeed {
  const b = designBuilder();

  // Another plan, with a nested parent and two subtasks that wait on each other.
  b.task({ id: 160, title: 'Menu-bar app v1', status: 'doing', repos: [AIRBOSS], branch: 'task-160-menu-bar-app', created: 7 * DAY, body: 'A menu-bar item with the Needs me count, a native menu, and quick add from anywhere.' });
  b.task({ id: 161, title: 'Tray icon and count', status: 'done', parent: 160, repos: [AIRBOSS], created: 6 * DAY });
  b.task({ id: 162, title: 'Native menu', status: 'doing', parent: 160, repos: [AIRBOSS], created: 6 * DAY });
  b.task({ id: 163, title: 'Menu items from Needs me', status: 'doing', parent: 162, repos: [AIRBOSS], created: 5 * DAY });
  b.task({ id: 164, title: 'Clock times in the menu', parent: 162, repos: [AIRBOSS], created: 5 * DAY });
  b.task({ id: 165, title: 'Quick-add panel', parent: 160, repos: [AIRBOSS], created: 6 * DAY });
  b.task({ id: 166, title: 'Panel focus rules', parent: 160, repos: [AIRBOSS], created: 6 * DAY });
  b.task({ id: 171, title: 'Focus without activating the main window', parent: 166, repos: [AIRBOSS], created: 5 * DAY });
  b.task({ id: 167, title: 'Hotkey recorder', parent: 160, repos: [AIRBOSS], created: 6 * DAY });
  b.task({ id: 172, title: 'Dock badge', status: 'dropped', parent: 160, repos: [AIRBOSS], created: 6 * DAY });
  b.waits(164, 163).waits(165, 161).waits(167, 166).waits(171, 167);
  b.moved(161, 'done', 2 * DAY, 'sync:gh', 'PR #29 merged');
  b.moved(172, 'dropped', 3 * DAY, 'me');
  b.thread(163, 'menu-items', 'running', { repo: 'airboss', launchedAgo: 3 * HOUR, ago: 4 * MIN });
  b.moved(163, 'doing', 3 * HOUR, 'launch');
  b.readySince[165] = b.at(2 * DAY);

  b.thread(128, 'mcp-server', 'settled', {
    repo: 'airboss',
    launchedAgo: 3 * DAY,
    ago: 45 * MIN,
    reply: [
      'The MCP server now answers `list_tasks`, `get_task` and `update_task`, and the stdio tests pass: [PR #31](https://github.com/airboss/airboss/pull/31).',
      '',
      '**Next on your side:**',
      '1. Review and merge PR #31.',
      '2. Run `airboss mcp install` once, so T3 picks the server up.',
      '3. Decide whether `propose_tasks` should need your approval in the app.',
      '',
      'Once you decide on approval, tell me and I will add it.',
    ].join('\n'),
  });

  // dotfiles
  b.task({ id: 175, title: 'Shell prompt speed-up', status: 'done', repos: [DOTFILES], created: 12 * DAY });
  b.moved(175, 'done', 9 * DAY, 'me');
  b.task({ id: 176, title: 'Brewfile cleanup', status: 'dropped', repos: [DOTFILES], created: 12 * DAY });
  b.moved(176, 'dropped', 8 * DAY, 'me');
  b.task({ id: 177, title: 'Back up SSH keys', repos: [DOTFILES], created: 3 * DAY });
  b.readySince[177] = b.at(3 * DAY);
  b.task({ id: 178, title: 'Neovim LSP config', status: 'doing', repos: [DOTFILES], created: 4 * DAY });
  b.thread(178, 'nvim-lsp', 'running', { source: 'claude', repo: 'dotfiles', launchedAgo: 2 * HOUR, ago: 9 * MIN });
  b.moved(178, 'doing', 2 * HOUR, 'me');
  // An agent in a Herdr terminal (SPEC-herdr): its link is the terminal's address.
  b.task({ id: 179, title: 'Tmux status line', status: 'doing', repos: [DOTFILES], created: 2 * DAY });
  b.thread(179, 't4p', 'running', { source: 'herdr', title: '179 Tmux status line', repo: 'dotfiles', launchedAgo: HOUR, ago: 3 * MIN });
  b.moved(179, 'doing', HOUR, 'launch');

  // booking-api
  b.task({ id: 180, title: 'Rate limiting', status: 'doing', repos: [API], created: 5 * DAY });
  b.thread(180, 'rate-limit', 'needs_me', {
    repo: 'booking-api',
    launchedAgo: DAY,
    ago: 3 * HOUR,
    stale: true,
    ask: { id: 'r-rate-1', kind: 'question', text: 'Should limits apply per API key or per IP address?', ago: 3 * HOUR },
  });
  b.moved(180, 'doing', DAY, 'launch');
  b.task({ id: 181, title: 'API docs refresh', status: 'in_review', repos: [API], created: 5 * DAY });
  b.pr(181, 'acme/booking-api#63', 'open', { title: 'Refresh the API docs', add: 58, del: 140, checks: 'failing', review: 'changes_requested', ago: 5 * HOUR });
  b.moved(181, 'in_review', 5 * HOUR, 'agent:api-docs');
  b.task({ id: 182, title: 'Upgrade Postgres driver', status: 'blocked', repos: [API], blocked: 'Waiting for the 3.2 release', created: 6 * DAY });
  b.moved(182, 'blocked', 2 * DAY, 'me', 'Waiting for the 3.2 release');

  // research and writing, with no repo
  b.task({ id: 185, title: 'Compare hosted queues', created: 2 * DAY, body: 'SQS, Cloud Tasks and a Postgres-backed queue. Cost at 1M jobs a month, retries, and how each one fails.' });
  b.link(185, 'github_issue', 'acme/booking-api#71', { url: 'https://github.com/acme/booking-api/issues/71', title: 'Issue #71 · Pick a job queue', ago: 2 * DAY });
  b.readySince[185] = b.at(2 * DAY);
  b.task({ id: 186, title: 'Write launch post', created: DAY, body: 'Short and concrete: what it does, one screenshot, how to install.' });
  b.readySince[186] = b.at(DAY);
  b.task({ id: 187, title: 'Pricing notes', status: 'dropped', created: 10 * DAY });
  b.moved(187, 'dropped', 7 * DAY, 'me');

  // booking-web
  b.task({ id: 190, title: 'Accessibility audit', status: 'doing', repos: [WEB], created: 5 * DAY });
  b.task({ id: 191, title: 'Keyboard pass', status: 'in_review', parent: 190, repos: [WEB], created: 5 * DAY });
  b.task({ id: 192, title: 'Screen reader labels', parent: 190, repos: [WEB], created: 5 * DAY });
  b.task({ id: 193, title: 'Colour contrast', status: 'done', parent: 190, repos: [WEB], created: 5 * DAY });
  b.waits(192, 191);
  b.pr(191, 'acme/booking-web#66', 'open', { title: 'Keyboard pass', add: 77, del: 31, checks: 'pending', ago: 3 * HOUR });
  b.moved(191, 'in_review', 3 * HOUR, 'agent:keyboard-pass');
  b.moved(193, 'done', DAY, 'sync:gh', 'PR #64 merged');
  b.task({
    id: 194,
    title: 'Make the slot picker work with screen readers, keyboard only, and right-to-left languages without any layout shift',
    repos: [WEB],
    created: DAY,
  });
  b.readySince[194] = b.at(DAY);
  b.task({ id: 195, title: 'Set up CI caching', status: 'done', repos: [API], created: 15 * DAY });
  b.moved(195, 'done', 11 * DAY, 'sync:gh');
  b.task({ id: 196, title: 'Fix timezone bug in reminders', status: 'done', repos: [WEB], created: 9 * DAY });
  b.moved(196, 'done', 5 * DAY, 'sync:gh');
  b.task({ id: 197, title: 'Archive old threads', repos: [AIRBOSS], created: 30 * MIN });
  b.readySince[197] = b.at(30 * MIN);
  b.task({ id: 198, title: 'Profile list scrolling', status: 'doing', repos: [AIRBOSS], created: DAY });
  b.moved(198, 'doing', 50 * MIN, 'me');

  b.need({
    key: 'question:r-rate-1',
    kind: 'question',
    task: 180,
    since: b.at(3 * HOUR),
    headline: 'Should limits apply per API key or per IP address?',
    context: 'Agent in rate-limit asks',
    target: { thread: { source: 't3', ref: 'rate-limit' } },
    stale: true,
  });
  b.need({
    key: `ready:165:${b.at(2 * DAY)}`,
    kind: 'ready',
    task: 165,
    since: b.at(2 * DAY),
    headline: 'Ready to start: 161 is done',
    context: '161 merged · ready to start',
    target: { task: 165 },
  });
  return seedDefaults(b, 'full', [200, 110, 150, 160, 162, 166, 190]);
}
