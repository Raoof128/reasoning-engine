import json

import pytest

from reasoning_engine.cli import main


def test_cli_scholar_search_outputs_json(tmp_path, db_path, capsys, monkeypatch):
    monkeypatch.setenv("REASONING_ENGINE_DB", db_path)
    monkeypatch.setenv("REASONING_ENGINE_RUNS_DIR", str(tmp_path))

    code = main(["scholar", "search", "Scholar Gateway semantic search", "--limit", "1"])
    captured = capsys.readouterr()
    payload = json.loads(captured.out)

    assert code == 0
    assert payload["evidence"][0]["source_adapter"] == "scholar_gateway"


def test_cli_research_exports_run_pack(tmp_path, db_path, capsys, monkeypatch):
    monkeypatch.setenv("REASONING_ENGINE_DB", db_path)
    monkeypatch.setenv("REASONING_ENGINE_RUNS_DIR", str(tmp_path))

    code = main(
        [
            "research",
            "Scholar Gateway exposes semantic search",
            "--draft",
            "Scholar Gateway exposes semantic search.",
        ]
    )
    captured = capsys.readouterr()
    payload = json.loads(captured.out)

    assert code == 0
    assert payload["attestation"]["valid"] is True


def test_cli_mcp_runs_stdio(monkeypatch):
    calls = []

    def fake_run_mcp(**kwargs):
        calls.append(kwargs)

    monkeypatch.setattr("reasoning_engine.cli.run_mcp", fake_run_mcp)

    assert main(["mcp"]) == 0
    assert calls == [{"transport": "stdio"}]


def test_cli_serve_http_wires_localhost_args(monkeypatch):
    calls = []

    def fake_run_mcp(**kwargs):
        calls.append(kwargs)

    monkeypatch.setattr("reasoning_engine.cli.run_mcp", fake_run_mcp)

    code = main(
        [
            "serve",
            "--transport",
            "http",
            "--host",
            "127.0.0.1",
            "--port",
            "8765",
            "--path",
            "/mcp",
        ]
    )

    assert code == 0
    assert calls == [
        {
            "transport": "streamable-http",
            "host": "127.0.0.1",
            "port": 8765,
            "streamable_http_path": "/mcp",
            "unsafe_bind_public": False,
            "bearer_token": None,
        }
    ]


def test_cli_serve_rejects_public_bind_without_unsafe_flag():
    with pytest.raises(ValueError, match="unsafe-bind-public"):
        main(["serve", "--transport", "http", "--host", "0.0.0.0"])


def test_cli_serve_allows_public_bind_with_unsafe_flag(monkeypatch):
    calls = []

    def fake_run_mcp(**kwargs):
        calls.append(kwargs)

    monkeypatch.setattr("reasoning_engine.cli.run_mcp", fake_run_mcp)

    code = main(
        [
            "serve",
            "--transport",
            "http",
            "--host",
            "0.0.0.0",
            "--unsafe-bind-public",
        ]
    )

    assert code == 0
    assert calls[0]["unsafe_bind_public"] is True


def test_cli_serve_bearer_token_env_does_not_print_secret(monkeypatch, capsys):
    monkeypatch.setenv("REASONING_ENGINE_HTTP_TOKEN", "secret-token")
    calls = []

    def fake_run_mcp(**kwargs):
        assert kwargs["transport"] == "streamable-http"
        calls.append(kwargs)

    monkeypatch.setattr("reasoning_engine.cli.run_mcp", fake_run_mcp)

    code = main(
        [
            "serve",
            "--transport",
            "http",
            "--bearer-token-env",
            "REASONING_ENGINE_HTTP_TOKEN",
        ]
    )
    captured = capsys.readouterr()

    assert code == 0
    assert calls[0]["bearer_token"] == "secret-token"
    assert "secret-token" not in captured.out
    assert "secret-token" not in captured.err


def test_cli_serve_bearer_token_env_requires_existing_secret(monkeypatch):
    monkeypatch.delenv("REASONING_ENGINE_HTTP_TOKEN", raising=False)

    with pytest.raises(ValueError, match="REASONING_ENGINE_HTTP_TOKEN"):
        main(
            [
                "serve",
                "--transport",
                "http",
                "--bearer-token-env",
                "REASONING_ENGINE_HTTP_TOKEN",
            ]
        )
