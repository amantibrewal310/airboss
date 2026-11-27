//! The real binary over stdin and stdout: initialize, tools/list, tools/call.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use airboss_core::{Actor, NewLink, NewTask, Store, TaskId};
use common::{Mcp, NEWER_DATABASE, TempDir, as_if_a_newer_airboss_migrated};
use serde_json::json;

const THREAD: &str = "thread-under-test";

fn seed(db: &std::path::Path, titles: &[&str]) -> Vec<TaskId> {
    let mut s = Store::open(db).unwrap();
    titles
        .iter()
        .map(|t| s.add_task(NewTask::new(*t), &Actor::Me).unwrap().id)
        .collect()
}

#[test]
fn initialize_names_the_server_and_the_actor() {
    let dir = TempDir::new();
    let mut mcp = Mcp::start(&dir.db(), Some(THREAD));
    let info = mcp.initialize();
    assert_eq!(info["serverInfo"]["name"], "airboss");
    assert!(info["capabilities"]["tools"].is_object());
    let instructions = info["instructions"].as_str().unwrap();
    assert!(
        instructions.contains(&format!("agent:t3/{THREAD}")),
        "{instructions}"
    );
    assert!(instructions.contains("list_tasks"), "{instructions}");
    assert!(
        instructions.contains("Don't widen your own task"),
        "{instructions}"
    );
    assert!(mcp.finish());
}

#[test]
fn initialize_warns_when_the_thread_is_unknown() {
    let dir = TempDir::new();
    let mut mcp = Mcp::start(&dir.db(), None);
    let info = mcp.initialize();
    let instructions = info["instructions"].as_str().unwrap();
    assert!(instructions.contains("agent:unknown/"), "{instructions}");
    assert!(instructions.contains("AIRBOSS_THREAD"), "{instructions}");
}

#[test]
fn tools_list_has_exactly_the_six_tools_each_with_an_input_schema() {
    let dir = TempDir::new();
    let mut mcp = Mcp::start(&dir.db(), Some(THREAD));
    mcp.initialize();
    let resp = mcp.request("tools/list", json!({}));
    let tools = resp["result"]["tools"].as_array().unwrap();
    let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "get_task",
            "import_plan",
            "link",
            "list_tasks",
            "propose_tasks",
            "update_task"
        ]
    );
    for tool in tools {
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], "object", "{tool}");
        assert!(schema["properties"].is_object(), "{tool}");
        assert!(!tool["description"].as_str().unwrap().is_empty(), "{tool}");
    }
    let get = tools.iter().find(|t| t["name"] == "get_task").unwrap();
    assert_eq!(get["inputSchema"]["required"], json!(["id"]));
    assert_eq!(get["annotations"]["readOnlyHint"], true);
}

#[test]
fn an_agent_finds_links_and_updates_its_task() {
    let dir = TempDir::new();
    let [work, _other] = seed(&dir.db(), &["Fix booking typo", "Other"])[..] else {
        unreachable!()
    };
    let mut mcp = Mcp::start(&dir.db(), Some(THREAD));
    mcp.initialize();

    let listed = mcp.ok("list_tasks", json!({"query": "booking"}));
    assert_eq!(listed["tasks"][0]["id"], work.0);
    assert_eq!(listed["truncated"], false);

    let link = mcp.ok("link", json!({"id": work.0, "kind": "t3_thread"}));
    assert_eq!(link["ref"], THREAD);
    assert_eq!(link["linked_by"], format!("agent:t3/{THREAD}"));

    let mine = mcp.ok("list_tasks", json!({"mine": true}));
    assert_eq!(mine["tasks"].as_array().unwrap().len(), 1);

    let task = mcp.ok("update_task", json!({"id": work.0, "status": "doing"}));
    assert_eq!(task["status"], "doing");

    let detail = mcp.ok("get_task", json!({"id": work.0}));
    assert_eq!(detail["task"]["status"], "doing");
    assert_eq!(detail["links"][0]["kind"], "t3_thread");
    assert!(mcp.finish());

    // The write landed in the file, as this thread.
    let s = Store::open(&dir.db()).unwrap();
    let last = s.history(work).unwrap().pop().unwrap();
    assert_eq!(last.actor, Actor::Agent(format!("t3/{THREAD}")));
}

#[test]
fn proposals_and_plans_work_over_stdio() {
    let dir = TempDir::new();
    let plan_file = dir.0.join("plan.md");
    std::fs::write(&plan_file, "# plan").unwrap();
    let mut mcp = Mcp::start(&dir.db(), Some(THREAD));
    mcp.initialize();

    let proposed = mcp.ok("propose_tasks", json!({"tasks": [{"title": "Add tests"}]}));
    assert_eq!(proposed[0]["status"], "proposed");

    let plan = json!({
        "plan_file": plan_file,
        "plan": [{"key": "p1", "title": "Phase 1", "children": [{"key": "a", "title": "Step"}]},
                 {"key": "p2", "title": "Phase 2", "depends_on": ["p1"]}]
    });
    let imported = mcp.ok("import_plan", plan.clone());
    assert_eq!(imported["created"].as_array().unwrap().len(), 3);
    let again = mcp.ok("import_plan", plan);
    assert_eq!(again["created"], json!([]));
    assert_eq!(again["existing"].as_array().unwrap().len(), 3);
}

