"""Drive the Python verification code for Rust cross-implementation tests.

Usage (stdlib only, so no venv is needed):
  python3 -I tests/fixtures/verify_tool.py export <input.json> <base_dir>
  python3 -I tests/fixtures/verify_tool.py verify <run_pack_dir>
  python3 -I tests/fixtures/verify_tool.py claims <input.json>

`export` writes a run pack with the real `export_run_pack` from the inputs
the Rust test also used and prints its path. `verify` prints
`verify_run_pack_attestation` for a pack. `claims` runs `extract_claims`
and `verify_claims` and prints both, minus random IDs and timestamps.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "src"))

from reasoning_engine.verifiable.claims import extract_claims, verify_claims  # noqa: E402
from reasoning_engine.verifiable.models import ResearchRun  # noqa: E402
from reasoning_engine.verifiable.runpack import (  # noqa: E402
    export_run_pack,
    verify_run_pack_attestation,
)


def _load(path: str):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def _emit(payload) -> None:
    # ASCII output survives any console encoding (Windows cp1252 included).
    print(json.dumps(payload, sort_keys=True))


def export(input_path: str, base_dir: str) -> None:
    data = _load(input_path)
    output = export_run_pack(
        base_dir=base_dir,
        run=ResearchRun(**data["run"]),
        evidence=data["evidence"],
        gaps=data["gaps"],
        claims=data["claims"],
        verifications=data["verifications"],
        provenance=data["provenance"],
        quality_gate=data["quality_gate"],
        report_markdown=data["report_markdown"],
    )
    _emit({"run_pack": str(output)})


def claims(input_path: str) -> None:
    data = _load(input_path)
    records = extract_claims(data["run_id"], data["draft"], data["domain"])
    evidence = [SimpleNamespace(**item) for item in data["evidence"]]
    verifications = verify_claims(records, evidence)
    out_verifications = []
    for verification in verifications:
        item = verification.to_dict()
        item.pop("verification_id")
        item.pop("verified_at")
        out_verifications.append(item)
    _emit(
        {
            "claims": [claim.to_dict() for claim in records],
            "verifications": out_verifications,
        }
    )


if __name__ == "__main__":
    command = sys.argv[1] if len(sys.argv) > 1 else ""
    if command == "export" and len(sys.argv) == 4:
        export(sys.argv[2], sys.argv[3])
    elif command == "verify" and len(sys.argv) == 3:
        _emit(verify_run_pack_attestation(sys.argv[2]))
    elif command == "claims" and len(sys.argv) == 3:
        claims(sys.argv[2])
    else:
        sys.exit(__doc__)
