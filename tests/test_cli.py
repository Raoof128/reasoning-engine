import json

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
