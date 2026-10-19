//! Speculative retrieval over the MCP tasks extension (`tasks` feature,
//! plan Phase 8).
//!
//! `plan_research_angles_tool(prefetch=true)` answers at once with the
//! angles, each carrying the `search_query` to use, and starts one task that
//! searches the bare query and every angle query concurrently. With the
//! evidence cache on, those results are waiting when Claude calls
//! `scholar_search_tool` with the same query. Clients that speak the tasks
//! extension can poll the task with `tasks/get` (progress in
//! `statusMessage`, per-query counts in the result) or cancel it; others
//! simply benefit from the warm cache.
//!
//! The plan returned a task handle (`resultType: "task"`) from the tool
//! itself. That would make the call fail for clients without the
//! extension, Claude Code included, so the handle is reported inside the
//! ordinary result instead.

use std::sync::Arc;

use re_core::planning::ResearchAngle;
use re_retrieval::AcademicSearchAggregator;
use rmcp::ErrorData;
use rmcp::model::{CallToolResult, ContentBlock, DetailedTask, Task};
use rmcp::task_manager::{TaskManager, TaskOptions};
use serde_json::{Value, json};
use tokio::task::JoinSet;

/// How long a finished prefetch task stays pollable.
pub const TASK_TTL_MS: u64 = 10 * 60 * 1000;
/// The `run_id` prefetch searches run under (records are rebound to the
/// real run when they are served from the cache).
pub const PREFETCH_RUN: &str = "prefetch";

/// `"{query} {angle name}"`: the query the skill searches each angle with.
#[must_use]
pub fn search_query(query: &str, angle: &ResearchAngle) -> String {
    format!("{} {}", query.trim(), angle.name)
}

/// Prefetch tasks for one server.
#[derive(Clone, Default)]
pub struct Prefetcher {
    manager: TaskManager,
}

impl std::fmt::Debug for Prefetcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Prefetcher")
    }
}

impl Prefetcher {
    /// Start searching `queries` (at most `limit` results each) in the
    /// background. Returns the task's seed state.
    #[must_use]
    pub fn start(
        &self,
        search: Arc<AcademicSearchAggregator>,
        queries: Vec<String>,
        limit: usize,
    ) -> Task {
        let total = queries.len();
        let options = TaskOptions::new()
            .with_ttl_ms(TASK_TTL_MS)
            .with_status_message(format!("prefetching 0/{total} queries"));
        self.manager.spawn(options, move |context| {
            Box::pin(async move {
                let mut jobs = JoinSet::new();
                for (index, query) in queries.into_iter().enumerate() {
                    let search = Arc::clone(&search);
                    jobs.spawn(async move {
                        let result = search.search(PREFETCH_RUN, &query, limit).await;
                        (index, query, result)
                    });
                }
                let mut done = Vec::with_capacity(total);
                while let Some(joined) = jobs.join_next().await {
                    if context.is_cancel_requested() {
                        jobs.abort_all();
                        return Err(rmcp::task_manager::TaskExit::Cancelled);
                    }
                    if let Ok((index, query, result)) = joined {
                        done.push((
                            index,
                            json!({
                                "query": query,
                                "evidence": result.evidence.len(),
                                "error": result.error.map(|e| e.error_type),
                            }),
                        ));
                    }
                    context
                        .set_status_message(format!("prefetched {}/{total} queries", done.len()));
                }
                done.sort_by_key(|(index, _)| *index);
                let summary =
                    json!({"queries": done.into_iter().map(|(_, v)| v).collect::<Vec<_>>()});
                let mut result =
                    CallToolResult::success(vec![ContentBlock::text(summary.to_string())]);
                result.structured_content = Some(summary);
                Ok(result)
            })
        })
    }

    /// `tasks/get`.
    ///
    /// # Errors
    /// An unknown task.
    pub fn get(&self, task_id: &str) -> Result<DetailedTask, ErrorData> {
        self.manager.get_task(task_id)
    }

    /// `tasks/cancel`.
    ///
    /// # Errors
    /// An unknown task.
    pub fn cancel(&self, task_id: &str) -> Result<(), ErrorData> {
        self.manager.cancel_task(task_id)
    }
}

/// The tool's answer: the angles with their `search_query`, and the task.
#[must_use]
pub fn response(query: &str, angles: &[ResearchAngle], task: &Task, queries: &[String]) -> Value {
    let angles: Vec<Value> = angles
        .iter()
        .map(|angle| {
            let mut value = serde_json::to_value(angle).unwrap_or(Value::Null);
            value["search_query"] = search_query(query, angle).into();
            value
        })
        .collect();
    json!({
        "angles": angles,
        "prefetch": {
            "task_id": task.task_id,
            "status": "working",
            "queries": queries,
            "note": "Search each angle with its search_query to use the prefetched evidence.",
        },
    })
}
