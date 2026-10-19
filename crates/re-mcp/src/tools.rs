//! The twenty tool bodies (port of `server.py`). Each one validates in
//! Python's order, so the first error a client sees is the same one.
//!
//! Store calls are synchronous `SQLite` work behind a mutex; they are short
//! and run inline on the async worker, as Python's server runs them inline.

use std::sync::Arc;

use indexmap::IndexMap;
use re_core::directional::{BranchInfo, plan_directions};
use re_core::dora::select_branches;
use re_core::planning::{evidence_gap_questions, plan_research_angles};
use re_core::profiles::classify_research_mode;
use re_core::sanitize::sanitize_content;
use re_retrieval::base::py_round;
use re_verify::VerifiableResearchService;
use re_verify::gate::gate_payloads;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::args::Args;
use crate::error::ToolError;
use crate::limits::{
    MAX_ANGLES, MAX_CRITIQUE_CHARS, MAX_DRAFT_CHARS, MAX_LEARNING_CHARS, MAX_LEARNINGS,
    MAX_MEMORY_LIMIT, MAX_QUERY_CHARS, MAX_RAW_TEXT_CHARS, MAX_RUN_ID_CHARS, MAX_SEARCH_LIMIT,
    MAX_TAG_CHARS, MAX_TAGS, MAX_TOP_K, MAX_TRACE_STEP_CHARS, MAX_TRACE_STEPS,
};
use crate::validate;

type ToolResult = Result<Value, ToolError>;

