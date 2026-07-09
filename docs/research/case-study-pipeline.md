# Case-study publishing and reporting pipeline

Date: 2026-07-08

## Question

We want to run the full decombine loop (index → embed → analyze →
markdown) on many permissively-licensed repos across ~12 languages, publish
the raw reports on a browsable site, and have an LLM write a narrative report
per repo so a reader can see both *what decombine finds* and *how to read it*.
How should the pipeline, the site, the LLM layer, and the automation be
architected — reproducibly, and without republishing GPL snippets?

## Facts established

- decombine already emits a deterministic markdown tree per run:
  `report/index.md` (sectioned Product / Mixed / Test-and-docs, plus a
  Cross-directory table and an ignore section), `report/cluster-NN.md`
  (top pairs + exact-group source excerpts), and, for `compare`,
  `report/compare/index.md` + one file per match class. Every report header
  records the full model identity (`model / backend / version / dims /
  provider / quantization`), thresholds, retention, timestamp, and project
  roots (`src/report/markdown.rs`, `ReportMeta`). **This header line is the
  reproducibility anchor — the site and the LLM both cite it.**
- The reference benchmark driver already exists:
  `runs/oss-eval/sweep.sh` indexes all corpora in parallel, then runs
  embed+analyze strictly serially (embedding saturates all cores), per the
  machine notes in CLAUDE.md. Per-repo config is a checked-in YAML
  (`runs/oss-eval/<proj>/<arm>.yaml`) pinning `source_dir`, `db_file`,
  `report_dir`, enabled languages, the `embedding.*` block (model, provider,
  custom ONNX dir, dims, pooling, max_length), and ported thresholds.
- The DB is bound to one model identity and is incremental (skips unchanged
  files by mtime/size then content hash). A DB is one-model-per-arm; a cheap
  metadata refresh is `DELETE FROM files; DELETE FROM code_units;` then
  re-index (embeddings survive, keyed by model+body-hash).
- Corpora already live as shallow clones under
  `runs/oss-eval/corpora/` (flask/express/gin/ripgrep/redis) — the first
  case-study batch is already on disk.
- Site tooling: **MkDocs Material** builds a browsable static site from a
  markdown tree with built-in client-side (lunr) search and Pygments code
  highlighting, deploys to GitHub Pages via a stock GitHub Actions workflow
  (`mkdocs gh-deploy` / `actions/deploy-pages`), and — critically for a
  *generated* tree of hundreds of pages — the `awesome-nav` plugin builds the
  nav from the directory structure plus per-directory `.pages` files, so we
  never hand-maintain a page list. Confirmed current (2026).
- LLM layer: default model `claude-opus-4-8` (1M context, $5/$25 per MTok).
  The **Message Batches API** runs the same Messages requests asynchronously
  at **50% price**, results in ≤24h (usually ≤1h), keyed by `custom_id` —
  the natural fit for "write N reports overnight." Prompt caching gives
  ~90%-off reads on a shared prefix (the teaching/system prompt), which every
  repo report reuses.

## Decision

**A single local batch driver on the 16-core box produces a versioned
`site/` markdown tree; an LLM report step (Claude Batches API, Opus 4.8)
writes one grounded narrative per repo into that tree; MkDocs Material builds
and GitHub Actions deploys it to a dedicated `gh-pages` branch of this repo.**
Corpora are permissive-license-gated at selection time, so nothing that
reaches the LLM or the site can be a GPL snippet.

Rationale against the alternatives is in each section below.

## 1. Pipeline architecture

### Stages

```
targets.toml ──▶ clone ──▶ gen config ──▶ index ──▶ embed ──▶ analyze ──▶ report tree
   (curated)     (shallow)  (per repo)   (parallel) (serial)  (serial)   (markdown)
                                                                            │
                                          site tree ◀── LLM narrative ◀─────┤
                                             │          (Batches, Opus 4.8)
                                        MkDocs build ──▶ gh-pages ──▶ Pages
```

1. **Target list.** `runs/case-studies/targets.toml` — one entry per repo:
   `{ id, url, ref (pinned commit SHA), language, license (SPDX), arm }`.
   The pinned SHA is what makes a run reproducible; `license` must be on the
   permissive allowlist (§4).
2. **Shallow clone** to `runs/case-studies/corpora/<id>` at the pinned SHA
   (reuse the existing oss-eval corpora directory convention). `--depth 1`.
3. **Generate `decombine.yaml` per repo** from a template + the target entry:
   the template is the arm's `embedding.*` block (pinned model, provider,
   custom ONNX path, dims, thresholds) with only `source_dir`, `db_file`,
   `report_dir`, and `languages.enabled` filled per repo. One template per
   model arm; do **not** hand-write per-repo configs — they must be
   byte-identical except for the three path fields and the language list, or
   the arms aren't comparable.
