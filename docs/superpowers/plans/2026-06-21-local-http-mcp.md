# Local Streamable HTTP MCP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a safe local Streamable HTTP MCP server mode while preserving STDIO as the default transport.

**Architecture:** Introduce a small transport module that constructs configured FastMCP instances and validates HTTP bind settings. The CLI delegates `mcp` and `serve --transport http` to that module, and tests exercise the ASGI app directly for Host/Origin protection.

**Tech Stack:** Python 3.12, MCP Python SDK FastMCP, Starlette TestClient, pytest, ruff.

---

## Tasks

### Task 1: Transport Factory and Security Validation

**Files:**
- Create: `src/reasoning_engine/transport.py`
- Modify: `src/reasoning_engine/server.py`
- Test: `tests/test_http_transport.py`

Steps:

- [ ] Add tests for `validate_http_bind`, safe MCP dependency floor, tool registry parity, and Host/Origin security.
- [ ] Add `transport.py` with `SAFE_MCP_MIN_VERSION`, `validate_http_bind`, `create_mcp`, and `run_mcp`.
- [ ] Refactor `server.py` to build `mcp = create_mcp()` and register tools through `register_tools(mcp)`.
- [ ] Run `pytest tests/test_http_transport.py tests/test_e2e.py -v`.
- [ ] Commit with `feat(http): add local mcp transport factory`.

### Task 2: CLI Serve Command

**Files:**
- Modify: `src/reasoning_engine/cli.py`
- Modify: `tests/test_cli.py`
- Modify: `README.md`
- Modify: `docs/api-reference.md`
- Modify: `docs/examples.md`

Steps:

- [ ] Add CLI tests for `mcp`, HTTP serve argument wiring, unsafe public bind rejection, unsafe opt-in, and bearer-token env redaction.
- [ ] Add `serve --transport http --host --port --path --unsafe-bind-public --bearer-token-env`.
- [ ] Add `mcp` command as explicit STDIO alias.
- [ ] Document local HTTP startup and Codex/Claude Code endpoint path.
- [ ] Run `pytest tests/test_cli.py tests/test_http_transport.py -v`.
- [ ] Commit with `feat(http): add local streamable http cli`.

### Task 3: Full E2E Verification and PR Update

**Files:**
- Test-only verification unless docs need correction.

Steps:

- [ ] Run `ruff check .`.
- [ ] Run `ruff format --check .`.
- [ ] Run `pytest -q`.
- [ ] Run a CLI smoke for argument validation without starting a long-running server.
- [ ] Push branch and update PR #1.

## Self-Review

Coverage:

- Native FastMCP Streamable HTTP is covered by Task 1.
- Localhost default and public bind guard are covered by Tasks 1 and 2.
- Host/Origin protection is covered by Task 1.
- Token non-leakage is covered by Task 2.
- STDIO default compatibility is covered by Task 2 and existing E2E tests.
