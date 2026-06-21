"""Command-line interface for reasoning-engine."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path

from reasoning_engine.db import init_db
from reasoning_engine.transport import run_mcp, validate_http_bind
from reasoning_engine.verifiable.service import VerifiableResearchService


def _service() -> VerifiableResearchService:
    db_path = os.environ.get("REASONING_ENGINE_DB")
    runs_dir = os.environ.get("REASONING_ENGINE_RUNS_DIR", "runs")
    initialized = init_db(db_path)
    return VerifiableResearchService(initialized, Path(runs_dir))


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="reasoning-engine")
    subparsers = parser.add_subparsers(dest="command", required=True)

    research = subparsers.add_parser("research")
    research.add_argument("query")
    research.add_argument("--draft", required=True)
    research.add_argument("--mode", default="standard")
    research.add_argument("--profile", default="auto")

    subparsers.add_parser("mcp")

    serve = subparsers.add_parser("serve")
    serve.add_argument("--transport", choices=["http"], default="http")
    serve.add_argument("--host", default="127.0.0.1")
    serve.add_argument("--port", type=int, default=8765)
    serve.add_argument("--path", default="/mcp")
    serve.add_argument("--unsafe-bind-public", action="store_true")
    serve.add_argument("--bearer-token-env", default="")

    scholar = subparsers.add_parser("scholar")
    scholar_sub = scholar.add_subparsers(dest="scholar_command", required=True)
    scholar_search = scholar_sub.add_parser("search")
    scholar_search.add_argument("query")
    scholar_search.add_argument("--limit", type=int, default=10)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    if args.command == "mcp":
        run_mcp(transport="stdio")
        return 0

    if args.command == "serve":
        validate_http_bind(
            host=args.host,
            port=args.port,
            unsafe_bind_public=args.unsafe_bind_public,
        )
        if args.bearer_token_env:
            has_token = bool(os.environ.get(args.bearer_token_env))
            print(
                json.dumps(
                    {
                        "bearer_token_env": args.bearer_token_env,
                        "has_bearer_token": has_token,
                        "token_value": "<redacted>" if has_token else None,
                    },
                    sort_keys=True,
                )
            )
        run_mcp(
            transport="streamable-http",
            host=args.host,
            port=args.port,
            streamable_http_path=args.path,
            unsafe_bind_public=args.unsafe_bind_public,
        )
        return 0

    service = _service()

    if args.command == "research":
        payload = service.run_research_pipeline(
            query=args.query,
            draft=args.draft,
            mode=args.mode,
            profile=args.profile,
        )
    elif args.command == "scholar" and args.scholar_command == "search":
        run = service.start_run(args.query)
        payload = service.scholar_search(run.run_id, args.query, args.limit)
        payload.pop("evidence_records", None)
    else:
        parser.error("unsupported command")

    print(json.dumps(payload, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
