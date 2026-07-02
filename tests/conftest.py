import pytest

from reasoning_engine.db import init_db


@pytest.fixture(autouse=True)
def _offline_sources(monkeypatch):
    """Keep the search aggregator deterministic and network-free by default.

    Tests that exercise real adapters or the live path delete/override this in
    their own body.
    """
    monkeypatch.setenv("REASONING_ENGINE_OFFLINE", "1")


@pytest.fixture
def db_path(tmp_path):
    """Temporary SQLite database for tests."""
    path = str(tmp_path / "test_reasoning.db")
    init_db(path)
    return path
