"""Generate crates/re-core/src/html_entities.rs from CPython's own tables.

The Rust sanitizer must decode entities exactly like Python's
``html.unescape``, so the tables come straight from the oracle:
``html.entities.html5``, ``html._invalid_charrefs`` and
``html._invalid_codepoints``.

Run from the repo root:  python3 -I scripts/gen_html_entities.py
"""

from __future__ import annotations

import html
import html.entities
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parents[1] / "crates/re-core/src/html_entities.rs"


def rust_str(value: str) -> str:
    parts = []
    for ch in value:
        if ch in ('"', "\\"):
            parts.append("\\" + ch)
        elif 0x20 <= ord(ch) < 0x7F:
            parts.append(ch)
        else:
            parts.append(f"\\u{{{ord(ch):x}}}")
    return '"' + "".join(parts) + '"'


def main() -> None:
    names = sorted(html.entities.html5.items(), key=lambda kv: kv[0].encode("utf-8"))
    charrefs = sorted(html._invalid_charrefs.items())
    codepoints = sorted(html._invalid_codepoints)

    lines = [
        "//! HTML entity tables generated from `CPython` by `scripts/gen_html_entities.py`.",
        "//!",
        f"//! Source: Python {sys.version.split()[0]} `html.entities.html5` "
        f"({len(names)} names), `html._invalid_charrefs` ({len(charrefs)}),",
        f"//! `html._invalid_codepoints` ({len(codepoints)}). Do not edit by hand.",
        '#![allow(clippy::unreadable_literal, reason = "generated table")]',
        "",
        "/// HTML5 named references, sorted by UTF-8 bytes for binary search.",
        "pub(crate) static HTML5: &[(&str, &str)] = &[",
    ]
    lines += [f"    ({rust_str(k)}, {rust_str(v)})," for k, v in names]
    lines += [
        "];",
        "",
        "/// Numeric references remapped by the HTML5 spec (mostly Windows-1252).",
        "pub(crate) static INVALID_CHARREFS: &[(u32, &str)] = &[",
    ]
    lines += [f"    (0x{k:x}, {rust_str(v)})," for k, v in charrefs]
    lines += [
        "];",
        "",
        "/// Code points that decode to the empty string.",
        "pub(crate) static INVALID_CODEPOINTS: &[u32] = &[",
    ]
    lines += [f"    0x{cp:x}," for cp in codepoints]
    lines += ["];", ""]
    OUT.write_text("\n".join(lines), encoding="utf-8")
    print(f"wrote {OUT} ({len(names)} names)")


if __name__ == "__main__":
    main()
