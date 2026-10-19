//! The MCP server: tool listing and dispatch over any rmcp transport.
//!
//! Results carry the payload twice, like Python's: as text that is
//! byte-identical to `json.dumps(payload, ensure_ascii=False)`, and as
//! `structuredContent` (objects as-is, arrays as `{"result": [...]}`, since
//! structured content must be an object). Failures are `isError` results
//! with `FastMCP`'s text, `Error executing tool <name>: <message>`, so the
//! model can read and correct them.

use std::sync::Arc;

use re_core::pyjson::{DumpOptions, dumps};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    InitializeResult, ListToolsResult, PaginatedRequestParams, ServerCapabilities, Tool,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::{Map, Value, json};

use crate::schema::{TOOLS, ToolSpec, input_schema};
use crate::tools::Tools;

/// Server name, as Python's `FastMCP("reasoning-engine")`.
pub const SERVER_NAME: &str = "reasoning-engine";
/// Instructions, copied from `transport.py`.
pub const INSTRUCTIONS: &str = "Actor-Critic-Planner-Reflexion reasoning backend for deep research";

/// `json.dumps(payload, ensure_ascii=False)`.
const RESPONSE_JSON: DumpOptions = DumpOptions {
    indent: None,
    sort_keys: false,
    item_separator: ", ",
    key_separator: ": ",
    ensure_ascii: false,
};

/// The reasoning-engine MCP server.
#[derive(Clone)]
pub struct ReasoningServer {
    tools: Arc<Tools>,
}

fn tool_definition(spec: &ToolSpec) -> Tool {
    let mut tool = Tool::new(spec.name, spec.description, Arc::new(input_schema(spec)));
    if spec.read_only {
        tool = tool.with_annotations(ToolAnnotations::new().read_only(true));
    }
    tool
}

/// A successful result for `payload`.
#[must_use]
pub fn success(payload: Value) -> CallToolResult {
    let text = dumps(&payload, &RESPONSE_JSON);
    let structured = if payload.is_object() {
        payload
    } else {
        json!({"result": payload})
    };
    let mut result = CallToolResult::success(vec![ContentBlock::text(text)]);
    result.structured_content = Some(structured);
    result
}

/// A failed result, worded as `FastMCP` words it.
#[must_use]
pub fn failure(tool: &str, message: &str) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(format!(
        "Error executing tool {tool}: {message}"
    ))])
}

impl ReasoningServer {
    /// A server running `tools`.
    #[must_use]
    pub fn new(tools: Tools) -> Self {
        Self {
            tools: Arc::new(tools),
        }
    }

    /// Every tool definition, in Python's registration order.
    #[must_use]
    pub fn tool_definitions() -> Vec<Tool> {
        TOOLS.iter().map(tool_definition).collect()
    }

    /// Call a tool and wrap the outcome as an MCP result.
    pub async fn call(&self, name: &str, arguments: &Map<String, Value>) -> CallToolResult {
        match self.tools.call(name, arguments).await {
            Ok(payload) => success(payload),
            Err(error) => failure(name, &error.0),
        }
    }

    /// Serve over stdin and stdout until the client disconnects.
    ///
    /// # Errors
    /// The handshake or the transport failed.
    pub async fn serve_stdio(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let running = self.serve(rmcp::transport::stdio()).await?;
        running.waiting().await?;
        Ok(())
    }
}

impl ServerHandler for ReasoningServer {
    fn get_info(&self) -> InitializeResult {
        let capabilities = ServerCapabilities::builder().enable_tools();
        // Prefetch tasks are pollable with `tasks/get` (tasks feature).
        #[cfg(feature = "tasks")]
        let capabilities = capabilities.enable_tasks();
        let mut info = InitializeResult::new(capabilities.build()).with_instructions(INSTRUCTIONS);
        info.server_info = Implementation::new(SERVER_NAME, env!("CARGO_PKG_VERSION"));
        info
    }

    #[allow(
        clippy::unused_async_trait_impl,
        reason = "matches the trait signature"
    )]
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(Self::tool_definitions()))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        crate::schema::find(name).map(tool_definition)
    }

    #[cfg(feature = "tasks")]
    #[allow(
        clippy::unused_async_trait_impl,
        reason = "matches the trait signature"
    )]
    async fn get_task(
        &self,
        request: rmcp::model::GetTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::GetTaskResult, ErrorData> {
        self.tools
            .prefetcher()
            .get(&request.task_id)
            .map(rmcp::model::GetTaskResult::new)
    }

    #[cfg(feature = "tasks")]
    #[allow(
        clippy::unused_async_trait_impl,
        reason = "matches the trait signature"
    )]
    async fn cancel_task(
        &self,
        request: rmcp::model::CancelTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        self.tools.prefetcher().cancel(&request.task_id)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let arguments = request.arguments.unwrap_or_default();
        Ok(self.call(&request.name, &arguments).await.into())
    }
}
