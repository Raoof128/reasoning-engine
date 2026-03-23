import sqlite3
import tempfile

import pytest


@pytest.fixture
def db_path(tmp_path):
    """Temporary SQLite database for tests."""
    # Will be updated in Task 2 when db module exists
    path = str(tmp_path / "test_reasoning.db")
    return path