/// A `usize` limit as the `i64` the range checks take.
fn cap(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

fn to_value<T: Serialize>(value: &T) -> Value {
    // Rows and records are plain data; serialisation cannot fail.
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// Executes tools against one research service.
pub struct Tools {
    service: Arc<VerifiableResearchService>,
    directional: bool,
}

impl Tools {
    /// Tools over `service` (sessions use its store too, as Python's server
    /// shares one database path). `REASONING_ENGINE_DORA=directional` adds
    /// per-direction advice to `select_next_branches`.
    #[must_use]
    pub fn new(service: Arc<VerifiableResearchService>) -> Self {
        let directional = std::env::var("REASONING_ENGINE_DORA").is_ok_and(|v| v == "directional");
        Self {
            service,
            directional,
        }
    }

    /// Turn directional DORA advice on or off, overriding the environment.
    #[must_use]
    pub fn with_directional(mut self, on: bool) -> Self {
        self.directional = on;
        self
    }

    fn store(&self) -> &re_store::Store {
        self.service.store()
    }

    /// Run tool `name` with `arguments`.
    ///
    /// # Errors
    /// Python's message for an unknown tool, a bad argument, or a failure.
    pub async fn call(&self, name: &str, arguments: &Map<String, Value>) -> ToolResult {
        let Some(spec) = crate::schema::find(name) else {
            return Err(ToolError::new(format!("Unknown tool: {name}")));
        };
        crate::args::check(spec, arguments)?;
        let a = Args::new(name, arguments);
        match name {
            "init_research_session" => self.init_research_session(&a),
            "register_branch" => self.register_branch(&a),
            "score_branch" => self.score_branch(&a),
            "select_next_branches" => self.select_next_branches(&a),
            "record_reflection_tool" => self.record_reflection(&a),
            "check_termination" => self.check_termination(&a),
            "get_session_state" => self.get_session_state(&a),
            "consensus_candidates" => self.consensus_candidates(&a),
            "save_to_memory" => self.save_to_memory(&a),
            "recall_memory_tool" => self.recall_memory(&a),
            "sanitize_content" => Self::sanitize(&a),
            "plan_research_angles_tool" => Self::plan_research_angles(&a),
            "evidence_gap_questions_tool" => Self::evidence_gap_questions(&a),
            "start_research_run" => self.start_research_run(&a),
            "classify_research_mode_tool" => Self::classify_research_mode(&a),
            "scholar_search_tool" => self.scholar_search(&a).await,
            "get_scholar_auth_status" => Ok(Self::scholar_auth_status()),
            "run_research_pipeline_tool" | "export_run_pack_tool" => {
                self.run_research_pipeline(&a).await
            }
            "run_quality_gate_tool" => self.run_quality_gate(&a),
            other => Err(ToolError::new(format!("Unknown tool: {other}"))),
        }
    }

    fn init_research_session(&self, a: &Args<'_>) -> ToolResult {
        let query = validate::text(&a.str("query")?, "query", MAX_QUERY_CHARS, false)?;
        let mut session = match to_value(&self.store().create_session(&query)?) {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        // `session["session_id"] = session.pop("id")` moves the key last.
        let id = session.shift_remove("id").unwrap_or(Value::Null);
        session.insert("session_id".into(), id);
        Ok(Value::Object(session))
    }

    fn register_branch(&self, a: &Args<'_>) -> ToolResult {
        let session_id = a.str("session_id")?;
        let trace = a.str_list_or_str("trace")?;
        let sources = a.dict_list_or_str_or("sources", "[]")?;
        let parent_id = a.str_or("parent_id", "")?;
        let depth = a.int_or("depth", 0)?;

        let session = self.store().get_session(&session_id)?;
        let depth = validate::non_negative_int(depth, "depth", session.budget_max_depth)?;
        let trace = validate::json_array(trace, "trace")?;
        let trace = validate::string_list(&trace, "trace", MAX_TRACE_STEPS, MAX_TRACE_STEP_CHARS)?;
        let sources = validate::sources(&validate::json_array(sources, "sources")?)?;
        let parent = (!parent_id.is_empty()).then_some(parent_id.as_str());
        let branch = self
            .store()
            .register_branch(&session_id, &trace, &sources, parent, depth)?;
        Ok(json!({"branch_id": branch.id, "session_id": session_id}))
    }

    fn score_branch(&self, a: &Args<'_>) -> ToolResult {
        let session_id = a.str("session_id")?;
        let branch_id = a.str("branch_id")?;
        let q_score = a.float_req("q_score")?;
        let advantage = a.float_req("advantage")?;
        let critique = a.str("critique")?;
        let confidence = a.float_req("confidence")?;

        self.store()
            .get_branch_for_session(&branch_id, &session_id)?;
        let q_score = validate::score(q_score, "q_score")?;
        let advantage = validate::score(advantage, "advantage")?;
        let confidence = validate::score(confidence, "confidence")?;
        let critique = validate::text(&critique, "critique", MAX_CRITIQUE_CHARS, true)?;
        self.store().score_branch(
            &branch_id,
            q_score,
            advantage,
            &critique,
            confidence,
            Some(&session_id),
        )?;
        Ok(json!({"status": "scored", "branch_id": branch_id}))
    }

    fn select_next_branches(&self, a: &Args<'_>) -> ToolResult {
        let session_id = a.str("session_id")?;
        let session = self.store().get_session(&session_id)?;
        let scores: IndexMap<String, f64> = self
            .store()
            .get_active_branches(&session_id)?
            .into_iter()
            .map(|b| (b.id, b.q_score))
            .collect();
        let result = select_branches(&scores, session.budget_remaining_steps);
        self.store().apply_planning_result(
            &session_id,
            &result.branches_to_reflect,
            &result.branches_to_prune,
            result.budget_remaining,
        )?;
        let mut payload = json!({
            "branches_to_continue": result.branches_to_continue,
            "branches_to_reflect": result.branches_to_reflect,
            "branches_to_prune": result.branches_to_prune,
            "allocation": result.allocation,
            "kappa": py_round(result.kappa, 4),
            "budget_remaining": result.budget_remaining,
        });
        // Plan 6.3: the same fields plus optional per-direction advice.
        if self.directional {
            let branches: Vec<BranchInfo> = self
                .store()
                .get_session_branches(&session_id)?
                .into_iter()
                .map(|b| BranchInfo {
                    id: b.id,
                    parent_id: b.parent_id,
                    q_score: b.q_score,
                    visits: b.visits,
                    status: b.status,
                })
                .collect();
            let directions =
                plan_directions(&branches, result.budget_remaining, session.difficulty);
            payload["directions"] = to_value(&directions);
        }
        Ok(payload)
    }

    fn record_reflection(&self, a: &Args<'_>) -> ToolResult {
        let session_id = a.str("session_id")?;
        let branch_id = a.str("branch_id")?;
        let original = a.str("original_critique")?;
        let summary = a.str("revision_summary")?;
        let before = a.float_req("score_before")?;
        let after = a.float_req("score_after")?;

        self.store()
            .get_branch_for_session(&branch_id, &session_id)?;
        let original = validate::text(&original, "original_critique", MAX_CRITIQUE_CHARS, true)?;
        let summary = validate::text(&summary, "revision_summary", MAX_CRITIQUE_CHARS, false)?;
        let before = validate::score(before, "score_before")?;
        let after = validate::score(after, "score_after")?;
        let result = self.store().record_reflection(
            &branch_id,
            &session_id,
            &original,
            &summary,
            before,
            after,
        )?;
        Ok(to_value(&result))
    }

    fn check_termination(&self, a: &Args<'_>) -> ToolResult {
        let session_id = a.str("session_id")?;
        let session = self.store().get_session(&session_id)?;
        let result = self
            .store()
            .check_termination(&session_id, session.budget_remaining_steps)?;
        Ok(to_value(&result))
    }

    fn get_session_state(&self, a: &Args<'_>) -> ToolResult {
        let session_id = a.str("session_id")?;
        let session = self.store().get_session(&session_id)?;
        let branches = self.store().get_session_branches(&session_id)?;
        Ok(json!({"session": session, "branches": branches}))
    }

    fn consensus_candidates(&self, a: &Args<'_>) -> ToolResult {
        let session_id = a.str("session_id")?;
        let top_k = validate::limited_int(a.int_or("top_k", 3)?, "top_k", 1, cap(MAX_TOP_K))?;
        Ok(to_value(
            &self.store().get_consensus_candidates(&session_id, top_k)?,
        ))
    }

    fn save_to_memory(&self, a: &Args<'_>) -> ToolResult {
        let session_id = a.str("session_id")?;
        let query = a.str("query")?;
        let learnings = a.str_list_or_str("key_learnings")?;
        let tags = a.str_list_or_str("domain_tags")?;

        self.store().get_session(&session_id)?;
        let query = validate::text(&query, "query", MAX_QUERY_CHARS, false)?;
        let learnings = validate::json_array(learnings, "key_learnings")?;
        let learnings = validate::string_list(
            &learnings,
            "key_learnings",
            MAX_LEARNINGS,
            MAX_LEARNING_CHARS,
        )?;
        let tags = validate::json_array(tags, "domain_tags")?;
        let tags = validate::string_list(&tags, "domain_tags", MAX_TAGS, MAX_TAG_CHARS)?;
        Ok(to_value(&self.store().save_memory(
            &session_id,
            &query,
            &learnings,
            &tags,
        )?))
    }

    fn recall_memory(&self, a: &Args<'_>) -> ToolResult {
        let query = a.str("query")?;
        let limit = a.int_or("limit", 5)?;
        let query = validate::text(&query, "query", MAX_QUERY_CHARS, false)?;
        let limit = validate::limited_int(limit, "limit", 1, cap(MAX_MEMORY_LIMIT))?;
        let results = self
            .store()
            .recall_memory(&query, usize::try_from(limit).unwrap_or(1))?;
        let results = results
            .iter()
            .map(|r| {
                let mut v = to_value(r);
                if let Some(map) = v.as_object_mut() {
                    map.shift_remove("_relevance");
                }
                v
            })
            .collect();
        Ok(Value::Array(results))
    }

    fn sanitize(a: &Args<'_>) -> ToolResult {
        let raw = validate::text(&a.str("raw_text")?, "raw_text", MAX_RAW_TEXT_CHARS, true)?;
        Ok(json!({"cleaned": sanitize_content(&raw)}))
    }

    fn plan_research_angles(a: &Args<'_>) -> ToolResult {
        let query = a.str("query")?;
        let max_angles = a.int_or("max_angles", 6)?;
        let query = validate::text(&query, "query", MAX_QUERY_CHARS, false)?;
        let max_angles = validate::limited_int(max_angles, "max_angles", 1, cap(MAX_ANGLES))?;
        Ok(to_value(&plan_research_angles(
            &query,
            usize::try_from(max_angles).unwrap_or(1),
        )?))
    }

    fn evidence_gap_questions(a: &Args<'_>) -> ToolResult {
        let query = a.str("query")?;
        let claims = a.str_list_or_str("claims")?;
        let query = validate::text(&query, "query", MAX_QUERY_CHARS, false)?;
        let claims = validate::json_array(claims, "claims")?;
        let claims =
            validate::string_list(&claims, "claims", MAX_TRACE_STEPS, MAX_TRACE_STEP_CHARS)?;
        Ok(to_value(&evidence_gap_questions(&query, &claims)?))
    }

    fn start_research_run(&self, a: &Args<'_>) -> ToolResult {
        let query = a.str("query")?;
        let mode = a.str_or("mode", "standard")?;
        let profile = a.str_or("profile", "auto")?;
        let query = validate::text(&query, "query", MAX_QUERY_CHARS, false)?;
        Ok(to_value(&self.service.start_run(&query, &mode, &profile)?))
    }

    fn classify_research_mode(a: &Args<'_>) -> ToolResult {
        let query = a.str("query")?;
        let requested = a.str_or("requested_mode", "standard")?;
        let query = validate::text(&query, "query", MAX_QUERY_CHARS, false)?;
        Ok(json!({"mode": classify_research_mode(&query, &requested)?.as_str()}))
    }

    async fn scholar_search(&self, a: &Args<'_>) -> ToolResult {
        let run_id = a.str("run_id")?;
        let query = a.str("query")?;
        let limit = a.int_or("limit", 10)?;
        let run_id = validate::text(&run_id, "run_id", MAX_RUN_ID_CHARS, false)?;
        let query = validate::text(&query, "query", MAX_QUERY_CHARS, false)?;
        let limit = validate::limited_int(limit, "limit", 1, MAX_SEARCH_LIMIT)?;
        let limit = usize::try_from(limit).unwrap_or(1);
        Ok(self
            .service
            .scholar_search(&run_id, &query, limit, false)
            .await?
            .to_json())
    }

    /// Whether live Scholar Gateway is on and a token is present; never the
    /// token itself.
    fn scholar_auth_status() -> Value {
        let env = |var: &str| std::env::var(var).ok();
        json!({
            "live_enabled": env("SCHOLAR_GATEWAY_LIVE").as_deref() == Some("1"),
            "has_env_token": env("SCHOLAR_GATEWAY_ACCESS_TOKEN").is_some_and(|t| !t.is_empty()),
            "token_storage": "environment_or_keyring",
        })
    }

    async fn run_research_pipeline(&self, a: &Args<'_>) -> ToolResult {
        let query = a.str("query")?;
        let draft = a.str("draft")?;
        let mode = a.str_or("mode", "standard")?;
        let profile = a.str_or("profile", "auto")?;
        let query = validate::text(&query, "query", MAX_QUERY_CHARS, false)?;
        let draft = validate::text(&draft, "draft", MAX_DRAFT_CHARS, false)?;
        Ok(self
            .service
            .run_research_pipeline(&query, &draft, &mode, &profile, false)
            .await?)
    }

    fn run_quality_gate(&self, a: &Args<'_>) -> ToolResult {
        let run_id = validate::text(&a.str("run_id")?, "run_id", MAX_RUN_ID_CHARS, false)?;
        let claims = self.store().list_claims(&run_id)?;
        let verifications = self.store().list_verifications(&run_id)?;
        let gaps = self.store().list_gaps(&run_id)?;
        Ok(gate_payloads(&run_id, &claims, &verifications, gaps.len()))
    }
}