4. **Index (parallelized).** Fan out `decombine index` across all repos —
   parser-bound, independent DBs, safe to run at core count (mirrors
   `sweep.sh` phase 1).
5. **Embed then analyze (serialized).** For each repo in size order (small
   first, so config mistakes fail cheap): `decombine embed` then
   `decombine analyze`. Embedding must be strictly serial — it already
   saturates all cores — so this is the wall-clock-dominant phase. Budget it
   with `embedding.max_batch_token_area` at the default 16M (smaller is both
   faster and lower-RSS on CPU per CLAUDE.md; do not raise it).
6. **Collect** each repo's `report_dir` into the staging site tree (§2 IA).
7. **LLM narrative** (§3) reads the collected markdown + a small stats JSON
   and writes `report.md` next to each repo's `index.md`.
8. **Build + deploy** (§2, §4).

Steps 2–6 are a thin Python driver beside `sweep.sh`
(`runs/case-studies/build.py`) reusing `run_phase.py` for wall/RSS capture.
Do not fold this into `sweep.sh` — that is the benchmark harness and must stay
comparable across runs; the case-study driver is allowed to churn.

### What runs where

Everything through step 6 runs **locally on the 16-core box** — embedding is
CPU-saturating and model weights + ONNX runtime are already cached there;
doing it in CI would mean re-downloading models and running embedding on a
2-core runner. Steps 7–8 (LLM calls + MkDocs build) are network/IO-bound and
can run locally or in CI interchangeably; the LLM step is the only one that
needs an API key.

### Reproducibility (pin everything)

A run is defined by three pinned inputs, all recorded *in the published
output*:

- **Corpora**: the commit SHA per target in `targets.toml`.
- **Model + config**: decombine already stamps the full model identity and
  thresholds into every report header — surface that verbatim on the site
  (§2) rather than re-deriving it.
- **decombine version**: capture `decombine --version` + `git rev-parse HEAD`
  of this repo into a `run.json` written at the top of the site tree and
  linked from the landing page.

Because the report header already carries model identity and the DB enforces
one-model-per-identity, "which model produced this" is never ambiguous — the
site just echoes it.

### Storage / versioning

**Generated artifacts do not go on `master`.** Publish to a dedicated
**`gh-pages` branch** of this same repo, written only by the deploy workflow
(`actions/deploy-pages` from a built `site/`). Rationale:

- A **separate repo** adds a second thing to clone, permission, and keep in
  sync with decombine's version — rejected; the site is a function of this
  repo's code.
- A **`docs/` dir on `master`** would pollute the source tree with hundreds
  of generated markdown files and a full site rebuild on every content run,
  churning PR diffs and code review — rejected.
- `gh-pages` isolates generated content from source, is the GitHub Pages
  default, and lets us keep the *source* of the site (templates, driver,
  `targets.toml`, LLM prompt) under `runs/case-studies/` on `master` while
  the *built output* lives only on `gh-pages`.

The intermediate markdown tree (pre-build) is a build artifact; keep the last
run under `runs/case-studies/site/` gitignored, and rely on `run.json` +
pinned SHAs for reproducibility rather than committing the tree.

## 2. Static site: MkDocs Material

**Recommendation: MkDocs Material + `mkdocs-awesome-nav`, deployed to GitHub
Pages via GitHub Actions.**

### Comparison

