//! The real handlers through Tauri's `invoke`, on the mock runtime with the app's own config
//! and capability files: one read and one write, camelCase arguments, and a window refused a
//! command its capability file doesn't list.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use airboss_app::App;
use serde_json::{Value, json};
use tauri::Manager;
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;

fn request(cmd: &str, body: Value) -> InvokeRequest {
    InvokeRequest {
        cmd: cmd.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: "tauri://localhost".parse().unwrap(),
        body: InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    }
}

#[test]
fn commands_go_through_invoke_and_each_window_only_gets_its_own() {
    let test = common::TestApp::new();
    let id = test.add("Through invoke", None);
    test.sync_vm();
    let app = mock_builder()
        .manage(App::ready(test.state.clone()))
        .invoke_handler(airboss_app::ipc::handler())
        .build(tauri::generate_context!("tauri.conf.json", test = true))
        .unwrap();
    let window = |label: &str| {
        app.get_webview_window(label).unwrap_or_else(|| {
            tauri::WebviewWindowBuilder::new(&app, label, Default::default())
                .build()
                .unwrap()
        })
    };
    let main = window("main");
    let quickadd = window("quickadd");

    // A read.
    let boot: Value = get_ipc_response(&main, request("bootstrap", json!({})))
        .unwrap()
        .deserialize()
        .unwrap();
    assert_eq!(boot["rows"][0]["title"], "Through invoke");

    // A write, with Tauri's camelCase argument names.
    let detail: Value = get_ipc_response(
        &main,
        request(
            "task_detail",
            json!({"id": id.0, "req": 7, "historyLimit": 30}),
        ),
    )
    .unwrap()
    .deserialize()
    .unwrap();
    assert_eq!(detail["req"], 7);
    let w: Value = get_ipc_response(
        &main,
        request(
            "set_status",
            json!({"id": id.0, "status": "doing", "note": null}),
        ),
    )
    .unwrap()
    .deserialize()
    .unwrap();
    assert_eq!(w["patch"]["upserts"][0]["status"], "doing");
    assert!(w["undo"]["id"].is_number());

    // An app error comes back as the AppError shape.
    let err = get_ipc_response(
        &main,
        request("rename_task", json!({"id": 999, "title": "x"})),
    )
    .unwrap_err();
    assert_eq!(err["code"], "not_found");
    assert_eq!(err["message"], "Task 999 no longer exists.");

    // Quick add may create a task but not read the whole tree.
    let created: Value = get_ipc_response(
        &quickadd,
        request(
            "create_task",
            json!({"task": {"title": "From quick add", "body": null, "parent": null, "repos": [], "links": [], "then": "none"}}),
        ),
    )
    .unwrap()
    .deserialize()
    .unwrap();
    assert!(created["id"].is_number());
    assert!(get_ipc_response(&quickadd, request("bootstrap", json!({}))).is_err());
    // …and the main window may not resize the panel.
    assert!(get_ipc_response(&main, request("quickadd_resize", json!({"height": 200}))).is_err());
}
