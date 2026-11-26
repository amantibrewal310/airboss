//! One real Do it in Herdr: the app's `do_it` command, with "Start agents in" set to Herdr, on
//! the real `launch`, `sync` and `attention`, the real `git`, `herdr-app` and `hc`.
//!
//! Ignored: it opens a real Herdr tab running a real agent, in a space named after the
//! throwaway repo. Run it by hand, with Herdr open, on a temporary database whose task
//! `AIRBOSS_QA_TASK` is linked to a throwaway git repo:
//!
//! ```text
//! AIRBOSS_DB=/tmp/qa/herdr/airboss.db AIRBOSS_QA_TASK=1 [AIRBOSS_QA_AGENT=codex] \
//!   cargo test -p airboss-app --test real_herdr -- --ignored --nocapture
//! ```
//!
//! It checks the link (`herdr_terminal`, by `launch`), the branch, the worktree, `doing`, and
//! then prints the row's live chip as sync follows the terminal, for up to two minutes. It
//! leaves the tab and the worktree for you to look at; close the space in Herdr and remove the
//! worktree (`git worktree remove`) afterwards.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use airboss_app::commands::app::{get_settings, set_settings};
use airboss_app::commands::launch::do_it;
use airboss_app::dto::DoItResult;
use airboss_app::settings::StartAgentsIn;
use airboss_app::{AppConfig, AppState, Caller, RecordingShell, Services, share};
use airboss_core::{LinkKind, Status, Store, TaskId};
use airboss_launch::{CoreAccess, LaunchConfig, LaunchTools};
use airboss_services::{
    AttentionService, Availability, HerdrAgent, LaunchService, Source, SystemClock,
};

fn wait_for<T>(what: &str, limit: Duration, mut f: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(v) = f() {
            println!("{what}: after {:?}", start.elapsed());
            return v;
        }
        assert!(start.elapsed() < limit, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[test]
#[ignore = "opens a real Herdr tab running a real agent; run by hand with Herdr open"]
fn one_real_do_it_in_herdr() {
    let db =
        PathBuf::from(std::env::var("AIRBOSS_DB").expect("set AIRBOSS_DB to a temporary file"));
    assert!(
        !db.to_string_lossy().contains("Application Support/Airboss"),
        "never the real database"
    );
    let task = TaskId(
        std::env::var("AIRBOSS_QA_TASK")
            .expect("set AIRBOSS_QA_TASK to the task linked to the throwaway repo")
            .parse()
            .unwrap(),
    );
    let agent = match std::env::var("AIRBOSS_QA_AGENT").as_deref() {
        Ok("codex") => HerdrAgent::Codex,
        _ => HerdrAgent::Claude,
    };
    let home = airboss_app::services::home();
    let store = share(Store::open(&db).unwrap());

    // The services exactly as the app builds them.
    let sync_cfg = airboss_sync::SyncConfig::defaults(&home);
    let sync: Arc<dyn airboss_services::SyncService> = airboss_sync::start(
        sync_cfg.clone(),
        store.clone(),
        airboss_sync::Tools::real(&sync_cfg),
    )
    .unwrap();
    let launch_cfg = LaunchConfig::defaults(&home);
    let tools = LaunchTools::real(&launch_cfg);
    let launch = airboss_launch::start(
        launch_cfg,
        CoreAccess::new(store.clone()),
        sync.clone(),
        tools,
    )
    .unwrap();
    let attention = airboss_attention::start(
        airboss_attention::AttentionConfig::default(),
        store.clone(),
        sync.clone(),
        Arc::new(SystemClock),
    )
    .unwrap();
    let services = Services {
        sync: sync.clone(),
        launch: launch.clone(),
        attention: attention.clone(),
    };
    let state = AppState::new(
        store.clone(),
        services,
        Arc::new(RecordingShell::new()),
        Arc::new(SystemClock),
        AppConfig::new(Default::default(), None),
    )
    .unwrap();
    state.start_background();

    // Settings › Agent setup: start agents in Herdr, with this agent.
    let mut settings = get_settings(&state, Caller::Main).unwrap();
    settings.start_agents_in = StartAgentsIn::Herdr;
    settings.herdr_agent = agent;
    set_settings(&state, Caller::Main, settings).unwrap();

    let version = wait_for("Herdr available", Duration::from_secs(20), || match launch
        .herdr_availability()
    {
        Availability::Ready { version } => Some(version),
        _ => None,
    });
    println!("Herdr {version}, agent {agent:?}");

    // Do it, as the main window's button calls it.
    let started = Instant::now();
    let report = match do_it(&state, Caller::Main, task, None, None) {
        Ok(DoItResult::Started { report, .. }) => report,
        other => panic!("do_it: {other:?}"),
    };
    println!("do_it took {:?}: {report:?}", started.elapsed());
    assert_eq!(report.started.len(), 1, "{report:?}");
    assert!(report.failed.is_empty(), "{report:?}");
    let thread = report.started[0].key.clone();
    assert_eq!(thread.source, Source::Herdr);
    println!("TERMINAL {}", thread.r#ref);

    // core: the link, the branch and doing.
    let (links, t) = {
        let s = store.lock().unwrap();
        (s.links(task).unwrap(), s.task(task).unwrap())
    };
    let link = links
        .iter()
        .find(|l| l.kind == LinkKind::HerdrTerminal)
        .expect("a herdr_terminal link");
    assert_eq!(link.r#ref, thread.r#ref);
    assert_eq!(link.linked_by.to_string(), "launch");
    let branch = t.branch.clone().expect("the branch is set");
    println!("branch {branch}; status {:?}", t.status);
    assert_eq!(t.status, Status::Doing);

    // The live chip, as sync follows the terminal: print each change for up to two minutes,
    // or until the agent has stopped after working.
    let mut last = None;
    let mut worked = false;
    let watch = Instant::now();
    while watch.elapsed() < Duration::from_secs(120) {
        let view = sync.snapshot().threads.get(&thread).cloned();
        let chip = state.row(task).and_then(|r| r.live);
        let now = (view.as_ref().map(|v| v.state), chip);
        if Some(now) != last {
            println!(
                "{:>5.1}s  state {:?}  chip {:?}  title {:?}",
                watch.elapsed().as_secs_f32(),
                now.0,
                now.1,
                view.as_ref().and_then(|v| v.title.clone())
            );
            last = Some(now);
        }
        worked |= matches!(now.0, Some(airboss_services::ThreadState::Running));
        if worked && matches!(now.0, Some(airboss_services::ThreadState::Idle)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let needs = attention.for_task(task);
    println!("needs me for the task: {needs:?}");

    state.shutdown(Duration::from_secs(2));
}
