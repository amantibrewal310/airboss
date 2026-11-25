import { describe, expect, it } from 'vitest';
import { App } from '../../src/lib/state/app.svelte';
import { setTransport } from '../../src/lib/api/transport';
import { setClockOffset } from '../../src/lib/logic/clock';
import { createRecordedTransport, RECORDED_NOW } from '../../src/lib/mock';
import { loadRecording } from '../../src/lib/mock/fixtures';
import type { Bootstrap } from '../../src/lib/types';
import { wait } from './setup';

/**
 * The UI against the backend's own recording (cargo test -p airboss-app --test fixtures, driven by
 * app/tests/script.json): the reads are Rust's answers on DESIGN's sample data, and every write
 * must go out with exactly the args Rust recorded, in order.
 */
describe('the recorded script', () => {
  it('boots on Rust’s bootstrap and sends every scripted write exactly as recorded', async () => {
    const rec = loadRecording();
    const boot = rec.reads.bootstrap as Bootstrap;
    const errors: string[] = [];
    const replay = createRecordedTransport((w) => errors.push(w));
    setTransport(replay);
    setClockOffset(Date.parse(RECORDED_NOW) - Date.now());
    const app = new App();
    await app.start(false);
    expect(app.tasks.rows.size).toBe(boot.rows.length);
    expect(app.needs.visible.length).toBeGreaterThan(0);

    const step = () => wait(450);
    await app.acceptOne(209);
    await step();
    await app.rejectOne(210);
    await step();
    const q = app.needs.visible.find((n) => n.key === 'question:slot-picker')!;
    expect(q).toBeTruthy();
    await app.dismiss(q);
    await step();
    await app.undo();
    await step();
    await app.toggleDone(145);
    await step();
    await app.setStatus(146, 'blocked', 'Waiting on the mail queue');
    await step();
    app.go('all');
    app.select(146);
    await app.reorder('up');
    await step();
    await app.create({ title: 'Write the release notes', body: 'Draft for the booking launch', parent: null, repos: [1], links: [], then: 'none' });
    await step();
    await app.addNote(144, 'Use the existing date picker.');
    await step();
    await app.rename(152, 'Read prior art on agent fleets');
    await step();

    expect(errors).toEqual([]);
    expect(replay.remaining()).toBe(0);
    expect(app.tasks.rows.get(152)!.title).toBe('Read prior art on agent fleets');
    expect(app.tasks.rows.get(146)!.status).toBe('blocked');
  });
});
