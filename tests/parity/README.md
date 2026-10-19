# Python vs Rust MCP parity

`run_parity.py` starts both servers over stdio (`reasoning-engine mcp`) with
`REASONING_ENGINE_OFFLINE=1` and separate temporary databases, runs every
step in `script.json` against each, and diffs the responses.

```bash
cargo build --release -p re-cli
.venv/bin/python tests/parity/run_parity.py            # exit 1 on any difference
.venv/bin/python tests/parity/run_parity.py --out t.json  # keep both transcripts
```

What is normalised before comparing, and why:

| Normalised | Why |
|---|---|
| UUIDs, `run_…`, `ver_…`, `gap_…` IDs | Random per run; replaced by placeholders in order of first appearance, so references between steps still have to line up. |
| `…T…Z` and SQLite `YYYY-MM-DD HH:MM:SS` timestamps | Wall-clock time. |
| Temp-directory prefixes in paths | Each server has its own. |
| `run_pack_hash` | Hashes artifacts that embed timestamps and IDs. |
| Pydantic's ` [type=…, input_value=…]` and documentation-URL suffixes | Pydantic-version specific; the rest of every validation message must match. |
| Order of `get_session_state.branches` | Python orders by `(created_at, id)`, so branches created in the same second tie-break on random UUIDs; the order is undefined there too. |

Everything else, including error text, key order and float formatting, must
be identical. The CI `parity` job runs this on every push.
