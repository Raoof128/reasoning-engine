import pytest

from reasoning_engine.db import init_db


@pytest.fixture
def db_path(tmp_path):
    """Temporary SQLite database for tests."""
    path = str(tmp_path / "test_reasoning.db")
    init_db(path)
    return path
