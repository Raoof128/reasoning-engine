#!/usr/bin/env bash
# Cold-start comparison of the Python and Rust CLIs.
# Usage: scripts/bench-startup.sh   (from the repo root)
set -euo pipefail

PY_BIN=".venv/bin/reasoning-engine"
RS_BIN="target/release/reasoning-engine"

if ! command -v hyperfine >/dev/null 2>&1; then
    echo "hyperfine not found; install it with: cargo install hyperfine --locked" >&2
    exit 1
fi
if [[ ! -x "$PY_BIN" ]]; then
    echo "Python CLI missing at $PY_BIN; run: python3 -m venv .venv && .venv/bin/pip install -e '.[dev]'" >&2
    exit 1
fi
if [[ ! -x "$RS_BIN" ]]; then
    echo "Rust CLI missing at $RS_BIN; run: cargo build --release -p re-cli" >&2
    exit 1
fi

# --help measures process start; verify-runpack on a missing directory adds
# config loading and JSON output without touching the network.
hyperfine --warmup 3 --export-markdown target/bench-startup.md \
    "$PY_BIN --help" \
    "$RS_BIN --help" \
    "$RS_BIN verify-runpack /nonexistent || true"
echo "Results written to target/bench-startup.md"
