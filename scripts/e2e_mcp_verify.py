#!/usr/bin/env python3
"""End-to-end check of the MCP server: search, accumulate evidence, verify a draft.

Starts `reasoning-engine mcp` over stdio with a throwaway database, runs a few
live searches in one run, then verifies a Markdown draft against that run's
evidence. Prints how long each call took and what the gate said.

  scripts/e2e_mcp_verify.py                          # target/release/reasoning-engine
  scripts/e2e_mcp_verify.py --binary ~/.local/bin/reasoning-engine --verifier lexical
  scripts/e2e_mcp_verify.py --no-reuse               # the old path: one search on the query

Needs the network. Standard library only.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

QUERY = (
    "How reliable are linguistic signals and detectors for AI-generated text, "
    "and what does paraphrasing do to detection?"
)
SEARCHES = [
    "linguistic features AI-generated text detection domains models",
    "adversarial paraphrasing humanizing AI-generated text detectors",
    "frequent ChatGPT users detect AI-generated text annotators",
    "AI detection false positives non-native academic writing editing",
    "measuring AI slop text taxonomy",
]
DRAFT = """# Detection signals and their limits

## Findings

### Which signals hold up

Many previously proposed linguistic indicators of AI-generated text are strongly context-dependent.
Measures of lexical richness remain robust signals across model families and text domains.
Classifiers based solely on linguistic features can distinguish AI-generated from human-written text.

### Paraphrasing

Adversarial paraphrasing guided by a detector reduces the true positive rate of AI text detectors.
Simple paraphrasing always makes detectors more accurate on every benchmark.
People who frequently use ChatGPT for writing are accurate detectors of AI-generated text.

| Signal | Status |
| --- | --- |
| Lexical richness | robust |

### Fairness

AI text detectors show widely varying false positive rates on human-written texts.
Binary slop judgments are somewhat subjective.
Detectors cure the common cold in 97% of patients.

## Sources

1. El Attar et al. Findings of AACL 2026. https://doi.org/10.18653/2026.findings-aacl.10
2. Cheng et al. NeurIPS 2025. https://doi.org/10.52202/085713-1588
"""


class Client:
    def __init__(self, binary: str, env: dict[str, str]) -> None:
        self.proc = subprocess.Popen(
            [binary, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, env=env, text=True,
        )
        self.next_id = 0
        self.request("initialize", {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "e2e", "version": "0"},
        })
        self.notify("notifications/initialized")

    def notify(self, method: str) -> None:
        self.proc.stdin.write(json.dumps({"jsonrpc": "2.0", "method": method}) + "\n")
        self.proc.stdin.flush()

    def request(self, method: str, params: dict) -> dict:
        self.next_id += 1
        self.proc.stdin.write(json.dumps(
            {"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params}) + "\n")
        self.proc.stdin.flush()
        while True:
            line = self.proc.stdout.readline()
            if not line:
                raise SystemExit("server closed the connection")
            message = json.loads(line)
            if message.get("id") == self.next_id:
                if "error" in message:
                    raise SystemExit(f"{method}: {message['error']}")
                return message["result"]

    def tool(self, name: str, arguments: dict) -> tuple[dict, float, int]:
        started = time.monotonic()
        result = self.request("tools/call", {"name": name, "arguments": arguments})
        elapsed = time.monotonic() - started
        text = result["content"][0]["text"]
        if result.get("isError"):
            raise SystemExit(f"{name} failed: {text[:400]}")
        return json.loads(text), elapsed, len(text)

    def close(self) -> None:
        self.proc.stdin.close()
        self.proc.wait(timeout=20)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    root = Path(__file__).resolve().parent.parent
    ap.add_argument("--binary", default=str(root / "target/release/reasoning-engine"))
    ap.add_argument("--verifier", default="nli", choices=("lexical", "hybrid", "nli"))
    ap.add_argument("--no-reuse", action="store_true", help="do not pass evidence_run_id")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    with tempfile.TemporaryDirectory() as tmp:
        env = dict(os.environ)
        env.update({
            "REASONING_ENGINE_DB": str(Path(tmp) / "reasoning.db"),
            "REASONING_ENGINE_RUNS_DIR": str(Path(tmp) / "runs"),
            "REASONING_ENGINE_VERIFIER": args.verifier,
        })
        client = Client(os.path.expanduser(args.binary), env)
        run, _, _ = client.tool("start_research_run", {"query": QUERY, "mode": "standard"})
        report: dict = {"binary": args.binary, "verifier_requested": args.verifier, "searches": []}
        seen_ids: list[str] = []
        for query in SEARCHES:
            found, elapsed, size = client.tool(
                "scholar_search_tool", {"run_id": run["run_id"], "query": query, "limit": 8})
            ids = [e["evidence_id"] for e in found["evidence"]]
            titles = [e["title"].lower() for e in found["evidence"]]
            seen_ids.extend(ids)
            report["searches"].append({
                "query": query, "seconds": round(elapsed, 2), "results": len(ids),
                "bytes": size, "first_id": ids[0] if ids else None, "last_id": ids[-1] if ids else None,
                "duplicate_titles": len(titles) - len(set(titles)),
            })
        report["distinct_evidence_ids"] = len(set(seen_ids))
        arguments = {"query": QUERY, "draft": DRAFT, "mode": "standard"}
        if not args.no_reuse:
            arguments["evidence_run_id"] = run["run_id"]
        verdict, elapsed, size = client.tool("run_research_pipeline_tool", arguments)
        report["verify"] = {
            "seconds": round(elapsed, 2), "bytes": size,
            "gate": verdict["quality_gate"]["result"],
            "blocking": verdict["quality_gate"].get("blocking_failures"),
            "verifier": verdict.get("verifier"),
            "evidence_reused_from": verdict.get("evidence_reused_from"),
            "evidence_count": verdict.get("evidence_count", len(verdict.get("evidence", []))),
            "summary": verdict.get("summary"),
        }
        if "summary" not in verdict:
            claims = json.loads((Path(verdict["run_pack"]) / "claim_verifications.json").read_text())
            counts: dict[str, int] = {}
            for c in claims:
                counts[c["support_status"]] = counts.get(c["support_status"], 0) + 1
            report["verify"]["by_status_from_pack"] = counts
        client.close()

    if args.json:
        json.dump(report, sys.stdout, indent=2)
        print()
        return 0
    total = sum(s["seconds"] for s in report["searches"])
    print(f"binary: {report['binary']}")
    for s in report["searches"]:
        print(f"  search {s['seconds']:>6.2f}s  {s['results']} results  {s['first_id']}..{s['last_id']}"
              f"  dup titles {s['duplicate_titles']}  {s['bytes']} bytes  | {s['query']}")
    print(f"  {len(report['searches'])} searches in {total:.1f}s; {report['distinct_evidence_ids']} distinct evidence IDs in the run")
    v = report["verify"]
    print(f"  verify {v['seconds']:.2f}s  gate={v['gate']}  verifier={v['verifier']}  "
          f"evidence={v['evidence_count']}  reused_from={'yes' if v['evidence_reused_from'] else 'no'}  {v['bytes']} bytes")
    if v["summary"]:
        print(f"  claims={v['summary']['claims']}  by_status={v['summary']['by_status']}")
        for item in v["summary"]["needs_attention"]:
            print(f"    [{item['status']}] {item['text'][:90]}")
    else:
        print(f"  by_status={v.get('by_status_from_pack')}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
