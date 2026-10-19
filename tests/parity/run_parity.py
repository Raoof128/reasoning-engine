"""Run the same MCP tool script against the Python and Rust servers and diff.

Usage (repo root, after `cargo build --release -p re-cli`):
  .venv/bin/python tests/parity/run_parity.py \
      [--python-bin .venv/bin/reasoning-engine] \
      [--rust-bin target/release/reasoning-engine]

Both servers run `mcp` over stdio with REASONING_ENGINE_OFFLINE=1 and their
own temporary database and run directory. Every response text is parsed as
JSON when it is JSON, then normalised: IDs become stable placeholders in
order of first appearance, timestamps and temp paths become fixed tokens,
and hashes over timestamped artifacts are masked. Any remaining difference
is printed as a unified diff and the exit code is 1.
"""

from __future__ import annotations

import argparse
import asyncio
import difflib
import json
import os
import re
import sys
import tempfile
from pathlib import Path
from typing import Any

from mcp import ClientSession
from mcp.client.stdio import StdioServerParameters, stdio_client

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = Path(__file__).with_name("script.json")

# Order matters: the specific prefixed IDs before bare hex.
ID_PATTERNS = [
    re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"),
    re.compile(r"run_[0-9a-f]{12}"),
    re.compile(r"ver_[0-9a-f]{12}"),
    re.compile(r"gap_run_[0-9a-f]{12}_[0-9a-f]{8}"),
]
TIMESTAMPS = [
    (re.compile(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z"), "<iso-time>"),
    (re.compile(r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}"), "<sqlite-time>"),
]
# Hashes over artifacts that embed timestamps or random IDs.
MASKED_KEYS = {"run_pack_hash"}
# Pydantic's per-error suffixes name its own version and error codes; the
# Rust port keeps everything else in the message.
PYDANTIC_SUFFIXES = [
    re.compile(r" \[type=[^\n]*\]$", re.MULTILINE),
    re.compile(r"\n    For further information visit \S+"),
]


# Lists whose order is not defined in Python either: get_session_state
# orders by (created_at, id), and branches created in the same second
# tie-break on random UUIDs. Compared as sorted lists.
UNORDERED = {("get_session_state", "branches")}


def _sort_unordered(tool: str, payload: Any) -> Any:
    if isinstance(payload, dict):
        for key in list(payload):
            if (tool, key) in UNORDERED and isinstance(payload[key], list):
                payload[key] = sorted(payload[key], key=lambda v: json.dumps(v, sort_keys=True))
    return payload


class Normaliser:
    """Replace volatile values consistently within one server's transcript."""

    def __init__(self, temp_root: str) -> None:
        self.temp_root = temp_root
        self.ids: dict[str, str] = {}

    def _id(self, match: re.Match[str]) -> str:
        token = match.group(0)
        if token not in self.ids:
            self.ids[token] = f"<id{len(self.ids) + 1}>"
        return self.ids[token]

    def text(self, value: str) -> str:
        value = value.replace(self.temp_root, "<tmp>").replace("\\", "/")
        for pattern in PYDANTIC_SUFFIXES:
            value = pattern.sub("", value)
        for pattern in ID_PATTERNS:
            value = pattern.sub(self._id, value)
        for pattern, token in TIMESTAMPS:
            value = pattern.sub(token, value)
        return value

    def value(self, value: Any, key: str | None = None) -> Any:
        if key in MASKED_KEYS and isinstance(value, str):
            return "<hash>"
        if isinstance(value, dict):
            return {k: self.value(v, k) for k, v in value.items()}
        if isinstance(value, list):
            return [self.value(v) for v in value]
        if isinstance(value, str):
            return self.text(value)
        return value


def _resolve(args: Any, results: dict[str, Any]) -> Any:
    if isinstance(args, dict):
        return {k: _resolve(v, results) for k, v in args.items()}
    if isinstance(args, list):
        return [_resolve(v, results) for v in args]
    if isinstance(args, str) and args.startswith("$"):
        step, _, key = args[1:].partition(".")
        return results[step][key]
    return args


async def run_server(label: str, command: list[str], steps: list[dict]) -> list[dict]:
    temp = tempfile.mkdtemp(prefix=f"parity-{label}-")
    env = {
        **{
            k: v
            for k, v in os.environ.items()
            if not k.startswith(("REASONING_ENGINE_", "SCHOLAR_"))
        },
        "HOME": temp,
        "USERPROFILE": temp,
        "REASONING_ENGINE_OFFLINE": "1",
        "REASONING_ENGINE_DB": str(Path(temp) / "reasoning.db"),
        "REASONING_ENGINE_RUNS_DIR": str(Path(temp) / "runs"),
        "REASONING_ENGINE_CONFIG": str(Path(temp) / "config.toml"),
    }
    params = StdioServerParameters(command=command[0], args=[*command[1:], "mcp"], env=env)
    normaliser = Normaliser(temp)
    transcript = []
    results: dict[str, Any] = {}
    with open(os.devnull, "w") as devnull:
        async with (
            stdio_client(params, errlog=devnull) as (read, write),
            ClientSession(read, write) as session,
        ):
            await session.initialize()
            for step in steps:
                response = await session.call_tool(step["tool"], _resolve(step["args"], results))
                text = response.content[0].text if response.content else ""
                try:
                    payload: Any = json.loads(text)
                    results[step["name"]] = payload
                except json.JSONDecodeError:
                    payload = text
                transcript.append(
                    {
                        "step": step["name"],
                        "tool": step["tool"],
                        "is_error": bool(response.isError),
                        "result": _sort_unordered(step["tool"], normaliser.value(payload)),
                    }
                )
    return transcript


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--python-bin", default=str(ROOT / ".venv" / "bin" / "reasoning-engine"))
    parser.add_argument("--rust-bin", default=str(ROOT / "target" / "release" / "reasoning-engine"))
    parser.add_argument("--out", help="write both transcripts here as JSON")
    args = parser.parse_args()

    steps = json.loads(SCRIPT.read_text(encoding="utf-8"))["steps"]
    python = asyncio.run(run_server("py", [args.python_bin], steps))
    rust = asyncio.run(run_server("rs", [args.rust_bin], steps))
    if args.out:
        Path(args.out).write_text(json.dumps({"python": python, "rust": rust}, indent=2) + "\n")

    def lines(transcript: list[dict]) -> list[str]:
        return json.dumps(transcript, indent=2, sort_keys=True, ensure_ascii=False).splitlines()

    diff = list(difflib.unified_diff(lines(python), lines(rust), "python", "rust", lineterm=""))
    if diff:
        print("\n".join(diff))
        print(f"\nparity: FAILED ({len(steps)} steps)", file=sys.stderr)
        return 1
    print(f"parity: OK ({len(steps)} steps, identical after normalisation)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
