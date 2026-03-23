"""Session and branch management for the reasoning engine."""

import json
import uuid

from reasoning_engine.db import get_connection
from reasoning_engine.difficulty import estimate_difficulty
from reasoning_engine.dora import allocate_budget

TERMINATION_Q_THRESHOLD = 0.85
TERMINATION_CONFIDENCE_THRESHOLD = 0.80


def create_session(db_path: str, query: str) -> dict:
    session_id = str(uuid.uuid4())
    difficulty = estimate_difficulty(query)
    budget = allocate_budget(difficulty)

    with get_connection(db_path) as conn:
        conn.execute(
            """INSERT INTO sessions
               (id, query, difficulty, strategy,
                budget_total_branches, budget_max_depth, budget_max_steps,
                budget_tokens_per_branch, budget_remaining_steps, status)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'active')""",
            (
                session_id,
                query,
                difficulty,
                budget.strategy,
                budget.total_branches,
                budget.max_depth,
                budget.max_steps,
                budget.tokens_per_branch,
                budget.max_steps,
            ),
        )

    return {
        "id": session_id,
        "query": query,
        "difficulty": difficulty,
        "strategy": budget.strategy,
        "budget": {
            "total_branches": budget.total_branches,
            "max_depth": budget.max_depth,
            "max_steps": budget.max_steps,
            "tokens_per_branch": budget.tokens_per_branch,
            "remaining_steps": budget.max_steps,
        },
        "status": "active",
    }


def get_session(db_path: str, session_id: str) -> dict:
    with get_connection(db_path) as conn:
        row = conn.execute("SELECT * FROM sessions WHERE id = ?", (session_id,)).fetchone()
    if not row:
        raise ValueError(f"Session {session_id} not found")
    return dict(row)


def register_branch(
    db_path: str,
    session_id: str,
    trace: list[str],
    sources: list[dict] | None = None,
    parent_id: str | None = None,
    depth: int = 0,
) -> dict:
    branch_id = str(uuid.uuid4())
    with get_connection(db_path) as conn:
        conn.execute(
            """INSERT INTO branches (id, session_id, parent_id, depth, trace, status)
               VALUES (?, ?, ?, ?, ?, 'active')""",
            (branch_id, session_id, parent_id, depth, json.dumps(trace)),
        )
        for source in sources or []:
            conn.execute(
                """INSERT INTO sources (id, branch_id, url, title, excerpt, relevance_score)
                   VALUES (?, ?, ?, ?, ?, ?)""",
                (
                    str(uuid.uuid4()),
                    branch_id,
                    source.get("url", ""),
                    source.get("title", ""),
                    source.get("excerpt", ""),
                    source.get("relevance_score", 0.0),
                ),
            )
    return {"id": branch_id, "session_id": session_id, "parent_id": parent_id, "depth": depth}


def get_branch(db_path: str, branch_id: str) -> dict:
    with get_connection(db_path) as conn:
        row = conn.execute("SELECT * FROM branches WHERE id = ?", (branch_id,)).fetchone()
    if not row:
        raise ValueError(f"Branch {branch_id} not found")
    return dict(row)


def score_branch(
    db_path: str, branch_id: str, q_score: float, advantage: float, critique: str, confidence: float
) -> None:
    with get_connection(db_path) as conn:
        conn.execute(
            """UPDATE branches SET q_score=?, advantage=?, critique=?, confidence=?, visits=visits+1 WHERE id=?""",
            (q_score, advantage, critique, confidence, branch_id),
        )


def update_branch_status(db_path: str, branch_id: str, status: str) -> None:
    with get_connection(db_path) as conn:
        conn.execute("UPDATE branches SET status=? WHERE id=?", (status, branch_id))


def get_active_branches(db_path: str, session_id: str) -> list[dict]:
    with get_connection(db_path) as conn:
        rows = conn.execute(
            "SELECT * FROM branches WHERE session_id=? AND status='active'", (session_id,)
        ).fetchall()
    return [dict(r) for r in rows]


def check_termination(db_path: str, session_id: str, budget_remaining: int) -> dict:
    if budget_remaining <= 0:
        return {"should_terminate": True, "reason": "Budget exhausted"}
    with get_connection(db_path) as conn:
        rows = conn.execute(
            "SELECT q_score, confidence FROM branches WHERE session_id=? AND status='active' ORDER BY q_score DESC LIMIT 1",
            (session_id,),
        ).fetchall()
    if rows:
        top = dict(rows[0])
        if (
            top["q_score"] > TERMINATION_Q_THRESHOLD
            and top["confidence"] > TERMINATION_CONFIDENCE_THRESHOLD
        ):
            return {
                "should_terminate": True,
                "reason": f"High confidence result (q={top['q_score']:.2f}, conf={top['confidence']:.2f})",
            }
    return {"should_terminate": False, "reason": "Continue"}


def get_consensus_candidates(db_path: str, session_id: str, top_k: int = 3) -> list[dict]:
    with get_connection(db_path) as conn:
        rows = conn.execute(
            "SELECT * FROM branches WHERE session_id=? AND status IN ('active','completed') ORDER BY q_score DESC LIMIT ?",
            (session_id, top_k),
        ).fetchall()
    return [dict(r) for r in rows]