| Option | Verdict |
| --- | --- |
| **Plain GitHub Pages (Jekyll)** | Weak code highlighting, no real search, painful with a deep generated tree; the default only because it's zero-setup. Rejected. |
| **mdBook** | Rust-native (matches decombine), single binary, great highlighting — the runner-up. But `SUMMARY.md` must list *every* page explicitly; for a generated tree of hundreds of cluster pages that's extra glue, and its search is weaker across many small pages. It's built for one linear book, not a matrix of repos. |
| **Docusaurus / Astro Starlight** | Best-looking, but Node + a heavy JS build step, MDX conventions, and per-page frontmatter — overkill for rendering an already-markdown tree, and violates the "no heavy JS build" constraint. Rejected. |
| **MkDocs Material** | **Pick.** pip-installable (matches the repo's Python tooling — `scripts/embedding_experiments.py`, `run_phase.py`), one `mkdocs.yml`, built-in client-side lunr search that scales to many pages, Pygments highlighting for all our languages, and `awesome-nav` builds the nav from the *directory structure* + per-dir `.pages` files — so a generated tree needs zero hand-maintained page list. First-class `gh-deploy` + Actions path. |

The deciding factor is the generated, growing tree: MkDocs Material is the
only markdown-native, no-JS-build option that produces the nav automatically
from the filesystem. mdBook loses only on that axis; if a future constraint
demands a Rust-only toolchain, it is the fallback.

### Information architecture

```
Landing (index.md)                     ── what decombine is, how to read a report,
  │                                       run.json (decombine version, model arm,
  │                                       date), the corpora license policy
  ├── How to read these reports          ── the teaching page (scores, sections,
  │                                          Mixed = impl↔test FP shape, idioms)
  └── <language>/                         ── one section per language (~12)
        └── <repo>/
              ├── report.md   ← LLM narrative (the page users land on)
              ├── index.md    ← decombine's raw duplicate index (verbatim)
              ├── cluster-01.md … cluster-NN.md   (verbatim)
              └── compare/…   (only for repos run through compare)
```

Each repo directory gets a generated `.pages` file ordering `report.md` first
(the narrative is the entry point), then the raw `index.md`, then clusters.
The per-repo `report.md` links into the raw pages by relative path so the
reader can jump from a claim to the evidence. The verbatim decombine output
is published unchanged so the narrative is auditable against it.

## 3. The LLM report layer

This is the deliverable users actually read. One `report.md` per repo, written
by Claude from that repo's decombine output.

### Model and cost

- **Model: `claude-opus-4-8`.** Report quality is the whole point; this is the
  quality tier and its 1M context comfortably holds a large report tree.
- **Transport: the Message Batches API** (`client.messages.batches.create`),
  one `custom_id` per repo. 50% cheaper, results keyed by `custom_id` (match
  by ID, never by order), and a natural "generate all reports for the run in
  one job" shape. Poll `processing_status` until `ended`, then stream results
  and write each `report.md`.
- **Prompt caching**: the teaching/system prompt (how to interpret decombine
  output) is identical across every repo — put it first with a
  `cache_control` breakpoint so all-but-the-first request in the batch read it
  at ~0.1× (the repo-specific report tree goes after the breakpoint).
- Order of magnitude: a repo's fed input is bounded to well under ~50–100K
  tokens by the selection budget below; dozens of repos × Opus batch pricing
  is dollars, not tens of dollars. Streaming isn't needed (batch is async);
  set a generous `max_tokens` (~8–16K) for the report.

### Input budget and selection

Report trees can be large (a big repo yields many cluster files). Do **not**
feed the whole tree. Feed a bounded, deterministic selection:

1. **Always**: the full `index.md` (it's the sectioned summary + the
   cross-directory table + the model-identity header) and a small `stats.json`
   the driver emits (candidate pairs, cluster counts per section, language
   histogram, top raw/boosted scores).
2. **Selected clusters**: the top *k* Product clusters by boosted score
   (k≈5), plus up to 2 Mixed clusters (the impl↔test false-positive shape,
   which the narrative must teach), plus 1 Test-and-docs cluster as a
   contrast. Cap total fed cluster pages at ~10 and total input at a fixed
   token budget; if over, drop lowest-scored Product clusters first. Selection
   is deterministic (sorted by section then boosted score) so reruns are
   stable.
3. Never feed raw repo source beyond what's already embedded in the cluster
   pages (decombine already put the illustrative excerpts there).

### Grounding (must cite, must not invent)

- The prompt instructs: **every finding must cite a `path:line` and a cluster
  hash that appears in the provided pages**; if it isn't in the input, it
  doesn't go in the report. The verbatim pages are published alongside, so
  claims are checkable.
- Feed the model-identity header and instruct it to state the model, provider,
  and thresholds used — so a reader knows these scores are backend-relative
  (raw cosine thresholds don't port across models, per CLAUDE.md).
- Require an explicit **"where decombine is wrong"** section: pick at least one
  Mixed or same-name-family cluster and explain why it's a benign idiom
  (trait/interface family, generic helper) rather than refactorable
  duplication — the report is dishonest without it.

### Output structure (fixed template)

The prompt pins the sections so pages are uniform across repos:

1. **What this is** — repo, language, pinned SHA, model arm, one-line result.
2. **How decombine read this repo** — plain-language pass over the index:
   candidate pairs → clusters → the three sections; what "top raw" vs
   "boosted" mean; what Cross-directory dispersion/entropy signal.
3. **Findings (2–3)** — each: the cluster, its score, the cited `path:line`
   members, and *why it is real duplication or a benign idiom*.
4. **False positives / limits** — the honest section above.
5. **Try it yourself** — the exact `decombine.yaml` fragment and the three
   commands (index/embed/analyze) that reproduce this page, plus a link to the
   verbatim `index.md` and clusters.

Use the Claude API skill's structured-output / template discipline; a plain
Messages call with the fixed template in the system prompt is sufficient — no
tools or agents needed (this is single-shot summarization over provided text).

## 4. Automation, cadence, guardrails

### Cadence

- **Primary: a one-shot local script** (`runs/case-studies/build.py`) run
  by hand (or by `/loop`/`/schedule` if we want it periodic). It does
  clone→config→index→embed→analyze→collect locally, then the LLM batch, then
  hands `site/` to the build. This is right because the expensive phase
  (embedding) belongs on the 16-core box, not a CI runner.
- **CI (GitHub Actions) does only build + deploy**: on push to `gh-pages`
  source inputs, or on a committed pre-built `site/` artifact, run
  `mkdocs build` and `actions/deploy-pages`. Keep embedding out of CI.
- **`/schedule`** is appropriate only once the corpus is stable and we want a
  monthly "rebuild with the latest decombine" — it would trigger the local
  driver, not CI.

### Adding a repo

Append one entry to `targets.toml` (url, pinned SHA, language, SPDX license,
arm) and rerun the driver. Because configs are generated from the arm
template, there is nothing else to write. New language ⇒ ensure
`assets/languages/<id>.toml` + `<id>/units.scm` exist first (per CLAUDE.md
code map); the driver should fail loudly if a target's language isn't
enabled/available.

### Regenerating when decombine changes

Extraction changes (naming, queries, adapters) do **not** refresh existing
units — incremental indexing skips unchanged files. The driver must, on a
decombine version bump, do the cheap refresh per CLAUDE.md
(`sqlite3 db "DELETE FROM files; DELETE FROM code_units;"` then re-index);
embeddings survive (keyed by model+body-hash) so only extraction re-runs. Bump
the recorded `decombine --version` in `run.json`. If the *model* changes, it's
a new arm and a new DB — never reuse a DB across model identities.

### Guardrails — no GPL snippets

The site republishes real source excerpts (cluster pages embed function
bodies), so license discipline is load-bearing, not cosmetic:

- **Permissive allowlist enforced at selection**: `targets.toml` entries carry
  an SPDX id; the driver refuses any repo whose license isn't on the allowlist
  (MIT / Apache-2.0 / BSD-2/3 / ISC / MPL-2.0-with-care). This is the single
  choke point — if a repo can't clear it, its source never reaches an embed,
  a report, or the site.
- Never pull corpora from `_upstream/` or any AGPL/GPL source (Slopo is
  research-only, never published).
- The LLM prompt is told the excerpts are permissively licensed and to
  attribute each repo (name, license, SHA) — the "Try it yourself" footer
  already carries provenance.
- Belt-and-suspenders: the build step re-reads each target's declared license
  and fails the deploy if any published repo dir lacks an allowlisted SPDX id.

## 5. Rollout order

1. **First batch = the existing oss-eval repos** (flask/express/gin/
   ripgrep/redis) — already cloned under `runs/oss-eval/corpora/`, already
   have per-arm configs and fresh reports from the sweep. Pin their SHAs into
   `targets.toml`, point the collector at their `report-coderank/` dirs. Zero
   new indexing needed to prove the *site + LLM* half of the pipeline.
2. **Stand up the site** on those 5: `mkdocs.yml` + `awesome-nav`, the IA
   above, verbatim reports only (no LLM yet), deploy to `gh-pages`. Confirm
   search, highlighting, and nav-from-tree on a real multi-language tree.
3. **Add the LLM layer** on the same 5: teaching page + the fixed report
   template, one Batches job, write `report.md` per repo, eyeball grounding
   (every cited `path:line` must exist in the verbatim page). Iterate the
   prompt here where it's cheap.
4. **Generalize the driver**: `targets.toml` + config-template generation +
   `build.py`, so adding a repo is a one-line change. Re-run steps 1–3 through
   the driver end-to-end.
5. **Scale the corpus** toward ~12 languages, one permissively-licensed repo
   per language first (breadth before depth), small repos first so config
   mistakes fail cheap. Enforce the license allowlist from the first added
   repo, not retroactively. Use
   [case-study-corpus-selection.md](case-study-corpus-selection.md) as the
   current target matrix; in particular, prefer Valkey over current Redis for
   the public C slot unless a Redis commit is manually cleared.
6. **Cadence**: once stable, wire a monthly `/schedule` rebuild against the
   current decombine version, with the cheap metadata-refresh path on version
   bumps.

## Open items

- Decide the exact SPDX allowlist and whether MPL-2.0 file-level copyleft is
  acceptable for embedded excerpts (likely yes; confirm before including MPL
  repos).
- Resolve the Redis publication question: current GitHub metadata did not
  return a permissive SPDX license, so Redis-derived reports should remain
  internal unless pinned to a manually cleared commit. Valkey is the preferred
  public C target.
- Whether to also publish `compare` case studies (project-to-project) or keep
  the first cut duplicate-only — compare adds a second report shape and a
  second narrative template; defer past step 5.
- Release coupling: the site advertises decombine; gate the first *public*
  deploy on the license decision in
  [release-architecture.md](release-architecture.md) (this repo is
  `publish = false` today).
