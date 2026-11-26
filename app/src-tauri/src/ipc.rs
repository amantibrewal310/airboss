//! The Tauri side of the command layer.
//!
//! Each command is declared once in [`commands!`]: that makes its arguments struct (serde and
//! TypeScript), its async `#[tauri::command]` (which runs the plain function from
//! `src/commands/` in `spawn_blocking`, inside `catch_unwind`), the handler list, and
//! `dispatch`, which the fixtures recorder uses to call commands by name with JSON.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};

use airboss_core::{LinkId, RepoId, Status, TaskId};
use airboss_services::ThreadKey;

use crate::commands::tasks::{self, Direction};
use crate::commands::{app, detail, launch, needs, repos};
use crate::dto::{
    AddReposResult, Bootstrap, CopyWhat, CreateTaskInput, CreateTaskResult, DoItPlace, DoItResult,
    InstallCliResult, OpenTarget, Patch, PlanCard, QuickaddData, RemoveRepoResult, RunMap,
    T3ProjectRow, TaskDetail, UndoResult, UndoableWrite,
};
use crate::errors::{AppError, AppResult, ErrorCode};
use crate::menu::{FocusContext, WindowName};
use crate::settings::Settings;
use crate::state::{AppState, Caller, lock};

/// What every command reaches: the app state once the database is open.
///
/// If the database couldn't open at start, every command (`bootstrap` first) tries again,
/// and fails with `db_unavailable` while it still can't: that's the full-window state's
/// "Try again".
#[derive(Clone)]
pub struct App(Arc<AppInner>);

struct AppInner {
    state: Mutex<Option<Arc<AppState>>>,
    open: Box<dyn Fn() -> AppResult<Arc<AppState>> + Send + Sync>,
}

impl App {
    /// An app that opens its state with `open` on first use (and again after a failure).
    pub fn new(open: impl Fn() -> AppResult<Arc<AppState>> + Send + Sync + 'static) -> App {
        App(Arc::new(AppInner {
            state: Mutex::new(None),
            open: Box::new(open),
        }))
    }

    /// An app whose state is already open.
    pub fn ready(state: Arc<AppState>) -> App {
        App(Arc::new(AppInner {
            state: Mutex::new(Some(state)),
            open: Box::new(|| Err(AppError::new(ErrorCode::Internal, "already open"))),
        }))
    }

    /// The state, opening it if it isn't yet.
    pub fn get(&self) -> AppResult<Arc<AppState>> {
        let mut slot = lock(&self.0.state);
        if let Some(s) = slot.as_ref() {
            return Ok(s.clone());
        }
        let s = (self.0.open)()?;
        *slot = Some(s.clone());
        Ok(s)
    }

    /// The state if it's open.
    pub fn current(&self) -> Option<Arc<AppState>> {
        lock(&self.0.state).clone()
    }
}

/// Runs a command body, turning a panic into `AppError::Internal`.
pub fn guarded_call<T>(f: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(p) => {
            let what = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "a panic".into());
            Err(AppError::internal(format!("command panicked: {what}")))
        }
    }
}

/// In the database problem state (R2), every command but the few that state needs answers
/// with it, so nothing is saved into a database that will lose it.
fn gate(s: &AppState, command: &str) -> AppResult<()> {
    match s.db_problem() {
        Some(e) if !crate::dbhealth::WHILE_BROKEN.contains(&command) => Err(e),
        _ => Ok(()),
    }
}

/// Runs a command body off the main thread.
async fn run<T: Send + 'static>(
    app: App,
    command: &'static str,
    f: impl FnOnce(&Arc<AppState>) -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(move || {
        guarded_call(|| {
            let s = app.get()?;
            gate(&s, command)?;
            f(&s)
        })
    })
    .await
    .map_err(|e| AppError::internal(format!("command task failed: {e}")))?
}

fn bad_args(e: serde_json::Error) -> AppError {
    AppError::internal(format!("bad arguments: {e}"))
}

