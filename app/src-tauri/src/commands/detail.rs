//! The read commands: `bootstrap`, `task_detail`, `run_map`, `plans`, `quickadd_data`,
//! `t3_projects` and `herdr_folders`.

use std::collections::HashSet;
use std::path::Path;

use airboss_core::{LinkKind, PrState, Status, TaskId};
use airboss_services::{Mode, PrRef, Refresh, Source, SyncSnapshot, ThreadKey};

use crate::commands::app::cli_status;
use crate::dto::{
    Bootstrap, DepDto, HistoryEntry, LinkDto, ParentOption, PlanCard, QuickaddData, RunMap,
    T3ProjectRow, TaskDetail, ThreadDto, open, path_text,
};
use crate::errors::{AppResult, Ctx};
use crate::runmap;
use crate::state::{AppState, Caller, lock};
use crate::viewmodel::{self, ViewModel};

/// Everything the main window draws first.
pub fn bootstrap(s: &AppState, _c: Caller) -> AppResult<Bootstrap> {
    let settings = s.settings();
    let accent = s.shell.accent();
    let cli = cli_status(s);
    let cu = lock(&s.catchup);
    Ok(Bootstrap {
        seq: cu.seq,
        rows: cu.vm.rows_in_order(),
        order: cu.vm.order.clone(),
        repos: cu.vm.repos.clone(),
        needs: (*cu.vm.needs).clone(),
        connections: cu.connections.connections.clone(),
        availability: cu.connections.availability.clone(),
        herdr: cu.connections.herdr.clone(),
        launcher: cu.connections.launcher,
        agent_report: cu.connections.agent_report.clone(),
        settings,
        accent,
        cli,
        agents: cu.connections.agents,
    })
}

/// The title of the thread an agent actor (`agent:t3/97b5c8b5`) speaks for, if `sync` knows it.
pub fn thread_title(actor: &airboss_core::Actor, sync: &SyncSnapshot) -> Option<String> {
    match actor {
        airboss_core::Actor::Agent(r) => thread_key_of_agent(r)
            .and_then(|k| sync.threads.get(&k))
            .and_then(|t| t.title.clone()),
        _ => None,
    }
}

/// `agent:t3/97b5c8b5` names thread `97b5c8b5` in T3; `herdr/<address>` a Herdr terminal;
/// `claude/<id>` a Claude Code session.
fn thread_key_of_agent(r: &str) -> Option<ThreadKey> {
    let (source, id) = r.split_once('/')?;
    let source = match source {
        "t3" => Source::T3,
        "herdr" => Source::Herdr,
        "claude" => Source::Claude,
        _ => return None,
    };
    Some(ThreadKey {
        source,
        r#ref: id.to_string(),
    })
}

fn dep(vm: &ViewModel, id: TaskId) -> Option<DepDto> {
    vm.rows.get(&id).map(|r| DepDto {
        id,
        title: r.title.clone(),
        status: r.status,
        waiting: r.waiting,
    })
}

