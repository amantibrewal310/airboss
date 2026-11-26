//! The MCP server: the six tools, the instructions sent at `initialize`, and turning each
//! tool's result into MCP content. Nothing but MCP messages goes to stdout.

use std::sync::Mutex;

use airboss_core::Store;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use serde::Serialize;

use crate::identity::Identity;
use crate::render::ToolError;
use crate::tools;

const LIST_TASKS: &str = "List tasks in tree order. Start here: `mine: true` finds the tasks \
linked to this thread; otherwise filter by `repos`, `status`, `ready` or `query`. Done and \
dropped tasks are left out unless `status` names them. Returns at most 200 (`truncated` says \
there were more). Next: get_task on the task you'll work on.";

const GET_TASK: &str = "Read one task before working on it: notes, repos, branch, links, the \
last 20 history entries, what it waits on and what waits on it, subtasks and their run order. \
Next: link this thread to it, then update_task as you go.";

const UPDATE_TASK: &str = "Report progress on a task: set `status`, add a `note`, or both. \
Set doing when you start, in_review when your PR is open, blocked with a `note` saying why. \
Set done with a `note` saying what you did, but only on a task with no PR: a task with a PR \
finishes by itself when the PR merges. A note alone records progress.";

const LINK: &str = "Attach something to a task. With kind t3_thread, herdr_terminal or \
claude_session and no `ref`, it links this thread: do that when you start on a task, with the \
kind the server's instructions name. Link every PR you open (kind \
pr, ref owner/repo#42, plus `url`). Linking the same thing twice returns the existing link.";

const PROPOSE_TASKS: &str = "Propose 1 to 20 new tasks you spotted, at top level or under \
`parent`. They arrive as proposed and wait for the user to accept or reject them, so don't \
start on one until it's accepted.";

const IMPORT_PLAN: &str = "Import a plan the user settled in plan-review as a tree of tasks, \
under `parent` or at top level. Only for a settled plan: `plan_file` must be that plan's file, \
and it gets linked to the top-level tasks. Give each step a stable `key`; sending the plan \
again adds only steps with new keys.";

/// One agent thread's server. The store is locked only for the length of one `core` call.
pub struct AirbossServer {
    store: Mutex<Store>,
    who: Identity,
    tool_router: ToolRouter<AirbossServer>,
}

#[tool_router]
impl AirbossServer {
    pub fn new(store: Store, who: Identity) -> AirbossServer {
        AirbossServer {
            store: Mutex::new(store),
            who,
            tool_router: AirbossServer::tool_router(),
        }
    }

    #[tool(description = LIST_TASKS, annotations(title = "List tasks", read_only_hint = true, open_world_hint = false))]
    async fn list_tasks(&self, Parameters(args): Parameters<tools::ListTasks>) -> CallToolResult {
        self.run(|store, who| tools::list_tasks(store, who, args))
    }

    #[tool(description = GET_TASK, annotations(title = "Get a task", read_only_hint = true, open_world_hint = false))]
    async fn get_task(&self, Parameters(args): Parameters<tools::GetTask>) -> CallToolResult {
        self.run(|store, _| tools::get_task(store, args))
    }

    #[tool(description = UPDATE_TASK, annotations(title = "Update a task", destructive_hint = false, open_world_hint = false))]
    async fn update_task(&self, Parameters(args): Parameters<tools::UpdateTask>) -> CallToolResult {
        self.run(|store, who| tools::update_task(store, who, args))
    }

    #[tool(description = LINK, annotations(title = "Link to a task", destructive_hint = false, idempotent_hint = true, open_world_hint = false))]
    async fn link(&self, Parameters(args): Parameters<tools::Link>) -> CallToolResult {
        self.run(|store, who| tools::link(store, who, args))
    }

    #[tool(description = PROPOSE_TASKS, annotations(title = "Propose tasks", destructive_hint = false, open_world_hint = false))]
    async fn propose_tasks(
        &self,
        Parameters(args): Parameters<tools::ProposeTasks>,
    ) -> CallToolResult {
        self.run(|store, who| tools::propose_tasks(store, who, args))
    }

    #[tool(description = IMPORT_PLAN, annotations(title = "Import a settled plan", destructive_hint = false, idempotent_hint = true, open_world_hint = false))]
    async fn import_plan(&self, Parameters(args): Parameters<tools::ImportPlan>) -> CallToolResult {
        self.run(|store, who| tools::import_plan(store, who, args))
    }
}

impl AirbossServer {
    /// Runs one tool against the store and turns its result into compact JSON text, or
    /// its refusal into a tool error the agent reads.
    fn run<T: Serialize>(
        &self,
        tool: impl FnOnce(&mut Store, &Identity) -> Result<T, ToolError>,
    ) -> CallToolResult {
        // A panic in another call can't leave the store half-written (core rolls back),
        // so a poisoned lock is safe to keep using.
        let mut store = self.store.lock().unwrap_or_else(|p| p.into_inner());
        let out = tool(&mut store, &self.who).and_then(|value| {
            serde_json::to_string(&value)
                .map_err(|e| ToolError::new(format!("Couldn't encode the result: {e}")))
        });
        match out {
            Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
            Err(err) => CallToolResult::error(vec![ContentBlock::text(err.0)]),
        }
    }

    /// The paragraph sent at `initialize`: how to use the tools, and who this process is.
    pub fn instructions(&self) -> String {
        // Name the one kind that is this thread, so the agent's link is the one Do it made.
        let kind = match self.who.own_kind() {
            Some(kind) => format!("kind {kind}"),
            None => "kind t3_thread, herdr_terminal or claude_session".to_string(),
        };
        let mut text = format!(
            "Airboss is the user's to-do list, and these tools report on the tasks you work \
             on. You act as {}. When you start, find your task with list_tasks (`mine: true`, \
             or your repos), or use the task id in your brief, and read it with get_task. Link \
             this thread to it (link, {kind}, no ref). Report status \
             changes with update_task as they happen, and link every PR you open. Propose new \
             work with propose_tasks; don't create it. Use import_plan only for a plan the user \
             settled in plan-review.",
            self.who.actor
        );
        if let Some(warning) = &self.who.warning {
            text.push(' ');
            text.push_str(warning);
        }
        text
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for AirbossServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("airboss", env!("CARGO_PKG_VERSION")))
            .with_instructions(self.instructions())
    }
}

/// Serves the tools over stdin and stdout until the client closes stdin.
pub fn serve(store: Store, who: Identity) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("couldn't start the MCP runtime: {e}"))?;
    runtime.block_on(async move {
        let running = AirbossServer::new(store, who)
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|e| format!("the MCP handshake failed: {e}"))?;
        running
            .waiting()
            .await
            .map_err(|e| format!("the MCP server stopped: {e}"))?;
        Ok(())
    })
}
