# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes       |

## Reporting a Vulnerability

If you discover a security vulnerability, please report it responsibly:

1. Do NOT open a public GitHub issue.
2. Use GitHub's private vulnerability reporting: go to **Security > Advisories > New draft advisory**.
3. Include steps to reproduce, impact assessment, and any suggested fix.
4. You will receive an acknowledgment within 48 hours.

## Security Considerations

### Content Sanitization

The `sanitize_content` tool strips HTML, script tags, and known prompt injection patterns from web-crawled content before it enters the reasoning pipeline. Detected patterns include:

- `<script>` and `<style>` blocks
- Prompt override attempts ("ignore all previous instructions", "system: override")
- Chat template markers (`[INST]`, `<<SYS>>`, `<|system|>`)

This is a defense-in-depth measure. It reduces risk but does not guarantee complete protection against adversarial inputs.

Externally sourced text is also sanitized before branch traces, source excerpts, critiques, reflections, or episodic memories are persisted. Treat all retrieved content as untrusted, and keep primary-source URLs attached so claims remain independently verifiable.

### MCP Tool Input Validation

All MCP tool inputs are validated at the server boundary. Numeric scores must be in the documented `0.0` to `1.0` range, list and text payloads have bounded sizes, branch mutations are checked against the requested session, and planning updates persist budget state atomically with branch status changes.

### Database

SQLite stores session state, branch scores, and episodic memory. The database file may contain research queries, reasoning traces, and source excerpts. It is excluded from version control via `.gitignore`. Treat it as sensitive data.

The database uses foreign-key checks, WAL mode, a busy timeout, and restrictive permissions for newly created files where the operating system supports them.

### MCP Transport

The default stdio transport opens no network ports; the optional local HTTP transport binds to `127.0.0.1` unless explicitly overridden with `--unsafe-bind-public`. Authentication and authorization for stdio are handled by the Claude Code MCP framework; the HTTP transport supports an optional static bearer token.

`scholar_search_tool` makes outbound HTTPS requests to academic APIs (OpenAlex, Crossref, arXiv, Semantic Scholar, Europe PMC, DOAJ, DBLP, and optionally Scholar Gateway) to retrieve evidence — queries and, for OpenAlex/Crossref, a configurable contact email are sent to these third-party services. Set `REASONING_ENGINE_OFFLINE=1` to disable all outbound calls and use deterministic mock evidence instead. arXiv responses are parsed with `defusedxml` to guard against XXE and billion-laughs payloads from this untrusted network input.

### Dependencies

The project depends on the official MCP Python SDK. Keep dependencies updated. Run `pip audit` periodically to check for known vulnerabilities.
