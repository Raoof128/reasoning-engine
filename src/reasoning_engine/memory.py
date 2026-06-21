"""Episodic memory for Reflexion learnings and cross-session knowledge."""

import json
import uuid

from reasoning_engine.db import get_connection
from reasoning_engine.sanitizer import sanitize_content


def save_memory(
    db_path: str, session_id: str, query: str, key_learnings: list[str], domain_tags: list[str]
) -> dict:
    memory_id = str(uuid.uuid4())
    with get_connection(db_path) as conn:
        session = conn.execute("SELECT id FROM sessions WHERE id = ?", (session_id,)).fetchone()
        if not session:
            raise ValueError(f"Session {session_id} not found")
        conn.execute(
            """INSERT INTO episodic_memory (id, session_id, query, key_learnings, domain_tags)
               VALUES (?, ?, ?, ?, ?)""",
            (
                memory_id,
                session_id,
                sanitize_content(query),
                json.dumps([sanitize_content(learning) for learning in key_learnings]),
                json.dumps([sanitize_content(tag) for tag in domain_tags]),
            ),
        )
    return {"id": memory_id}


def recall_memory(db_path: str, query: str, limit: int = 5) -> list[dict]:
    query_words = set(query.lower().split())
    with get_connection(db_path) as conn:
        rows = conn.execute("SELECT * FROM episodic_memory").fetchall()

    scored = []
    for row in rows:
        row_dict = dict(row)
        stored_query_words = set(row_dict["query"].lower().split())
        stored_tags = json.loads(row_dict["domain_tags"])
        tag_words = set()
        for tag in stored_tags:
            tag_words.add(tag.lower())
            for part in tag.lower().split("-"):
                tag_words.add(part)
        all_stored_words = stored_query_words | tag_words
        overlap = len(query_words & all_stored_words)
        if overlap > 0:
            row_dict["key_learnings"] = json.loads(row_dict["key_learnings"])
            row_dict["domain_tags"] = json.loads(row_dict["domain_tags"])
            row_dict["_relevance"] = overlap
            scored.append(row_dict)

    scored.sort(key=lambda x: x["_relevance"], reverse=True)
    return scored[:limit]


def record_reflection(
    db_path: str,
    branch_id: str,
    session_id: str,
    original_critique: str,
    revision_summary: str,
    score_before: float,
    score_after: float,
) -> dict:
    reflection_id = str(uuid.uuid4())
    with get_connection(db_path) as conn:
        branch = conn.execute(
            "SELECT id FROM branches WHERE id = ? AND session_id = ?",
            (branch_id, session_id),
        ).fetchone()
        if not branch:
            raise ValueError(f"Branch {branch_id} not found in session {session_id}")
        conn.execute(
            """INSERT INTO reflections (id, branch_id, session_id, original_critique, revision_summary, score_before, score_after)
               VALUES (?, ?, ?, ?, ?, ?, ?)""",
            (
                reflection_id,
                branch_id,
                session_id,
                sanitize_content(original_critique),
                sanitize_content(revision_summary),
                score_before,
                score_after,
            ),
        )
        conn.execute(
            "UPDATE branches SET status='active' WHERE id=? AND session_id=?",
            (branch_id, session_id),
        )
    return {"id": reflection_id, "score_before": score_before, "score_after": score_after}
