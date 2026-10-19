# Vendored CSL styles

| File | Style | Source | Updated | SHA-256 | Licence |
|---|---|---|---|---|---|
| `australian-guide-to-legal-citation.csl` | Australian Guide to Legal Citation, 4th edition (AGLC4) | Zotero Style Repository, <https://www.zotero.org/styles/australian-guide-to-legal-citation> (fetched 19 October 2026) | 2020-07-09 | `6993a8f6b4ba13c2d8df9dc8fdf6fb1dd36386a2bbedfb4031bd733a6b9a62f7` | CC BY-SA 3.0 |
| `vancouver.csl` | Vancouver (Citing Medicine) | Zotero Style Repository, <https://www.zotero.org/styles/vancouver> (fetched 19 October 2026; upstream SHA-256 `3bcebfe5ac07cba3d17716e77e6ded213435fba77ceb28a55e8e39f1eb9d2dfd`) | 2026-03-29 | `216a7700088661bb782019ce960fd3c222819649854a3e0257fc3e4b33e6b222` (patched) | CC BY-SA 3.0 |

AGLC4 serves the `legal` profile and Vancouver the `vancouver` one. The
other profile styles (`harvard_au`, `ieee`, `chicago`) come from the styles bundled
in `hayagriva`'s `archive` feature, which are also CSL project styles under
CC BY-SA 3.0.

**Patch:** `vancouver.csl` adds one `<name/>` element inside the author
`<names>` (marked with a comment). `hayagriva` 0.10 renders no names
without it, although CSL 1.0.2 makes it optional. The patched style is
shared under the same CC BY-SA 3.0 licence.

Do not edit these files otherwise; refetch them and update the hashes and
dates above.
