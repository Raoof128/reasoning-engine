# Beast Mode: What the Engine Could Become

Status: vision, deliberately ambitious. Builds on `rust-rewrite-plan-2026.md`
(the "gauntlet"). Every idea here is grounded in a real crate or a real
protocol feature as of October 2026, but none of it is promised. Treat it as
a menu: pick the tier that matches your appetite.

The organising idea: today the engine is a **calculator** that Claude calls.
It could become a **research organism** that gets smarter with every run,
proves what it knows, notices when it stops being true, and lets Claude spend
its tokens on thinking rather than plumbing.

## 0. The one-paragraph pitch

A single static binary that: prefetches evidence for every research angle in
parallel while Claude is still reading the first one; reads full open-access
papers, not 1,500-character abstracts; checks each claim with an entailment
model and a numeric-consistency checker; builds a citation graph to tell
*independent* corroboration from an echo chamber; learns from its own history
which branches pay off; signs every run pack into a tamper-evident log; and
re-verifies old reports on a schedule so research visibly rots instead of
silently lying. Works in English and Persian. Runs offline after first use.

## 1. Tier S: the moonshots

### 1.1 Speculative retrieval via the MCP tasks extension

The skill runs sequentially in one context by design. The server does not
have to. When `plan_research_angles_tool` returns six angles, the server
starts six background fan-outs immediately, returning a task handle. By the
time Claude registers branch one, evidence for angles two to six is already
cached, deduped and ranked.

- Protocol: MCP 2026-07-28 tasks extension (`io.modelcontextprotocol/tasks`,
  `tasks/get` polling, unsolicited task handles).
- Rust: `tokio::JoinSet`, content-addressed cache in SQLite keyed by
  `(source, normalised query)`, TTL per source.
- Payoff: the research loop stops waiting on the network at all. Angle
  investigation becomes "read what is already there".

### 1.2 Full-text verification

Abstracts are where claims get overstated; methods sections are where they
get qualified. Use OpenAlex `open_access.oa_url` and Europe PMC full-text XML
to pull the open-access version, extract text, chunk by section, and verify
claims against the *body* of the paper.

- Rust: `pdfium-render` 0.9.4 (needs the pdfium binary; ship it in the
  release) or `pdf-extract` 0.12.1 (pure Rust, weaker on layout). Europe PMC
  full text is XML, so `quick-xml` handles the easy half with no PDF at all.
- Chunk store: `usearch` 2.26.5 (single-file HNSW) alongside SQLite, keyed by
  content hash, so a paper read once is read forever.
- Verifier impact: the NLI tier gets a section label as context ("results"
  vs "limitations"), and a claim supported only by the introduction is marked
  `partially_supported` with the reason stated.

### 1.3 A process reward model that learns from this machine's own history

Every session already records traces, critiques, score before and after
reflexion, and whether the final gate passed. That is a labelled dataset.
Train a small model on it: embed the trace, predict the probability that the
branch ends up in the consensus set.

- Rust: `fastembed` for trace embeddings, `linfa` 0.8.1 for logistic
  regression or a small gradient-boosted model, retrained by
  `reasoning-engine calibrate` after every N sessions, stored as a versioned
  artifact next to the database.
- Use: a *prior* for UCB (`q_value` initialised from the model instead of
  0.0), a second opinion against Claude's self-score (large disagreement
  triggers reflexion), and a better difficulty estimator (the model has seen
  which queries actually burned budget).
- Guard rails: cold start uses the deterministic heuristics; the learned prior
  is blended in with a weight that grows with data volume; everything is
  logged so a bad model is a diff, not a mystery.

### 1.4 Evidence graph and corroboration independence

Two papers that support a claim but share an author, a lab, or a citation
chain are one piece of evidence wearing two hats. OpenAlex exposes
`referenced_works`, `authorships` and institutions.

- Rust: `petgraph` 0.8.3. Build a per-run graph: evidence nodes, citation
  edges, shared-author edges. For each claim compute an **independence
  score**: the number of connected components among its supporting evidence.
- Retraction cascade: if a node is retracted, every claim whose only support
  is downstream of it is demoted and the run pack gains a `cascade.json`.
- Report output: a Mermaid or Graphviz snippet per claim so the docx can show
  *why* the engine believes something.

### 1.5 Living reports: claims that re-verify themselves

A run pack is a snapshot. Add `reasoning-engine watch <run_id>`: on a
schedule, re-run each claim's evidence search, re-verify, and emit a diff
(new contradicting paper, retraction, status change).

- Protocol: `subscriptions/listen` for push to a connected client; the tasks
  extension for the re-verification jobs; plain cron for the laptop case.
- Rust: `tokio` timers, the same verifier, `insta`-style diff rendering.
- Payoff: the first research assistant whose reports tell you when they go
  stale.

## 2. Tier A: big, bounded wins

### 2.1 Adversarial branch as a first-class mechanism

Reserve one branch in `beam_search` and `forest` strategies for the
counter-position. The server generates *negated* search queries
(`"X does not reduce Y"`, `"no effect of X on Y"`, `"replication failure X"`)
and the NLI tier scores contradiction, not support. A claim that survives a
funded contradiction search earns a confidence bonus; one that does not gets
a steel-manned counter-view section in the report.

### 2.2 Numeric claim checker

