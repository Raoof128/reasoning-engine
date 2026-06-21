# Local Streamable HTTP MCP Design

Date: 2026-06-21
Status: Approved

## Goal

Add a local Streamable HTTP MCP transport for `reasoning-engine` while keeping
STDIO as the default transport and preserving the same MCP tool registry.

## Approved Direction

- Use the installed MCP Python SDK FastMCP native transport:
  `mcp.run(transport="streamable-http")`.
- Keep STDIO as the default server mode.
- Add CLI:

```bash
reasoning-engine serve --transport http --host 127.0.0.1 --port 8765
```

- Default endpoint path is `/mcp`.
- Bind to `127.0.0.1` by default.
- Reject public bind addresses such as `0.0.0.0` unless
  `--unsafe-bind-public` is passed.
- Reuse the same tool-registration function for STDIO and HTTP.

## Security Constraints

- Require `mcp>=1.23.0` for HTTP transport safety. The current dependency
  `mcp>=1.24.0,<2` satisfies this.
- For local HTTP mode, keep FastMCP DNS rebinding protection enabled by using
  constructor-level `host="127.0.0.1"` defaults.
- Validate requested host before server startup.
- Test that localhost Host/Origin values are accepted by the ASGI app.
- Test that suspicious Host/Origin values are rejected by the ASGI app.
- If an optional bearer token env name is configured, never print or log the
  token value.

## Non-Goals

- Hosted remote MCP.
- OAuth server implementation.
- SSE transport.
- Custom REST API.
- Public HTTP mode as a recommended workflow.

## Acceptance Criteria

- `reasoning-engine serve --transport http --host 127.0.0.1 --port 8765`
  starts Streamable HTTP MCP on `/mcp`.
- `reasoning-engine mcp` still starts STDIO MCP.
- Public bind is rejected unless `--unsafe-bind-public` is passed.
- The HTTP and STDIO modes expose the same tools.
- Tests cover localhost Host/Origin acceptance, suspicious Host/Origin
  rejection, public bind rejection, token non-leakage, and CLI argument wiring.