#[test]
fn a_refused_call_is_a_tool_error_and_the_server_keeps_going() {
    let dir = TempDir::new();
    let id = seed(&dir.db(), &["Code"])[0];
    {
        let mut s = Store::open(&dir.db()).unwrap();
        s.link(id, NewLink::pr("acme/web#1"), &Actor::Me).unwrap();
    }
    let mut mcp = Mcp::start(&dir.db(), Some(THREAD));
    mcp.initialize();
    let (is_error, text) = mcp.call(
        "update_task",
        json!({"id": id.0, "status": "done", "note": "did it"}),
    );
    assert!(is_error);
    assert!(
        text.starts_with("Agents can't finish a task that has a PR"),
        "{text}"
    );

    let (is_error, text) = mcp.call("update_task", json!({"id": id.0, "status": "in_review"}));
    assert!(!is_error, "{text}");
    assert!(mcp.finish());
}

#[test]
fn bad_arguments_are_a_tool_error_too() {
    let dir = TempDir::new();
    let mut mcp = Mcp::start(&dir.db(), Some(THREAD));
    mcp.initialize();
    let (is_error, text) = mcp.call("update_task", json!({"id": 1, "status": "finished"}));
    assert!(is_error);
    assert!(text.contains("unknown variant `finished`"), "{text}");
    let (is_error, text) = mcp.call("get_task", json!({"task": 1}));
    assert!(is_error);
    assert!(text.contains("unknown field `task`"), "{text}");
}

#[test]
fn an_unknown_tool_is_a_protocol_error() {
    let dir = TempDir::new();
    let mut mcp = Mcp::start(&dir.db(), Some(THREAD));
    mcp.initialize();
    let resp = mcp.request(
        "tools/call",
        json!({"name": "delete_task", "arguments": {}}),
    );
    assert_eq!(resp["error"]["code"], -32602);
}

#[test]
fn it_creates_the_database_and_its_directory_on_first_use() {
    let dir = TempDir::new();
    let db = dir.0.join("nested").join("deeper").join("airboss.db");
    let mut mcp = Mcp::start(&db, Some(THREAD));
    mcp.initialize();
    let listed = mcp.ok("list_tasks", json!({}));
    assert_eq!(listed["total"], 0);
    assert!(db.is_file());
}

#[test]
fn a_database_it_cannot_open_is_one_line_on_stderr_and_exit_code_1() {
    let dir = TempDir::new();
    let blocker = dir.0.join("file");
    std::fs::write(&blocker, "not a dir").unwrap();
    let out = common::airboss(&blocker.join("airboss.db"))
        .arg("mcp")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    let err = common::stderr(&out);
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(err.starts_with("airboss: "), "{err}");
}

#[test]
fn two_servers_and_the_cli_write_at_once_without_errors() {
    let dir = TempDir::new();
    let id = seed(&dir.db(), &["Shared"])[0];
    let mut writers: Vec<_> = ["one", "two"]
        .into_iter()
        .map(|thread| {
            let db = dir.db();
            std::thread::spawn(move || {
                let mut mcp = Mcp::start(&db, Some(thread));
                mcp.initialize();
                for i in 0..40 {
                    let (is_error, text) = mcp.call(
                        "update_task",
                        json!({"id": id.0, "note": format!("{thread} {i}")}),
                    );
                    assert!(!is_error, "{thread} {i}: {text}");
                }
                assert!(mcp.finish());
            })
        })
        .collect();
    let db = dir.db();
    writers.push(std::thread::spawn(move || {
        for i in 0..15 {
            let out = common::run(
                &db,
                &["add", &format!("cli {i}"), "--under", &id.0.to_string()],
            );
            assert!(out.status.success(), "cli {i}: {}", common::stderr(&out));
        }
    }));
    for w in writers {
        w.join().unwrap();
    }
    let s = Store::open(&dir.db()).unwrap();
    assert_eq!(s.history(id).unwrap().len(), 1 + 80);
    assert_eq!(s.tree().unwrap().len(), 1 + 15);
}

#[test]
fn a_server_running_when_a_newer_airboss_migrates_refuses_writes_plainly() {
    let dir = TempDir::new();
    let id = seed(&dir.db(), &["Code"])[0];
    let mut mcp = Mcp::start(&dir.db(), Some(THREAD));
    mcp.initialize();
    mcp.ok("get_task", json!({"id": id.0}));
    as_if_a_newer_airboss_migrated(&dir.db());
    let (is_error, text) = mcp.call("update_task", json!({"id": id.0, "status": "doing"}));
    assert!(is_error);
    assert_eq!(text, NEWER_DATABASE);
    assert!(mcp.finish());

    // A new server on that file doesn't start.
    let out = common::airboss(&dir.db()).arg("mcp").output().unwrap();
    assert!(!out.status.success());
    assert_eq!(
        common::stderr(&out).trim_end(),
        format!("airboss: {NEWER_DATABASE}")
    );
}
