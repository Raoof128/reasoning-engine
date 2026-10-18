"""Content sanitizer for web-crawled data."""

import re
from html import unescape

HTML_TAG_RE = re.compile(r"<[^>]+>")
SCRIPT_RE = re.compile(r"<script[^>]*>.*?</script>", re.DOTALL | re.IGNORECASE)
STYLE_RE = re.compile(r"<style[^>]*>.*?</style>", re.DOTALL | re.IGNORECASE)
MARKDOWN_LINK_RE = re.compile(r"\[([^\]]+)\]\(([^)]+)\)")
CONTROL_CHAR_RE = re.compile(r"[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]")
ZERO_WIDTH_RE = re.compile(r"[\u200b-\u200f\u202a-\u202e\u2060-\u206f]")

INJECTION_PATTERNS = [
    re.compile(r"ignore\s+(all\s+)?previous\s+instructions?", re.IGNORECASE),
    re.compile(r"system:\s*override", re.IGNORECASE),
    re.compile(r"you\s+are\s+now\s+a\b", re.IGNORECASE),
    re.compile(r"disregard\s+(all\s+)?(prior|previous|above)", re.IGNORECASE),
    re.compile(r"new\s+instructions?:\s*", re.IGNORECASE),
    re.compile(r"<\|?(system|user|assistant)\|?>", re.IGNORECASE),
    re.compile(r"\[INST\]|\[/INST\]|<<SYS>>|<</SYS>>", re.IGNORECASE),
    re.compile(r"developer\s*message\s*:", re.IGNORECASE),
    re.compile(r"tool\s*call\s*:", re.IGNORECASE),
]

# Removing one injection phrase can splice its surroundings into another
# ("ignore previous ignore previous instructions instructions"), so the
# removal pass repeats until the text stops changing.
MAX_INJECTION_PASSES = 8


def _remove_injection_patterns(text: str) -> str:
    for pattern in INJECTION_PATTERNS:
        text = pattern.sub("", text)
    return text


def _strip_injections(text: str) -> str:
    for _ in range(MAX_INJECTION_PASSES):
        cleaned = _remove_injection_patterns(text)
        if cleaned == text:
            return cleaned
        text = cleaned
    if any(pattern.search(text) for pattern in INJECTION_PATTERNS):
        text = _remove_injection_patterns(text)
    return text


def sanitize_content(raw: str) -> str:
    if not raw or not raw.strip():
        return ""
    text = unescape(raw)
    text = ZERO_WIDTH_RE.sub("", text)
    text = CONTROL_CHAR_RE.sub(" ", text)
    text = SCRIPT_RE.sub("", text)
    text = STYLE_RE.sub("", text)
    text = MARKDOWN_LINK_RE.sub(r"\1 (\2)", text)
    text = HTML_TAG_RE.sub("", text)
    text = _strip_injections(text)
    text = re.sub(r"\s+", " ", text).strip()
    return text
