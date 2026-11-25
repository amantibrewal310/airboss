//! The read commands: `bootstrap`, `task_detail`, `run_map`, `plans`, `quickadd_data` and
//! `t3_projects`.

use std::collections::HashSet;
use std::path::Path;

use airboss_core::{Actor, LinkKind, PrState, Status, TaskId};
use airboss_services::{Mode, PrRef, Refresh, Source, SyncSnapshot, ThreadKey};

use crate::dto::{
    ActorDto, ActorKind, Bootstrap, CrumbDto, DepDto, EventDto, LinkDto, NeedsDto, NeedsItemDto,
    ParentOption, PlansDto, PrDto, PreparedDto, QuickAddData, RepoDto, RunMap, T3ProjectDto,
    TaskDetail, TaskDto, ThreadDto, path_text,
};
use crate::errors::{AppError, AppResult, Ctx};
use crate::runmap;
use crate::settings::hotkey_label;
use crate::state::{AppState, Caller, lock};
use crate::viewmodel::{self, ViewModel};

/// Everything the main window draws first.
pub fn bootstrap(s: &AppState, _c: Caller) -> AppResult<Bootstrap> {
    let cu = lock(&s.catchup);
    let settings = s.settings();
    Ok(Bootstrap {
        seq: cu.seq,
        rows: cu.vm.rows_in_order(),
        order: cu.vm.order.clone(),
        repos: cu.vm.repo_dtos.clone(),
        needs: NeedsDto::from(&*cu.vm.needs),
        connections: cu.connections.connections.clone(),
        availability: cu.connections.availability.clone(),
        first_run: cu.vm.order.is_empty(),
        hotkey: hotkey_label(&settings.hotkey),
        settings,
        accent: s.shell.accent(),
        db_path: s
            .core(|st| Ok(st.path()))
            .ok()
            .flatten()
            .map(|p| path_text(&p)),
    })
}

/// Names an actor for the activity feed: "You", "Agent in slot-picker", "GitHub" …
pub fn actor_dto(actor: &Actor, sync: &SyncSnapshot) -> ActorDto {
    let kind = ActorKind::from(actor);
    let (name, thread) = match actor {
        Actor::Me => ("You".to_string(), None),
        Actor::Agent(r) => {
            let title = thread_key_of_agent(r)
                .and_then(|k| sync.threads.get(&k))
                .and_then(|t| t.title.clone());
            (
                match title {
                    Some(t) => format!("Agent in {t}"),
                    None => "Agent".to_string(),
                },
                Some(r.clone()),
            )
        }
        Actor::SyncGh => ("GitHub".to_string(), None),
        Actor::SyncT3 => ("T3".to_string(), None),
        Actor::Plan => ("Plan".to_string(), None),
        Actor::Launch => ("Launch".to_string(), None),
    };
    ActorDto { kind, name, thread }
}

