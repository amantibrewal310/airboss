import { describe, expect, it } from 'vitest';
import { startApp, wait } from './setup';

describe('the patch stream (SPEC-app "The view model and patches")', () => {
  it('applies a write’s patch once, though it also arrives as an event', async () => {
    const { app } = await startApp();
    const seq = app.tasks.seq;
    const untouched = app.tasks.rows.get(128);
    await app.toggleDone(146);
    expect(app.tasks.seq).toBe(seq + 1);
    expect(app.tasks.rows.get(146)!.status).toBe('done');
    await wait(10);
    expect(app.tasks.seq).toBe(seq + 1);
    // Unchanged rows keep their objects, so their components don't re-render.
    expect(app.tasks.rows.get(128)).toBe(untouched);
  });

  it('ignores a stale patch', async () => {
    const { app } = await startApp();
    const row = app.tasks.rows.get(144)!;
    app.onPatch({ seq: app.tasks.seq, upserts: [{ ...row, title: 'stale' }], removed: [], order: null, repos: null, created: [] });
    expect(app.tasks.rows.get(144)!.title).toBe('Web: slot picker');
  });

  it('highlights a created task and selects it', async () => {
    const { app } = await startApp();
    const id = await app.create({ title: 'Fix double booking', body: null, parent: 142, repos: [1], links: [], then: 'none' });
    expect(id).not.toBeNull();
    expect(app.tasks.fresh.has(id!)).toBe(true);
    expect(app.ui.selected).toBe(id);
    expect(app.ui.view).toBe('all');
    await wait(750);
    expect(app.tasks.fresh.has(id!)).toBe(false);
  });

  it('moves the selection to the row at the same index when the selected row leaves', async () => {
    const { app } = await startApp();
    app.go('progress');
    app.select(144);
    const index = app.visibleIds.indexOf(144);
    await app.toggleDone(144);
    expect(app.ui.selected).toBe(app.visibleIds[index]);
  });

  it('refetches the detail when a patch upserts the selected task, ignoring stale answers', async () => {
    const { app } = await startApp();
    app.go('all');
    app.select(146, { open: true });
    await wait(10);
    expect(app.detail.detail?.task.id).toBe(146);
    await app.addNote(146, 'Use the shared mail queue.');
    await wait(20);
    expect(app.detail.detail?.history[0]?.body).toBe('Use the shared mail queue.');
    const old = app.detail.detail!;
    app.select(144);
    expect(app.detail.accept({ ...old })).toBe(false);
  });
});

describe('Needs me: local hiding and the reject window', () => {
  it('slides an acted-on item out at once and keeps it hidden until attention drops it', async () => {
    const { app } = await startApp();
    const item = app.needs.visible.find((n) => n.kind === 'ready')!;
    void app.mainAction(item);
    expect(app.needs.leaving.has(item.key)).toBe(true);
    await wait(400);
    expect(app.needs.items.some((n) => n.key === item.key)).toBe(false);
  });

  it('brings an item back after 2 s if attention still lists it', async () => {
    const { app } = await startApp();
    const item = app.needs.visible.find((n) => n.kind === 'quiet')!;
    app.needs.hideLocally(item.key, 60);
    await wait(200);
    expect(app.needs.visible.some((n) => n.key === item.key)).toBe(false);
    await wait(120);
    expect(app.needs.visible.some((n) => n.key === item.key)).toBe(true);
  });

  it('hides rejected proposals for the 5 s window, and Undo brings them back', async () => {
    const { app, backend } = await startApp();
    const item = app.needs.visible.find((n) => n.kind === 'proposed')!;
    await app.rejectProposals(item, [153]);
    expect(app.tasks.hidden.has(153)).toBe(true);
    const undo = app.toasts.list.at(-1)!;
    expect(undo.text).toBe('Rejected #153');
    undo.actions[0]!.run();
    await wait(10);
    expect(app.tasks.hidden.has(153)).toBe(false);
    await wait(200);
    expect(backend.s.tasks.get(153)!.status).toBe('proposed');
  });

  it('commits a reject after the window', async () => {
    const { app, backend } = await startApp();
    const item = app.needs.visible.find((n) => n.kind === 'proposed')!;
    await app.rejectProposals(item, item.proposed.map(([id]) => id));
    await wait(250);
    expect(backend.s.tasks.get(153)!.status).toBe('dropped');
    expect(app.tasks.rows.get(153)!.status).toBe('dropped');
  });
});
