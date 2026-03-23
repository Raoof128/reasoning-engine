# Contributing to Reasoning Engine

## Development Setup

1. Clone the repository
2. Create a virtual environment: `python3 -m venv .venv`
3. Activate: `source .venv/bin/activate`
4. Install with dev deps: `pip install -e ".[dev]"`
5. Run tests: `pytest -v`

## Code Standards

- Python 3.12+ with full type hints on all function signatures
- All functions must have docstrings
- Use `get_connection()` context manager for database access (never manual close)
- Named constants for thresholds (no magic numbers)
- Run `ruff check .` before committing

## Testing

- Follow TDD: write test first, then implementation
- All tests in `tests/` directory
- Run: `pytest -v --tb=short`
- All PRs must maintain 100% test pass rate

## Pull Request Process

1. Fork the repo and create a feature branch
2. Write tests for new functionality
3. Implement the feature
4. Ensure all tests pass
5. Update documentation if needed
6. Submit PR with clear description

## Commit Convention

Use conventional commits:
- `feat:` new feature
- `fix:` bug fix
- `refactor:` code improvement
- `docs:` documentation
- `test:` test changes
- `chore:` maintenance
