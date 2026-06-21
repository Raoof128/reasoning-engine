"""Command-line interface for reasoning-engine."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path

from reasoning_engine.db import init_db
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

    scholar = subparsers.add_parser("scholar")
    scholar_sub = scholar.add_subparsers(dest="scholar_command", required=True)
    scholar_search = scholar_sub.add_parser("search")
    scholar_search.add_argument("query")
    scholar_search.add_argument("--limit", type=int, default=10)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
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