Quantitative claims are currently just "requires citation". Parse numbers,
units and comparatives from the claim and from the evidence ("reduces
hallucination by 30%" vs "reduced hallucination rate from 12.4% to 8.7%"),
normalise units, and match within tolerance. A claim whose number appears
nowhere in its supporting evidence cannot be `supported`.

- Rust: a small `nom` grammar for numbers, percentages, ranges, confidence
  intervals and units; no model required.

### 2.3 Proper citations with CSL

The profiles already name styles (Harvard AU, Vancouver, IEEE, Chicago,
legal). `hayagriva` 0.10.1 (the Typst bibliography engine) renders real CSL
styles from structured metadata, so `sources.bib` becomes a correctly
formatted reference list in the user's required style, in the run pack and
in the docx.

### 2.4 Replayable runs

Record every HTTP response (headers stripped of credentials) into the run
pack, content-addressed. `reasoning-engine replay <run_id>` reproduces the
entire deterministic pipeline offline, byte for byte, including the attested
hashes. This is also the perfect test fixture generator for the Rust port.

### 2.5 Signed run packs and a transparency log

Fill the `signature` and `signature_algorithm` fields that are `null` today:
`ed25519-dalek` 3.0.0 signs `attestation.json`; an append-only Merkle log in
SQLite records every run-pack hash; `reasoning-engine verify-runpack` checks
the signature, the inclusion proof and the artifact hashes offline. A
research report becomes something you can hand to a supervisor with a
cryptographic chain of custody.

### 2.6 Calibrated confidence

Raw softmax from an NLI model is not a probability you should quote. With a
few hundred labelled claim-evidence pairs (built from your own run packs),
conformal prediction turns scores into "supported at 90% coverage" sets. It
is a few dozen lines of Rust over sorted residuals, no crate needed.

### 2.7 Source-quality model per profile

Medicine wants systematic reviews and RCTs, law wants primary sources,
security wants vendor advisories and CVE references. Europe PMC exposes
publication types; OpenAlex exposes venue, open-access status and institution
data. Score evidence against the profile's `preferred_source_types` with real
metadata instead of a constant `peer_reviewed_article` label.

### 2.8 Persian and cross-lingual research

You read Persian. The engine should too. `bge-m3` is a multilingual
embedding model that runs through `fastembed`; cross-lingual matching means a
Persian query can find English evidence and an English claim can be checked
against Persian sources. Add adapters for Persian scholarly indexes (SID,
Magiran, ISC) behind a `fa` feature, and let the report render in either
language with the citation style still correct.

## 3. Tier B: quality of life that compounds

- **Embedded audit UI.** `axum` + `askama` 0.16.1 serving a localhost page
  from the same binary: the branch tree, kappa over rounds, the evidence
  graph, every sanitizer removal. No JavaScript build step.
- **Cross-session evidence memory.** Every snippet ever retrieved is embedded
  into `usearch`; `recall_memory_tool` returns "you found this paper on 3
  October for a related question" with the old verification status.
- **Compute-aware mid-run budget.** Re-estimate difficulty after round one
  from observed kappa and evidence density (how many sources returned, how
  spread the scores are), in the spirit of CATS (ICLR 2026), instead of
  committing the whole budget at init.
- **MRTR instead of round trips.** When the quality gate blocks, return
  `input_required` with the offending claims so Claude revises them inside the
  same request rather than through three more tool calls.
- **WASM source adapters.** Compile adapters to WASM components run under
  `wasmtime` 50 with no filesystem and an allow-listed host list. Anyone can
  add a source without a recompile, and a malicious adapter cannot reach your
  disk. The same build lets `re-core` run in a browser demo.
- **Cost ledger.** Per session: API credits used (OpenAlex reports them per
  response), bytes fetched, estimated tokens handed to Claude. Shown at the
  end of every run and stored in provenance.
- **Public benchmark scoreboard.** Evaluate the verifier on SciFact and
  similar scientific claim-verification sets with a `reasoning-engine bench
  verify` subcommand and publish the numbers in the README. Accuracy claims
  about an accuracy engine should themselves be verifiable.

## 4. How the tiers stack

```
Rust port (gauntlet, phases 1-6)
  └─ 2.4 replayable runs           ← cheapest, makes everything testable
     └─ 2.3 CSL citations + 2.5 signing + 2.2 numeric checker
        └─ 1.1 speculative retrieval (tasks ext)
           └─ 1.2 full-text verification
              └─ 1.4 evidence graph + 2.1 adversarial branch + 2.6 calibration
                 └─ 1.3 learned PRM prior  (needs the data the above produce)
                    └─ 1.5 living reports
```

Suggested first three after the port, in order: replayable runs, speculative
retrieval, full-text verification. Together they turn the engine from "fast"
into "reads the actual papers before Claude does".

## 5. Things I would *not* do

- Run an LLM inside the server. The whole design wins by keeping the server
  deterministic and the LLM in Claude Code on your subscription. Local small
  models (NLI, embeddings, a tiny PRM) are classifiers, not generators, and
  stay reproducible.
- Replace SQLite with a server database. A research workstation should stay
  a single file you can back up and `scp`.
- Add a cloud component. Everything above runs on a laptop; the Cloudflare
  tunnel remains the only way out, opt-in.

## Crate check (crates.io, 2026-10-18)

| Crate | Version | For |
|---|---|---|
| `hayagriva` | 0.10.1 | CSL citation rendering |
| `petgraph` | 0.8.3 | evidence graph |
| `usearch` | 2.26.5 | HNSW vector index |
| `sqlite-vec` | 0.1.10-alpha.4 | alternative in-SQLite vectors (alpha; prefer usearch) |
| `wasmtime` | 50.0.0-rc.1 | sandboxed adapters |
| `pdfium-render` / `pdf-extract` | 0.9.4 / 0.12.1 | full-text extraction |
| `linfa` | 0.8.1 | learned PRM prior |
| `ed25519-dalek` | 3.0.0 | signed attestations |
| `askama` | 0.16.1 | embedded audit UI |
| `unicode-segmentation` | 1.13.3 | sentence-level claim extraction |
| `wiremock` | 0.6.5 | recorded-HTTP adapter tests |
