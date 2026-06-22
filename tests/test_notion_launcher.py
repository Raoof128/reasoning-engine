from pathlib import Path


def test_notion_launcher_exports_http_token_after_validation():
    script = Path("run-notion-mcp-laptop.sh").read_text()

    validation = 'if [[ -z "${REASONING_ENGINE_HTTP_TOKEN:-}" ]]; then'
    export = "export REASONING_ENGINE_HTTP_TOKEN"
    server_start = '"$APP_NAME" serve \\'

    assert validation in script
    assert export in script
    assert script.index(validation) < script.index(export) < script.index(server_start)


def test_clickable_notion_launcher_delegates_to_shell_script():
    script = Path("run-notion-mcp-laptop.command").read_text()

    assert "#!/usr/bin/env bash" in script
    assert 'cd "$SCRIPT_DIR"' in script
    assert "exec ./run-notion-mcp-laptop.sh" in script
