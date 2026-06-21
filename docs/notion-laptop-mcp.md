# Notion Laptop MCP Tunnel

This mode runs Verifiable Research Engine on a Mac and exposes it to Notion AI
through a temporary Cloudflare HTTPS tunnel.

Use it for development, demos, and personal testing. Do not treat it as a
production or high-availability deployment.

## Runtime Model

```text
Notion AI Custom MCP
  -> Cloudflare temporary HTTPS URL
  -> 127.0.0.1:8765/mcp on the laptop
  -> reasoning-engine Streamable HTTP MCP server
```

The local MCP server remains bound to `127.0.0.1`. The script does not open a
raw public port and does not bind the MCP server to `0.0.0.0`.

## Quick Start

From the repository root:

```bash
chmod +x ./run-notion-mcp-laptop.sh
./run-notion-mcp-laptop.sh
```

On macOS, you can also double-click `run-notion-mcp-laptop.command` from
Finder. It opens Terminal and runs the same launcher from the repository root.

The script will:

1. Verify macOS and Homebrew.
2. Install `cloudflared` with Homebrew if needed.
3. Install this project in editable mode if `reasoning-engine` is not already available.
4. Create or reuse `~/.reasoning-engine/notion-http.env`.
5. Start `reasoning-engine serve` on `127.0.0.1:8765`.
6. Start a temporary Cloudflare Tunnel.
7. Print the public Notion MCP URL.

## Notion Configuration

When the script prints a URL like:

```text
https://example.trycloudflare.com/mcp
```

add it to Notion as the Custom MCP server URL.

For authentication, configure this header in Notion:

```text
Authorization: Bearer <token>
```

The token is stored locally in:

```text
~/.reasoning-engine/notion-http.env
```

Do not commit, screenshot, paste into public chats, or store this token in run
packs or SQLite.

## Security Behavior

- The server binds to `127.0.0.1` by default.
- Public bind addresses are rejected unless `--unsafe-bind-public` is explicitly passed.
- `--bearer-token-env` now requires the named environment variable to exist.
- HTTP requests without the correct bearer token are rejected.
- CLI status output redacts token values.
- FastMCP Host and Origin protection remains enabled through the pinned MCP SDK version.

## Operational Limits

Laptop mode works only while:

- the Mac is awake,
- the launcher script is running,
- Cloudflare Tunnel is running,
- the temporary Cloudflare URL is still active.

For daily or 24/7 use, prefer a persistent Cloudflare Tunnel or a properly
hardened HTTPS server deployment.