/// `agent:t3/97b5c8b5` names thread `97b5c8b5` in T3; `claude/<id>` a Claude Code session.
fn thread_key_of_agent(r: &str) -> Option<ThreadKey> {
    let (source, id) = r.split_once('/')?;
    let source = match source {
        "t3" => Source::T3,
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
    let (ancestors, waits_on, blocks, waves, inherited_branch) = s.vm(|vm| {
        let mut ancestors = Vec::new();
        let mut at = task.parent;
        while let Some(p) = at {
            let Some(row) = vm.rows.get(&p) else { break };
            ancestors.push(CrumbDto {
                id: p,
                title: row.title.clone(),
            });
            at = row.parent;
        }
        ancestors.reverse();
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
        let waves =
            (!vm.index.children_of(id).is_empty()).then(|| runmap::display_waves(vm, id).to_dto());
        let branch = vm.rows.get(&id).and_then(|r| r.branch.clone());
        (ancestors, waits_on, blocks, waves, branch)
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
        .map(|l| {
            let pr = (l.kind == LinkKind::Pr)
                .then(|| PrRef::parse(&l.r#ref))
                .flatten()
                .and_then(|r| sync.prs.get(&r))
                .map(PrDto::from);
            let thread = match l.kind {
                LinkKind::T3Thread => thread_view(Source::T3, &l.r#ref),
                LinkKind::ClaudeSession => thread_view(Source::Claude, &l.r#ref),
                _ => None,
            };
            LinkDto::new(l, pr, thread)
        })
        .collect();
    let threads = sync
        .threads_for(id)
        .into_iter()
        .map(|t| ThreadDto::new(t, s.services.sync.thread_url(&t.key)))
        .collect();
    let prs = sync.prs_for(id).into_iter().map(PrDto::from).collect();
    let history = history
        .iter()
        .map(|e| EventDto::new(e, actor_dto(&e.actor, &sync)))
        .collect();
    let needs = s
        .services
        .attention
        .for_task(id)
        .iter()
        .map(NeedsItemDto::from)
        .collect();

    let (prepared, prepare_error) = if matches!(
        task.status,
        Status::Proposed | Status::Done | Status::Dropped
    ) {
        (None, None)
    } else {
        match s.services.launch.prepare(id, Mode::DoIt) {
            Ok(p) => (Some(PreparedDto::from(&p)), None),
            Err(e) => (None, Some(AppError::from(e))),
        }
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
        .map(|b| {
            sync.suggested_prs(&b)
                .into_iter()
                .map(PrDto::from)
                .collect()
        })
        .unwrap_or_default();

    Ok(TaskDetail {
        req,
        task: TaskDto::from(&task),
        ancestors,
        links: link_dtos,
        threads,
        prs,
        history,
        history_more,
        waits_on,
        blocks,
        waves,
        needs,
        prepared,
        prepare_error,
        suggested_prs,
    })
}

/// The run-order map of a parent's subtasks.
pub fn run_map(s: &AppState, _c: Caller, id: TaskId, req: u32) -> AppResult<RunMap> {
    let order = s.core_as(Ctx::Task(id), |st| st.run_order(id))?;
    Ok(s.vm(|vm| runmap::run_map(vm, id, &order, req)))
}

/// One card per open top-level parent.
pub fn plans(s: &AppState, _c: Caller, req: u32) -> AppResult<PlansDto> {
    Ok(PlansDto {
        req,
        cards: s.vm(viewmodel::plan_cards),
    })
}

/// Quick add's repos and open parents, from the view model (no I/O).
pub fn quickadd_data(s: &AppState, _c: Caller) -> AppResult<QuickAddData> {
    let hotkey = hotkey_label(&s.settings().hotkey);
    Ok(s.vm(|vm| QuickAddData {
        repos: vm.repo_dtos.clone(),
        parents: vm
            .rows_in_order()
            .into_iter()
            .filter(|r| r.status.is_open() && r.status != crate::dto::StatusDto::Proposed)
            .map(|r| ParentOption {
                id: r.id,
                title: r.title,
                depth: r.depth,
            })
            .collect(),
        hotkey,
    }))
}

/// T3's projects for Add repos, each with whether it's a git repo and already added.
pub fn t3_projects(s: &AppState, _c: Caller) -> AppResult<Vec<T3ProjectDto>> {
    let sync = s.services.sync.snapshot();
    let added: HashSet<String> = s.vm(|vm| vm.repos.iter().map(|r| r.path.clone()).collect());
    Ok(sync
        .projects
        .iter()
        .map(|p| T3ProjectDto {
            path: path_text(&p.path),
            name: p.name.clone(),
            is_git: Path::new(&p.path).join(".git").exists(),
            added: added.contains(&path_text(&p.path)),
        })
        .collect())
}

/// Every repo, for the UI after it adds some.
pub fn repo_dtos(s: &AppState) -> Vec<RepoDto> {
    s.vm(|vm| vm.repo_dtos.clone())
}
