"""Write the Python server's tools/list (name, description, inputSchema).

Usage: .venv/bin/python -I tests/fixtures/dump_mcp_tools.py
Needs the venv (the `mcp` package). The Rust server's tool list is tested
against tests/fixtures/mcp_tools.json, so regenerate it whenever a Python
tool signature or docstring changes.
"""

from __future__ import annotations

import asyncio
import json
import os
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "src"))
_TMP = tempfile.mkdtemp()
os.environ["REASONING_ENGINE_DB"] = str(Path(_TMP) / "tools.db")
os.environ["REASONING_ENGINE_RUNS_DIR"] = str(Path(_TMP) / "runs")

from mcp.shared.memory import create_connected_server_and_client_session  # noqa: E402

from reasoning_engine.transport import create_mcp  # noqa: E402


async def main() -> None:
    server = create_mcp()
    async with create_connected_server_and_client_session(server._mcp_server) as client:
        init = client.get_server_capabilities()
        tools = (await client.list_tools()).tools
    payload = {
        "tools": [
            {"name": t.name, "description": t.description, "inputSchema": t.inputSchema}
            for t in tools
        ],
        "has_tools_capability": init is not None and init.tools is not None,
    }
    out = ROOT / "tests" / "fixtures" / "mcp_tools.json"
    out.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {len(tools)} tools")


asyncio.run(main())
