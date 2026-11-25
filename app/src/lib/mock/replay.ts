/*
  The strict replay transport for the Rust-recorded script (SPEC-app "Testing"): reads come
  from the read fixtures, writes are matched IN ORDER by exact command and args and answered
  with the recorded response, then the recorded events are sent. Anything else throws
  `UnscriptedCall`. Not used until `cargo test --test fixtures` writes app/tests/script.json.
*/

import type { CommandName, EventName, Events } from '../api/contract';
import type { Transport } from '../api/transport';
import { UnscriptedCall } from './backend';

export interface ScriptStep {
  command: string;
  args: unknown;
  response: unknown;
  /** A recorded error, instead of a response. */
  error?: unknown;
  events: { event: string; payload: unknown }[];
}

export interface Recording {
  /** Read commands answered from fixtures, any number of times, keyed by command. */
  reads: Record<string, unknown>;
  /** Writes, in order. */
  script: ScriptStep[];
}

function same(a: unknown, b: unknown): boolean {
  return JSON.stringify(sortKeys(a)) === JSON.stringify(sortKeys(b));
}

function sortKeys(v: unknown): unknown {
  if (Array.isArray(v)) return v.map(sortKeys);
  if (v && typeof v === 'object') {
    return Object.fromEntries(Object.entries(v as Record<string, unknown>).sort(([a], [b]) => a.localeCompare(b)).map(([k, x]) => [k, sortKeys(x)]));
  }
  return v;
}

export function createReplayTransport(rec: Recording): Transport & { remaining(): number } {
  let next = 0;
  const listeners = new Map<string, Set<(p: unknown) => void>>();
  return {
    native: false,
    remaining: () => rec.script.length - next,
    async invoke(command: CommandName, args: unknown) {
      if (command in rec.reads) return structuredClone(rec.reads[command]) as never;
      const step = rec.script[next];
      if (!step || step.command !== command || !same(step.args, args)) {
        throw new UnscriptedCall(`${command} ${JSON.stringify(args)} (expected ${step ? `${step.command} ${JSON.stringify(step.args)}` : 'nothing'})`);
      }
      next++;
      queueMicrotask(() => {
        for (const e of step.events) for (const h of listeners.get(e.event) ?? []) h(structuredClone(e.payload));
      });
      if (step.error !== undefined) throw structuredClone(step.error);
      return structuredClone(step.response) as never;
    },
    async listen<E extends EventName>(event: E, handler: (payload: Events[E]) => void) {
      const set = listeners.get(event) ?? new Set();
      set.add(handler as (p: unknown) => void);
      listeners.set(event, set);
      return () => set.delete(handler as (p: unknown) => void);
    },
  };
}
