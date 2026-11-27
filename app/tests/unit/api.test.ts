import { describe, expect, it } from 'vitest';
import { api, on } from '../../src/lib/api';
import { COMMAND_NAMES, EVENT_NAMES } from '../../src/lib/api/contract';
import { setTransport, type Transport } from '../../src/lib/api/transport';

/** The command names, as Appendix A of SPEC-app.md lists them, SPEC-herdr's `herdr_folders` and SPEC-updates' three. */
const APPENDIX_A = `bootstrap task_detail run_map plans quickadd_data t3_projects herdr_folders create_task rename_task
edit_notes set_status toggle_done accept_proposals reject_proposals move_task reorder link_repo
unlink_repo add_repos edit_repo remove_repo back_up_database pick_folder set_branch set_due add_dependency remove_dependency add_link remove_link
add_note do_it plan_it add_to_t3 reopen_and_start open_thread open_url needs_dismiss needs_snooze
needs_restore undo recheck_connections copy_text install_cli connect_claude get_settings set_settings
notification_permission open_notification_settings set_folded
set_focus_context ui_ready quickadd_shown quickadd_hide quickadd_resize show_main
update_status check_for_updates install_update`.split(/\s+/);

const APPENDIX_B = `tree:patch needs:changed connections:changed menu:action nav:open quickadd:reset toast
appearance:changed error:background db:unavailable update:changed`.split(/\s+/);

function recording() {
  const calls: { command: string; args: unknown }[] = [];
  const listened: string[] = [];
  const t: Transport = {
    native: false,
    invoke: async (command, args) => {
      calls.push({ command, args });
      return null as never;
    },
    listen: async (event) => {
      listened.push(event);
      return () => {};
    },
  };
  setTransport(t);
  return { calls, listened };
}

describe('the command contract', () => {
  it('names every Appendix A command and every Appendix B event', () => {
    expect([...COMMAND_NAMES].sort()).toEqual([...APPENDIX_A].sort());
    expect([...EVENT_NAMES].sort()).toEqual([...APPENDIX_B].sort());
  });

  it('has one wrapper per command, each calling its own command name', async () => {
    const { calls } = recording();
    const fns = Object.values(api) as ((...a: unknown[]) => Promise<unknown>)[];
    expect(fns).toHaveLength(APPENDIX_A.length);
    for (const fn of fns) await fn(1, 2, 30);
    expect(new Set(calls.map((c) => c.command))).toEqual(new Set(APPENDIX_A));
  });

  it('passes the documented argument names', async () => {
    const { calls } = recording();
    await api.taskDetail(144, 7, 30);
    await api.addDependency(145, 144);
    await api.setStatus(203, 'blocked', 'Needs test keys');
    await api.doIt(146, { repo: 2 }, false);
    await api.showMain('needs', null, 'question:r1');
    expect(calls).toEqual([
      { command: 'task_detail', args: { id: 144, req: 7, historyLimit: 30 } },
      { command: 'add_dependency', args: { id: 145, waitsOn: 144 } },
      { command: 'set_status', args: { id: 203, status: 'blocked', note: 'Needs test keys' } },
      { command: 'do_it', args: { id: 146, place: { repo: 2 }, again: false } },
      { command: 'show_main', args: { view: 'needs', task: null, needsKey: 'question:r1' } },
    ]);
  });

  it('listens to events by their Appendix B names', async () => {
    const { listened } = recording();
    for (const e of EVENT_NAMES) await on(e, () => {});
    expect(listened).toEqual([...EVENT_NAMES]);
  });
});
