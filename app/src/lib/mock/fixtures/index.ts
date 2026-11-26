/*
  THE ONE FIXTURE LOADER. Every mock fixture comes through `loadFixture()`.

  PLUG-IN POINT for the generated Rust fixtures: `cargo test -p airboss-app --test fixtures`
  will write the read fixtures and the recorded write script (see SPEC-app "Testing"). When
  they land, `loadFixture()` returns a `MockSeed` built from those files (or the app switches
  to `createReplayTransport()` in ../replay.ts with the recorded script), and the hand-made
  fixtures below are deleted. Nothing outside this folder imports a fixture directly.
*/

import type { MockSeed } from '../model';
import type { Recording } from '../replay';
import recorded from './recorded.json';
import { CONNECTED, designSeed, seedDefaults } from './design';
import { fullSeed } from './full';
import { perfSeed } from './perf';
import { SeedBuilder } from './build';

export type FixtureName = 'design' | 'full' | 'perf' | 'empty' | 'welcome' | 't3missing' | 'nothing' | 'dberror' | 'stopped';

export const FIXTURES: readonly FixtureName[] = ['design', 'full', 'perf', 'empty', 'welcome', 't3missing', 'nothing', 'dberror', 'stopped'];

export function isFixtureName(name: string): name is FixtureName {
  return (FIXTURES as readonly string[]).includes(name);
}

const T3_MISSING_REASON = 'Do it and Plan it need T3 Code. Install it, then choose Check again.';

export function loadFixture(name: FixtureName): MockSeed {
  switch (name) {
    case 'design':
      return designSeed();
    case 'full':
      return fullSeed();
    case 'perf':
      return perfSeed(2000);
    case 'nothing': {
      const seed = designSeed();
      seed.name = 'nothing';
      seed.needs = [];
      return seed;
    }
    case 't3missing': {
      const seed = designSeed();
      seed.name = 't3missing';
      seed.availability = { missing: { reason: T3_MISSING_REASON } };
      seed.connections = [
        { name: 'T3 Code', ok: false, state: 'Not found', detail: 'Do it and Plan it start agent threads here.', fix: 'Install T3 Code to turn on Do it and Plan it. Everything else works.' },
        ...CONNECTED.filter((c) => c.name !== 'T3 Code'),
      ];
      seed.t3Projects = [];
      return seed;
    }
    case 'empty':
    case 'welcome': {
      const b = new SeedBuilder('2026-11-25T10:44:00.000Z');
      const seed = seedDefaults(b, name, []);
      seed.cli = { installed: false, path: '~/.local/bin/airboss', on_path: true };
      // #9: installing the CLI isn't enough; no agent tool has the server yet.
      seed.agents = { claude: false, codex: false };
      seed.settings.open_at_login = true;
      if (name === 'empty') {
        seed.availability = { missing: { reason: T3_MISSING_REASON } };
        seed.t3Projects = [];
        seed.connections = [
          { name: 'Agents', ok: true, state: 'Ready', detail: 'Agents report progress and ask questions through Airboss. Each repo needs the agent note.', fix: null },
          { name: 'T3 Code', ok: false, state: 'Not found', detail: 'Do it and Plan it start agent threads here.', fix: 'Do it and Plan it are off until T3 Code is installed. Everything else works.' },
          { name: 'GitHub', ok: false, state: 'Not signed in', detail: 'Pull requests sync every 5 minutes.', fix: "PRs won't sync. Run `gh auth login`." },
        ];
      } else {
        seed.connections = [
          { name: 'Agents', ok: false, state: 'Not set up', detail: 'Agents report progress and ask questions through Airboss.', fix: 'Install the command-line tool, then copy the agent note into each repo.' },
          ...CONNECTED.filter((c) => c.name !== 'Agents'),
        ];
      }
      return seed;
    }
    case 'stopped': {
      // R5: Refund service's agent settled 20 minutes ago with no PR, so 202 is Agent stopped.
      const seed = designSeed();
      seed.name = 'stopped';
      const since = new Date(Date.parse(seed.now) - 20 * 60_000).toISOString();
      const thread = seed.threads.find((t) => t.view.key.ref === 'refund-service')!;
      thread.view = { ...thread.view, state: 'settled', last_activity_at: since };
      const at = seed.needs.findIndex((n) => n.kind === 'failed_run') + 1;
      seed.needs.splice(at, 0, {
        key: `stopped:202:${since}`,
        kind: 'agent_stopped',
        group: 'stopped',
        task: 202,
        task_title: 'Refund service',
        since,
        headline: 'Agent stopped · no PR yet',
        context: 'refund-service stopped',
        target: { thread: { source: 't3', ref: 'refund-service' } },
        proposed: [],
        stale: false,
      });
      return seed;
    }
    case 'dberror': {
      const seed = designSeed();
      seed.name = 'dberror';
      seed.dbError = {
        path: '~/Library/Application Support/Airboss/airboss.db',
        reason: 'The file is locked by another process.',
      };
      return seed;
    }
  }
}

/**
  The Rust-recorded fixtures (`cargo test -p airboss-app --test fixtures` writes
  ./recorded.json): every read command's answer on DESIGN's sample data, and each scripted write
  (app/tests/script.json if it exists, else app/src-tauri/tests/fixtures/script.json) with its
  exact args, response or error, and the events it sent. Feed it to `createReplayTransport()`.
*/
export function loadRecording(): Recording {
  return structuredClone(recorded) as unknown as Recording;
}