/// The panel's data for one task.
pub fn task_detail(
    s: &AppState,
    _c: Caller,
    id: TaskId,
    req: u32,
    history_limit: u32,
) -> AppResult<TaskDetail> {
    let ctx = Ctx::Task(id);
    let task = s.core_as(ctx, |st| st.task(id))?;
    let links = s.core_as(ctx, |st| st.links(id))?;
    let limit = history_limit.clamp(1, 500);
    let mut history = s.core_as(ctx, |st| st.history_with_subtasks(id, limit + 1))?;
    let history_more = history.len() > limit as usize;
    history.truncate(limit as usize);

    let sync = s.services.sync.snapshot();
    let (waits_on, blocks, waves, inherited_branch) = s.vm(|vm| {
        let waits_on: Vec<DepDto> = vm
            .snapshot
            .edges
            .iter()
            .filter(|(w, _)| *w == id)
            .filter_map(|&(_, b)| dep(vm, b))
            .collect();
        let blocks: Vec<DepDto> = vm
            .snapshot
            .edges
            .iter()
            .filter(|(_, b)| *b == id)
            .filter_map(|&(w, _)| dep(vm, w))
            .collect();
        let waves = (!vm.index.children_of(id).is_empty()).then(|| runmap::display_waves(vm, id));
        let branch = vm.rows.get(&id).and_then(|r| r.branch.clone());
        (waits_on, blocks, waves, branch)
    });

    let thread_view = |source: Source, r: &str| {
        let key = ThreadKey {
            source,
            r#ref: r.to_string(),
        };
        sync.threads
            .get(&key)
            .map(|t| ThreadDto::new(t, s.services.sync.thread_url(&key)))
    };
    let link_dtos = links
        .iter()
        .map(|l| LinkDto {
            id: l.id,
            task: l.task,
            kind: l.kind,
            r#ref: l.r#ref.clone(),
            url: l.url.clone(),
            pr_state: l.pr_state,
            title: l.title.clone(),
            linked_by: l.linked_by.to_string(),
            linked_at: l.linked_at.clone(),
            pr: (l.kind == LinkKind::Pr)
                .then(|| PrRef::parse(&l.r#ref))
                .flatten()
                .and_then(|r| sync.prs.get(&r).cloned()),
            thread: match l.kind {
                LinkKind::T3Thread => thread_view(Source::T3, &l.r#ref),
                LinkKind::HerdrTerminal => thread_view(Source::Herdr, &l.r#ref),
                LinkKind::ClaudeSession => thread_view(Source::Claude, &l.r#ref),
                _ => None,
            },
        })
        .collect();
    let history = history
        .iter()
        .map(|e| HistoryEntry {
            id: e.id,
            task: e.task,
            kind: e.kind,
            to_status: e.to_status,
            body: e.body.clone(),
            actor: e.actor.to_string(),
            at: e.at.clone(),
            thread_title: thread_title(&e.actor, &sync),
        })
        .collect();
    let needs = s.services.attention.for_task(id);
    let prepared = if matches!(
        task.status,
        Status::Proposed | Status::Done | Status::Dropped
    ) {
        None
    } else {
        // T3 missing and the like show through the connections and availability, not here.
        s.services.launch.prepare(id, Mode::DoIt).ok()
    };
    if links
        .iter()
        .any(|l| l.kind == LinkKind::Pr && l.pr_state == Some(PrState::Open))
    {
        s.services.sync.refresh(Refresh::PrsOf(id));
    }
    let branch = task
        .branch
        .clone()
        .or_else(|| prepared.as_ref().and_then(|p| p.branch.clone()))
        .or(inherited_branch);
    let suggested_prs = branch
        .map(|b| sync.suggested_prs(&b).into_iter().cloned().collect())
        .unwrap_or_default();

    Ok(TaskDetail {
        req,
        task,
        links: link_dtos,
        history,
        history_more,
        waits_on,
        blocks,
        waves,
        needs,
        prepared,
        suggested_prs,
    })
}

/// The run-order map of a parent's subtasks. `req` is the UI's request number; the map
/// itself doesn't carry it.
pub fn run_map(s: &AppState, _c: Caller, id: TaskId, _req: u32) -> AppResult<RunMap> {
    let order = s.core_as(Ctx::Task(id), |st| st.run_order(id))?;
    Ok(s.vm(|vm| runmap::run_map(vm, id, &order)))
}

/// One card per open top-level parent.
pub fn plans(s: &AppState, _c: Caller, _req: u32) -> AppResult<Vec<PlanCard>> {
    Ok(s.vm(viewmodel::plan_cards))
}

/// Quick add's repos and open parents, from the view model (no I/O).
pub fn quickadd_data(s: &AppState, _c: Caller) -> AppResult<QuickaddData> {
    Ok(s.vm(|vm| QuickaddData {
        repos: vm.repos.clone(),
        parents: vm
            .rows_in_order()
            .into_iter()
            .filter(|r| open(r.status) && r.status != Status::Proposed)
            .map(|r| ParentOption {
                id: r.id,
                title: r.title,
                status: r.status,
                waiting: r.waiting,
                depth: r.depth,
            })
            .collect(),
    }))
}

/// T3's projects for Add repos, each with whether it's a git repo and already added.
pub fn t3_projects(s: &AppState, _c: Caller) -> AppResult<Vec<T3ProjectRow>> {
    let sync = s.services.sync.snapshot();
    let added: HashSet<String> = s.vm(|vm| vm.repos.iter().map(|r| r.path.clone()).collect());
    Ok(sync
        .projects
        .iter()
        .map(|p| T3ProjectRow {
            path: path_text(&p.path),
            name: p.name.clone(),
            is_git: Path::new(&p.path).join(".git").exists(),
            added: added.contains(&path_text(&p.path)),
        })
        .collect())
}

/// Herdr's space folders, then its recent ones, for first run without T3 Code, with the same
/// rules as T3's projects. Nothing when Herdr can't be asked (not installed, window closed).
pub fn herdr_folders(s: &AppState, _c: Caller) -> AppResult<Vec<T3ProjectRow>> {
    let folders = match s.shell.herdr_folders().folders() {
        Ok(f) => f,
        Err(e) => {
            tauri_plugin_log::log::info!("no folders from Herdr: {e}");
            return Ok(Vec::new());
        }
    };
    let added: HashSet<String> = s.vm(|vm| vm.repos.iter().map(|r| r.path.clone()).collect());
    Ok(folders
        .into_iter()
        .map(|f| T3ProjectRow {
            path: path_text(&f.path),
            name: f.name,
            is_git: f.path.join(".git").exists(),
            added: added.contains(&path_text(&f.path)),
        })
        .collect())
}