macro_rules! commands {
    ($(
        $name:ident($Args:ident) => $path:path,
            [$($(#[$m:meta])* $arg:ident: $ty:ty),* $(,)?] -> $ret:ty;
    )*) => {
        $(
            #[doc = concat!("The arguments of `", stringify!($name), "`, as `invoke` sends them.")]
            #[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
            #[serde(rename_all = "camelCase")]
            pub struct $Args {
                $( $(#[$m])* pub $arg: $ty, )*
            }

            #[doc = concat!("`", stringify!($name), "`, run off the main thread.")]
            #[allow(clippy::too_many_arguments)]
            #[tauri::command]
            pub async fn $name<R: tauri::Runtime>(
                window: tauri::Window<R>,
                app: tauri::State<'_, App>,
                $( $arg: $ty, )*
            ) -> Result<$ret, AppError> {
                let caller = Caller::from_label(window.label());
                let app = app.inner().clone();
                run(app, stringify!($name), move |s| $path(s, caller $(, $arg)*)).await
            }
        )*

        /// Every command, for the Tauri builder.
        pub fn handler<R: tauri::Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
            tauri::generate_handler![$($name),*]
        }

        /// Every command's name, in Appendix A's order.
        pub const NAMES: &[&str] = &[$(stringify!($name)),*];

        /// Runs a command by name with JSON arguments, as `invoke` would (without Tauri).
        pub fn dispatch(
            s: &Arc<AppState>,
            caller: Caller,
            command: &str,
            args: serde_json::Value,
        ) -> AppResult<serde_json::Value> {
            match command {
                $( stringify!($name) => {
                    #[allow(unused_variables)]
                    let a: $Args = serde_json::from_value(args).map_err(bad_args)?;
                    gate(s, command)?;
                    let out = guarded_call(|| $path(s, caller $(, a.$arg)*))?;
                    serde_json::to_value(out).map_err(|e| AppError::internal(e.to_string()))
                } )*
                other => Err(AppError::internal(format!("unknown command {other}"))),
            }
        }
    };
}

commands! {
    bootstrap(BootstrapArgs) => detail::bootstrap, [] -> Bootstrap;
    task_detail(TaskDetailArgs) => detail::task_detail,
        [id: TaskId, req: u32, history_limit: u32] -> TaskDetail;
    run_map(RunMapArgs) => detail::run_map, [id: TaskId, req: u32] -> RunMap;
    plans(PlansArgs) => detail::plans, [req: u32] -> Vec<PlanCard>;
    quickadd_data(QuickaddDataArgs) => detail::quickadd_data, [] -> QuickaddData;
    t3_projects(T3ProjectsArgs) => detail::t3_projects, [] -> Vec<T3ProjectRow>;
    herdr_folders(HerdrFoldersArgs) => detail::herdr_folders, [] -> Vec<T3ProjectRow>;
    create_task(CreateTaskArgs) => tasks::create_task, [task: CreateTaskInput] -> CreateTaskResult;
    rename_task(RenameTaskArgs) => tasks::rename_task, [id: TaskId, title: String] -> Patch;
    edit_notes(EditNotesArgs) => tasks::edit_notes, [id: TaskId, body: Option<String>] -> Patch;
    set_status(SetStatusArgs) => tasks::set_status,
        [id: TaskId, status: Status, note: Option<String>] -> UndoableWrite;
    toggle_done(ToggleDoneArgs) => tasks::toggle_done, [id: TaskId] -> UndoableWrite;
    accept_proposals(AcceptProposalsArgs) => tasks::accept_proposals, [ids: Vec<TaskId>] -> Patch;
    reject_proposals(RejectProposalsArgs) => tasks::reject_proposals,
        [ids: Vec<TaskId>, reason: Option<String>] -> UndoableWrite;
    move_task(MoveTaskArgs) => tasks::move_task,
        [id: TaskId, parent: Option<TaskId>, position: Option<u32>] -> UndoableWrite;
    reorder(ReorderArgs) => tasks::reorder, [id: TaskId, direction: Direction] -> UndoableWrite;
    link_repo(LinkRepoArgs) => tasks::link_repo, [id: TaskId, repo: RepoId] -> Patch;
    unlink_repo(UnlinkRepoArgs) => tasks::unlink_repo, [id: TaskId, repo: RepoId] -> Patch;
    add_repos(AddReposArgs) => repos::add_repos, [paths: Vec<String>] -> AddReposResult;
    edit_repo(EditRepoArgs) => repos::edit_repo,
        [repo: RepoId, name: String, path: String, remote: Option<String>] -> Patch;
    remove_repo(RemoveRepoArgs) => repos::remove_repo, [repo: RepoId] -> RemoveRepoResult;
    back_up_database(BackUpDatabaseArgs) => repos::back_up_database, [] -> Option<String>;
    pick_folder(PickFolderArgs) => repos::pick_folder, [] -> Option<String>;
    set_branch(SetBranchArgs) => tasks::set_branch, [id: TaskId, branch: Option<String>] -> Patch;
    add_dependency(AddDependencyArgs) => tasks::add_dependency, [id: TaskId, waits_on: TaskId] -> Patch;
    remove_dependency(RemoveDependencyArgs) => tasks::remove_dependency,
        [id: TaskId, waits_on: TaskId] -> Patch;
    add_link(AddLinkArgs) => repos::add_link,
        [id: TaskId, text: String, title: Option<String>] -> Patch;
    remove_link(RemoveLinkArgs) => repos::remove_link, [link: LinkId] -> Patch;
    add_note(AddNoteArgs) => tasks::add_note, [id: TaskId, text: String] -> Patch;
    do_it(DoItArgs) => launch::do_it,
        [id: TaskId, place: Option<DoItPlace>, again: Option<bool>] -> DoItResult;
    plan_it(PlanItArgs) => launch::plan_it, [id: TaskId, again: Option<bool>] -> DoItResult;
    add_to_t3(AddToT3Args) => launch::add_to_t3, [repo: RepoId] -> ();
    reopen_and_start(ReopenAndStartArgs) => launch::reopen_and_start, [id: TaskId] -> DoItResult;
    open_thread(OpenThreadArgs) => app::open_thread, [key: ThreadKey] -> ();
    open_url(OpenUrlArgs) => app::open_url, [target: OpenTarget] -> ();
    needs_dismiss(NeedsDismissArgs) => needs::needs_dismiss, [key: String] -> UndoableWrite;
    needs_snooze(NeedsSnoozeArgs) => needs::needs_snooze,
        [key: String, until: Option<String>] -> UndoableWrite;
    needs_restore(NeedsRestoreArgs) => needs::needs_restore, [key: String] -> ();
    undo(UndoArgs) => tasks::undo, [entry: Option<u64>] -> UndoResult;
    recheck_connections(RecheckConnectionsArgs) => app::recheck_connections, [] -> ();
    copy_text(CopyTextArgs) => app::copy_text, [what: CopyWhat] -> ();
    install_cli(InstallCliArgs) => app::install_cli, [] -> InstallCliResult;
    connect_claude(ConnectClaudeArgs) => app::connect_claude, [] -> ();
    get_settings(GetSettingsArgs) => app::get_settings, [] -> Settings;
    set_settings(SetSettingsArgs) => app::set_settings, [settings: Settings] -> Settings;
    notification_permission(NotificationPermissionArgs) => app::notification_permission,
        [] -> crate::notify::Permission;
    open_notification_settings(OpenNotificationSettingsArgs) => app::open_notification_settings,
        [] -> ();
    set_folded(SetFoldedArgs) => app::set_folded, [ids: Vec<TaskId>, folded: bool] -> ();
    set_focus_context(SetFocusContextArgs) => app::set_focus_context, [context: FocusContext] -> ();
    ui_ready(UiReadyArgs) => app::ui_ready, [window: WindowName] -> ();
    quickadd_shown(QuickaddShownArgs) => app::quickadd_shown, [] -> ();
    quickadd_hide(QuickaddHideArgs) => app::quickadd_hide, [] -> ();
    quickadd_resize(QuickaddResizeArgs) => app::quickadd_resize, [height: f64] -> ();
    show_main(ShowMainArgs) => app::show_main,
        [view: Option<String>, task: Option<TaskId>, needs_key: Option<String>] -> ();
}

include!("command_names.rs");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registered_commands_match_the_capability_list() {
        assert_eq!(NAMES, COMMAND_NAMES);
    }
}
