"""Validation helpers for MCP tool boundaries."""

from __future__ import annotations

from typing import TYPE_CHECKING
from urllib.parse import urlparse

if TYPE_CHECKING:
    from collections.abc import Iterable

MAX_QUERY_CHARS = 8000
MAX_TRACE_STEPS = 80
MAX_TRACE_STEP_CHARS = 4000
MAX_SOURCES = 50
MAX_SOURCE_FIELD_CHARS = 4000
MAX_CRITIQUE_CHARS = 8000
MAX_LEARNINGS = 30
MAX_LEARNING_CHARS = 2000
MAX_TAGS = 20
MAX_TAG_CHARS = 80
MAX_MEMORY_LIMIT = 20
MAX_TOP_K = 10
MAX_ANGLES = 12
MAX_QUESTION_CHARS = 500


def validate_text(value: str, field_name: str, max_chars: int, *, allow_empty: bool = False) -> str:
    if not isinstance(value, str):
        raise ValueError(f"{field_name} must be a string")
    cleaned = value.strip()
    if not allow_empty and not cleaned:
        raise ValueError(f"{field_name} must not be empty")
    if len(cleaned) > max_chars:
        raise ValueError(f"{field_name} exceeds {max_chars} characters")
    return cleaned


def validate_score(value: float, field_name: str) -> float:
    try:
        score = float(value)
    except (TypeError, ValueError) as exc:
        raise ValueError(f"{field_name} must be a number") from exc
    if not 0.0 <= score <= 1.0:
        raise ValueError(f"{field_name} must be between 0.0 and 1.0")
    return score


def validate_non_negative_int(value: int, field_name: str, max_value: int) -> int:
    if not isinstance(value, int) or isinstance(value, bool):
        raise ValueError(f"{field_name} must be an integer")
    if value < 0:
        raise ValueError(f"{field_name} must be non-negative")
    if value > max_value:
        raise ValueError(f"{field_name} must be <= {max_value}")
    return value


def validate_limited_int(value: int, field_name: str, min_value: int, max_value: int) -> int:
    if not isinstance(value, int) or isinstance(value, bool):
        raise ValueError(f"{field_name} must be an integer")
    if value < min_value or value > max_value:
        raise ValueError(f"{field_name} must be between {min_value} and {max_value}")
    return value


def validate_string_list(
    value: object,
    field_name: str,
    *,
    max_items: int,
    max_item_chars: int,
    allow_empty: bool = False,
) -> list[str]:
    if not isinstance(value, list):
        raise ValueError(f"{field_name} must be a JSON array")
    if not allow_empty and not value:
        raise ValueError(f"{field_name} must not be empty")
    if len(value) > max_items:
        raise ValueError(f"{field_name} must contain at most {max_items} items")
    return [
        validate_text(item, f"{field_name}[{index}]", max_item_chars)
        for index, item in enumerate(value)
    ]


def validate_sources(value: object) -> list[dict]:
    if not isinstance(value, list):
        raise ValueError("sources must be a JSON array")
    if len(value) > MAX_SOURCES:
        raise ValueError(f"sources must contain at most {MAX_SOURCES} items")

    validated = []
    for index, source in enumerate(value):
        if not isinstance(source, dict):
            raise ValueError(f"sources[{index}] must be an object")
        url = validate_text(
            str(source.get("url", "")),
            f"sources[{index}].url",
            MAX_SOURCE_FIELD_CHARS,
            allow_empty=True,
        )
        if url:
            parsed = urlparse(url)
            if parsed.scheme not in {"http", "https"} or not parsed.netloc:
                raise ValueError(f"sources[{index}].url must be an http(s) URL")
        relevance_score = validate_score(
            source.get("relevance_score", 0.0), f"sources[{index}].relevance_score"
        )
        validated.append(
            {
                "url": url,
                "title": validate_text(
                    str(source.get("title", "")),
                    f"sources[{index}].title",
                    MAX_SOURCE_FIELD_CHARS,
                    allow_empty=True,
                ),
                "excerpt": validate_text(
                    str(source.get("excerpt", "")),
                    f"sources[{index}].excerpt",
                    MAX_SOURCE_FIELD_CHARS,
                    allow_empty=True,
                ),
                "relevance_score": relevance_score,
            }
        )
    return validated


def unique_ordered(values: Iterable[str]) -> list[str]:
    seen = set()
    result = []
    for value in values:
        key = value.lower()
        if key not in seen:
            seen.add(key)
            result.append(value)
    return result
