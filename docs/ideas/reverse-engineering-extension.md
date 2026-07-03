# Reverse-Engineering Extension: Ghidra-Decompiled Code for Malware & Functionality ID

Date: 2026-07-02

Status: research synthesis. No implementation has started. Builds on
`docs/ideas/linear-algebra-research-synthesis.md` (the prior LA analysis) and
`architecture.md` (the current decombine architecture, which has evolved to
include `analyze/concerns/`, `analyze/compare/`, and `analyze/experimental/`).

Source: structured deepthink analysis with four parallel sub-agents
(decompiler realism, pipeline reuse, adversarial honesty, model + curriculum),
five refinement iterations, and verification against `architecture.md`.

## The headline

decombine's architecture ALREADY contains the two analyzers this needs. The
`concerns` pipeline IS functionality identification. The `compare` pipeline IS
reference-set malware/family matching. The RE extension is a **new input adapter
+ new honest framing**, not a new engine — ~90% of the code is reused, and the
new ~10% lives at the edges (Ghidra ingestion adapter, RE reporting transform,
query-pack data, small schema additions). The deepest distinction from source-
clone detection is the **adversarial threat model**: the author is actively
evading you, which reshapes every confidence claim.

## 1. Input layer — Ghidra ingestion and the make-or-break normalizer

### What Ghidra emits

Ghidra headless mode (`analyzeHeadless`) does NOT emit decompiled C itself; a
~30-line postScript using `ghidra.app.decompiler.DecompInterface` calls
`decompileFunction(func, timeout, monitor)` → `DecompileResults.getDecompiled
Function().getC()` for each function. Per-function metadata comes from the
`Function` object: name, entry address, body size (`getBody()`), signature,
calling convention, parameters, `isThunk`, `isExternal`, symbol `SourceType`
(DEFAULT = auto-name like `FUN_001020a0` vs IMPORTED/USER_DEFINED). There is no
built-in decompile-to-JSON exporter; every RE pipeline writes a postScript.
`DecompInterface.setSimplificationStyle("normalize")` is an alternate output
style worth knowing.

**v1 integration: decoupled.** The user runs Ghidra separately, exports
JSON/CSV via a postScript, and decombine ingests the export. This matches
decombine's "no external runtime" ethos and keeps `cargo install` clean. An
opt-in later mode could have decombine invoke `analyzeHeadless` directly, but
that adds a Java/Ghidra dependency — defer.

### The make-or-break normalizer

Ghidra auto-names (`FUN_001020a0`, `uVar12`, `param_1`, `DAT_…`, `LAB_…`) are
~60–80% of tokens and VARY across decompilations (different base address →
different `FUN_` suffix; different register allocation → different `uVarN`
numbering). Unnormalized embeddings "embed the noise" and cross-sample
similarity collapses — far worse than source, where meaningful identifiers
carry the signal.

This is NOT the source normalizer's job (it is identifier-preserving and
operates on `display_source`). It needs a new `ReNormalizer` selected by
`language_id == "ghidra-c"` that rewrites `embedding_text`/`normalized_body`
while keeping `display_source` verbatim. The ReNormalizer **MUST be
deterministic** (same input → same output) for `normalized_body_hash` to be
stable across runs — dropping ordinals entirely (`var` not `var_0`) is the
deterministic choice.

Normalization rules:
- `FUN_…`/`DAT_…`/`LAB_…` → `fn`/`dat`/`lab`; `param_\d+` → `arg`;
  `local_*`/`uVar\d+`/`lVar\d+`/`in_/out_/unaff_/aff_` → `var` (drop ordinals).
- Strip casts; `undefined`/`undefined4/8` are placeholders carrying no signal.
- **Tiered literal handling** (config flag):
  - `fingerprint mode` (for malware-ID): keep small/symbolic constants and
    known magic values (0x63, 0x01, syscall numbers, crypto round constants
    ARE fingerprints); normalize hex addresses and large ints to `<num>`,
    strings to `<str>`.
  - `strip mode` (for cross-compiler functionality-ID): all literals →
    `<num>`/`<str>` (constants are compiler-dependent noise).
- Inlined intrinsics (`__stack_chk_fail`, `__guard_check_icall`, `memset`
  call-fixups): keep for malware-ID (fingerprint); canonicalize to
  `<intrinsic>` for cross-compiler functionality-ID.

**Over-normalization trade-off (must state honestly):** aggressive renaming
trades false negatives (evasion) for false positives (trivial-function
collisions — two unrelated small utilities become near-identical text after
renaming). This is a tunable trade-off, not a free win. Mitigate with a higher
complexity gate (see below).

### Tree-sitter: unnecessary for unit boundaries

A C grammar does NOT cleanly parse Ghidra pseudocode (`undefined*` types,
`unaff_` registers, `switchD_` labels, cast-everywhere). Key simplification:
Tree-sitter is unnecessary for unit BOUNDARIES — Ghidra hands you one function
per `DecompileResults`; the importer emits one `CodeUnit` per function from
script iteration. For body-AST/normalization, use a tolerant C-grammar adapter
OR skip body-AST entirely.

### Complexity gate: use function_size, not body_node_count

`body_node_count` (default threshold 10) is unreliable on Ghidra pseudocode
(the C grammar error-recovers heavily). For RE, use `function_size` (bytes from
`getBody()`) or decompiled-text line count as the complexity gate. Trivial
functions (thunks, small wrappers) must be dropped to avoid over-normalization
false positives.

### Importer filtering: thunks and externals

- **Externals** (`isExternal=true`): no decompiled body — skip entirely.
- **Thunks** (`isThunk=true`): 2-line jump trampolines — either skip OR index
  with `kind="thunk"` and exclude from default analysis (every thunk looks like
  every other thunk and pollutes similarity search).

### New schema (minimal, source units unaffected)

```sql
binaries(
  id INTEGER PRIMARY KEY,
  project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  binary_hash TEXT NOT NULL,          -- sample sha256, NOT source_hash
  architecture TEXT NOT NULL,         -- x86/arm/etc
  sample_source TEXT,                 -- how the binary was obtained
  decompiler_version TEXT NOT NULL,   -- critical for version drift
  decompilation_coverage REAL NOT NULL, -- 0..1, <0.3 flags packing
  imported_at TEXT NOT NULL,
  UNIQUE (project_id, binary_hash)
)
unit_tags(                            -- NEW: no existing tag mechanism
  unit_id INTEGER NOT NULL REFERENCES code_units(id) ON DELETE CASCADE,
  key TEXT NOT NULL,
  value TEXT NOT NULL,
  PRIMARY KEY (unit_id, key)
)
```

Extend `code_units` with NULLABLE RE fields (nullable keeps source units
unaffected): `function_address` (TEXT hex), `function_size` (INT bytes),
`calling_convention` (TEXT), `is_thunk` (INT bool), `is_external` (INT bool),
`binary_id` (FK). `decompilation_coverage` lives on `binaries`.

The existing `analysis_runs`/`analysis_artifacts` schema already supports RE
provenance with no change: `analysis_kind="re-compare"`,
`project_scope_json` captures reference-DB + sample project ids,
`config_json` captures thresholds, `metrics_json` captures coverage, `blob`
holds the report.

### Decompiler version drift

Same binary, Ghidra 10 vs 11 → different pseudocode (temp-var numbering, cast
insertion, sometimes CFG). A reference DB built on one version silently degrades
against samples decompiled with another — the WORST failure mode (false
negatives). The tool must:
1. persist `decompiler_version` per binary AND per code_unit;
2. refuse or loudly warn when query-sample version != reference-set majority;
3. restrict `normalized_body_hash` exact-match to same-binary/same-version —
   cross-version goes only through embedding similarity, never the hash path.

`exact_copy` match class is near-inert for cross-binary RE (different
compiler/flags → different body text even after normalization); useful only for
same-binary re-analysis and version-drift detection. The embedding-similarity
path is the cross-binary workhorse.

## 2. Functionality ID — the concerns pipeline, reused as-is

decombine's `analyze/concerns/` ALREADY embeds arbitrary query strings and
scores every unit by `dot(concern, unit)`, keeping top units above
`min_projection` with structural-spread reporting. "Find functions implementing
AES key schedule" is LITERALLY this pipeline with RE-ingested units and
RE-specific query strings. Zero analyzer changes.

**Two query forms (model-dependent viability):**
- (a) Natural-language: `analysis.concerns.queries = ["AES key schedule
  expansion routine", "round-key derivation"]`. Viable with general-text
  models (BGE); may be weak with code-specific models (Jina-code, trained on
  code not NL).
- (b) Reference-code: embed a known AES key-schedule source function, use its
  vector as the concern. v1: paste the source as a query string (already
  supported). Small extension: `Query::FromUnit { project, unit_id }` config
  variant that pulls a precomputed vector from `AnalysisContext` — a Config
  enum change, not an analyzer change. Preferred for code-specific models.

**Reporting tweaks (post-processing in the RE shim, not analyzer changes):**
group hits by binary (multi-binary sample set) instead of directory; group by
address range (functions in an address window likely form one module); bucket
by function size (tiny matched stubs are weak evidence; large matches are
strong); emit address/binary/size fields. The "candidate concerns" caveat
already fits RE (embeddings are evidence, not proof).

**Threshold retuning:** `min_projection` (default 0.45, tuned for source) needs
empirical RE retuning under distribution shift — the shim provides RE defaults,
but actual numbers come from the benchmark, not inherited from source.

## 3. Malware / family ID — the compare pipeline, reused as-is

decombine's `analyze/compare/` ALREADY does asymmetric bipartite matching
(`left` reference, `right` candidate) with split/merge classification and match
classes. Set `left` = curated malware-family reference DB (units tagged with
family label via `unit_tags`); `right` = sample. The existing match classes map
cleanly to RE verdicts:

| compare class | RE verdict |
| --- | --- |
| `exact_copy` / `strong_match` | known family member |
| `possible_match` | candidate, review |
| `new_in_right` | novel / no known-family match |
| `missing_in_right` | expected capability absent (stripped/removed) |
| `split` (one left → many right) | reference function inlined/expanded |
| `merge` (many left → one right) | sample function recombines reference funcs |
| many-to-many component | variant recombination |

The analyzer, scoring, bipartite components, and match-class taxonomy are reused
verbatim. Coverage aggregation groups by family label (via `unit_tags`) and by
binary.

**Multi-family candidate reporting:** a sample function near two reference
families should report both, not force one. The compare pipeline's top-k output
already keeps multiple candidates per unit; the honest RE verdict is "candidate
families: A (0.84), B (0.81)" not a single forced label.

**Compiler-inlining confound:** a compiler may inline `helper_A` into
`caller_B`, producing a decompiled `caller_B` whose body contains `helper_A`'s
logic. The compare pipeline classifies this as `merge` — but it's a compiler
artifact, not a semantic recombination by the malware author. Mitigation: flag
inlined functions (Ghidra's call-fixup / `isThunk` marks) and treat as a
distinct verdict subclass ("compiler-inlined merge") vs author-intent
recombination.

**Threshold retuning:** `match_threshold` (0.86) and `candidate_threshold`
(0.78), tuned for source cross-project, need empirical RE retuning — the shim
provides RE defaults; actual numbers come from the benchmark.

### Shared-library exclusion (the highest-volume false positive)

OpenSSL `EVP_DecryptInit`, zlib `inflate`, libpng chunk parsers appear in
malware AND every benign app. Embedding hits them hard — they're literally the
same code. Without exclusion the tool cries wolf on every sample that links a
crypto library.

Implementation: run `compare` twice in the shim (sample vs malware-DB, sample vs
shared-libs-project), then subtract — matches in shared-libs are labeled
"shared library: zlib inflate" and suppressed from malware verdicts. No analyzer
change; orchestration in the shim. The shared-libs project is a curated
reference project (label "shared-libs", role "exclusion") indexed with decompiled
functions from common libraries.

## 4. The adversarial limits — the deepest distinction

### The central distinction

- **Achievable:** "This decompiled function is N% cosine-similar to function Y
  in reference family X." This is similarity search — true by construction.
- **NOT achievable:** "This code is malicious." Malice is intent + behavior-in-
  context, neither of which is a text property. An embedding has no model of
  data flow, no model of the victim, no model of author intent. Identical
  string-decryption loops appear in DRM, packers, and ransomware.

Embeddings **triage** — they surface candidates a human then investigates. They
do not **classify** malice. Anyone shipping "AI malware detector" based on
cosine alone is selling a geometric fantasy.

### Adversarial transformations and concrete failure modes

| Transform | Failure mode | Tool response |
| --- | --- | --- |
| **Packing** | Stub decompiles to ~5 lines; real payload decompressed at runtime, never seen statically. Embedding matches the PACKER, not the family. | Report `decompilation_coverage ≈ 0`; flag `packed — static analysis defeated`; REFUSE to score the payload. Silently returning "no match found" reads as "clean" — the worst outcome. |
| **Polymorphism** | Renamed locals, reordered blocks, junk predicates; cosine to a reference drops ~0.9 → ~0.5–0.7. | False negatives at the decision boundary; tunable but every threshold is a gamble. |
| **Metamorphism** | Instruction substitution, register reassignment, body rewrite; semantically identical functions can land at cosine < 0.3. | Embeddings are WEAK against metamorphism; structural/CFG features carry the signal embeddings lose. State this honestly. |
| **Stripping** | Largely MOOT post-decompilation — Ghidra auto-names everything regardless. | Don't list as a real risk. |
| **Ghidra version drift** | Same binary, different release → cosine drift ~0.05–0.15; reference DB silently rots. | Pin and document the version; treat reference DB as version-locked, not eternal. (Cosine-drop ranges are qualitative estimates, not measured — MEDIUM confidence.) |
| **Shared-library FPs** | OpenSSL/zlib/libpng code in malware AND benign apps; highest-volume noise. | Maintain shared-libs exclusion project; report "shared library: X" not "malware." |

### Confidence / provenance story — every hit ships with provenance, or it ships nothing

- `decompilation_coverage` — fraction of functions decompiled cleanly. Low ⇒
  packed; the score is about the stub, not the sample.
- `reference_set_coverage` — how much of the DB was actually searched.
- **Score-as-similarity, never-as-verdict:** "82% similar to family-X function
  `decrypt_strings_0`" — never "82% malicious." The percentage is a ranking
  signal, not a probability of malice.
- **Pairing with structural features:** call-graph shape, immediates/constants,
  string tables. Embeddings are one channel; orthogonal channels catch what
  embeddings miss.
- **Pairing with YARA / byte features:** bytes and embeddings fail differently;
  cross-corroboration is the only honest strong claim.
- **Human-in-the-loop:** the tool triages, the analyst decides. No autonomous
  verdict. Ever.

### Competitive positioning (complementary, not replacement)

decombine-re adds the **semantic similarity** channel to an RE toolkit that
already has:
- **YARA** — byte/pattern rules (catches exact patterns; can't do "find AES-
  like").
- **Function-hash matching** — imphash, iddhash, fuzzyHash (catches exact
  matches; misses variants that hash-diff but are semantically near).
- **BinDiff / Diaphora** — CFG-isomorphism function matching (catches
  structural similarity; heavier, exact-structure-dependent).

decombine-re's distinct value: semantic similarity without exact pattern
matches (catches variants); NL/reference-code queries that YARA can't do;
gradated ranking (not binary match/no-match). The honest pitch is "add semantic
triage to your existing RE toolkit," not "replace YARA/BinDiff."

### Honest product framing (README ceiling)

> **decombine-re** surfaces decompiled functions that are cosine-similar to a
> curated reference set. It is a triage assistant for reverse engineers, **not
> a malware classifier.** It does not assess intent, behavior, or harm. Packed,
> polymorphic, or metamorphic samples may evade it entirely; shared-library
> code may produce matches that are not malicious. Every result is a similarity
> score with provenance, to be reviewed by an analyst alongside structural,
> byte-level (YARA), and behavioral signals. **The analyst renders the
> verdict; the tool only prioritizes.**

### Responsible deployment

The reference DB is user-curated and stored locally; decombine does not
redistribute it. Curating and distributing decompiled known-malware code is the
user's responsibility under applicable law. The tool is dual-use technology for
legitimate RE (malware triage, vulnerability research, CTF, incident response);
overclaiming "AI malware detector" is both technically false and ethically costly
(false positives cause wrongful takedowns, false negatives cause misplaced
confidence).

## 5. Architecture fit — thin shim, not a sister tool

The minimal choice is `analyze/re/` thin shim: construct `AnalysisContext` with
RE defaults (concern query packs, thresholds tuned for decompiler noise),
invoke the existing `Analyzer::run` for concerns and compare, then post-process
each analyzer's `Output` into RE verdict labels / binary-and-address grouping /
family coverage. Zero changes to the `Analyzer` trait or the three analyzers. RE
specifics stay isolated; generic analyzers stay input-agnostic. A separate
`decombine-re` binary is premature — no analyzer logic differs. Revisit IF
RE-specific metadata/normalization pressure grows to the point of poisoning the
source-focused tool.

**One real exception to "no analyzer changes":** the concerns pipeline's
`spread.rs` computes structural spread using directory entropy and path distance.
Decompiled functions have NO directories or paths — they have addresses in a
binary. Two options:
- (a) the spread module gains a "grouping key" abstraction (directory for
  source, address-range/binary for RE) — a small analyzer extension;
- (b) the RE shim disables the analyzer's spread and computes address/binary
  spread post-hoc from the raw output — preserves "no analyzer changes" but
  discards the analyzer's spread for RE.

Recommend (b) for v1.

### CLI surface

| Command | Status |
| --- | --- |
| `decombine re index <ghidra-export>` | NEW ingestion (export → units → vectors). Reuses embed/vector pipeline; new only the export-format adapter. |
| `decombine re concerns --query "AES"` | CONFIG-REUSE of `decombine concerns` + RE defaults/query pack. New `--re` flag. |
| `decombine re concerns --reference-function <file>` | MINOR extension: `Query::FromUnit` config variant. |
| `decombine re compare --reference-db <db> --sample <s>` | CONFIG-REUSE of `decombine compare` with `left=reference-db`, `right=sample`, RE verdict labels + family coverage. |
| `decombine re family-db add/list` | NEW small admin — project + family-tag management. |

### Minimal new code (~10% of the engine, all at the edges)

1. **Ghidra/IDA export → units adapter** (ingestion only, analyzer-agnostic).
   The largest genuinely-new piece. Includes the `ReNormalizer`.
2. **`analyze/re/` shim** — defaults construction + a reporting transform over
   existing `Output` structs (verdict labels, binary/address/size grouping,
   family coverage, shared-libs exclusion orchestration, address/binary spread
   post-hoc). No analyzer edits.
3. **`Query::FromUnit` config variant** (optional; skip in v1 by pasting
   reference source as a query string).
4. **`unit_tags` table + family-label management** — small schema addition.
5. **`binaries` table + nullable RE fields on `code_units`** — small schema
   addition.
6. **RE query pack** — data, not code.

## 6. Embedding model — cheap first, research gated

### The transfer question (open, empirical, expect degradation)

Source-trained models (BGE/Jina-code, trained on GitHub source) face a
distribution shift on decompiled pseudocode: no meaningful identifiers (the
strongest signal in source embeddings), no comments, goto-heavy unstructured
flow, imprecise types, version-dependent text. Expect measurable degradation,
especially for BGE/Jina-code which lean on identifiers. This is empirical —
benchmark, don't assume.

### Cheap v1: text embeddings on normalized pseudocode through fastembed

Right v1 because it reuses decombine's whole stack unchanged and gives a
baseline to benchmark against. Any CFG-aware encoder has to beat it to justify
the added complexity.

### Research path (gated on benchmarks)

CFG-aware binary-function encoders outperform text embeddings on binary
similarity but none ship as fastembed-ready ONNX — they require training and
ONNX export. The architecture's `custom_onnx` backend is the extension point.

| Encoder | Approach | Notes |
| --- | --- | --- |
| Gemini (Yin et al.) | structure2vec over control-flow graph | GNN learns node states via message passing |
| SAFE | self-attentive function embedding | instruction-token transformer, no explicit graph |
| OrderMatters | preserves instruction order | recurrent/positional structure |
| Asm2Vec | doc2vec on disassembly instruction tokens | analogy to doc2vec on source |

### Two-signal design (pseudocode + assembly)

Index BOTH pseudocode AND assembly as two orthogonal signals through the same
pipelines (concerns/compare). Costs 2× index storage + 2× embedding compute —
cheap. Pseudocode catches high-level logic (loops, conditionals, API call
sequences) even when instructions are obfuscated; assembly catches instruction-
level patterns (opcode histograms, register usage, immediates) even when
decompilation is lossy.

**A function similar in BOTH spaces is a strong, low-FP match; similar in ONLY
ONE is a FLAG** — either an evasion attempt (obfuscation that breaks one
signal) or a decompiler artifact. The disagreement signal is itself a feature.

**Reconciliation is a scoring rule, not vector arithmetic:** the two embedding
spaces have different dimensions (384d pseudocode + ~300d Asm2Vec) and no shared
coordinate system. The analyzers run independently per signal (each in its own
space); the shim applies a joint scoring rule over two scalar scores per pair
(strong if both > threshold; flag if disagree; weak if both <). No vector
arithmetic crosses spaces.

### Reference-DB scale and the ANN trigger

Exact cross-project matching scales to ~100k reference × ~5k sample (5×10⁸ dot
products, feasible with `ndarray`/`rayon`). Beyond that, ANN (HNSW) is
triggered. **RE is a stronger ANN candidate than source-clone detection**
because the reference DB is a fixed, reusable index queried per sample (classic
ANN use case: fixed index, many queries). The `SimilarityIndex` trait
(`ExactFlat` / `AnnHnsw`) from `architecture.md` applies directly to the
compare pipeline's cross-project matching.

### Benchmark that settles transfer

1. **Duplicate-function pairs across decompiler versions** (Ghidra 10/11/12,
   Hex-Rays) — within-function cross-version pairs must retrieve each other
   (measures version robustness, the deepest distribution-shift axis).
2. **Known-family malware set** (e.g. Mirai/Emotet/TrickBot variants) —
   functions must cluster with their family (measures functionality-ID recall).
3. **Shared-library set** (libc/openssl/zlib multi-arch multi-compiler) —
   cross-arch same-function pairs are true positives; different functions from
   the same library are the FP stress test (they share boilerplate — the exact
   confound that inflates naive cosine).

Report recall@k, MAP, FP rate at fixed recall. Only these numbers settle the
transfer question and the threshold values.

### Reference-DB curation (the primary operational burden)

There is no canonical public decompiled-malware function DB. Sources: VirusTotal
(paid, ToS-limited), MalwareBazaar/Abuse.ch (samples, not decompiled), user's
own analysis history, public RE writeups. Building a good reference DB is a
significant curation effort. The feature is NOT vaporware — a user can start
with a small hand-curated set (their own RE history, known-family samples) and
grow it — but **the DB is the work, not the code.** Value scales with the
reference set, same as any signature-based tool.

## 7. Curriculum map — same LA, harder input, deeper honesty

The prior synthesis's LA tie-ins (`docs/ideas/linear-algebra-research-
synthesis.md`) carry over to RE **unchanged**:

- **cosine = inner product** (lesson `04-dot-products-inner-products-and-norm.md`)
  — same operation on decompiled-code embeddings; the bra `⟨query|` is still
  `fn(Vec) -> Scalar`.
- **projection = query-then-rebuild** (lesson `05-projections-and-measurement.md`)
  — the concerns pipeline embedding a query and scoring by dot product is the
  SAME `⟨query|unit⟩` measurement as in the source-code case.
- **compare = bipartite matching in a shared inner-product space** — same.

The curriculum reinforcement here is **depth of practice, not new concepts** —
the identical LA on a harder, noisier, adversarial input.

### NEW tie-ins the RE extension adds

- **Reference-set malware matching = projecting sample functions onto a
  "known-bad" subspace and reading off loadings** — a lesson-05 measurement-
  basis analogue: the reference DB IS the measurement basis; the loadings ARE
  the amplitudes; the squared loading is a formal echo of Born probability
  (same caveat as the prior synthesis — Pythagoras, not physics; no collapse,
  no phase). The novel beat: the basis vectors are *known-bad functions*
  rather than principal directions, so the measurement is explicitly semantic,
  not statistical.
- **Compare's split/merge classification = a rank/dependence observation**
  (lesson `06-matrices-as-linear-transformations.md`) — a "merge" verdict means
  one reference vector lies in the span of several sample vectors — exactly the
  dimension-collapse chain (`det A = 0 ⇔ columns dependent ⇔ one column is a
  linear combination of the others ⇔ 0 is an eigenvalue`). The compare pipeline
  detects that one direction is a linear combination of others — the
  curriculum's "lossiness gauge" applied to a matching decision.
- **Two-signal (pseudocode + assembly) agreement = two different inner-product
  spaces; agreement is a cross-space consistency check, NOT a single-space
  claim** — lesson `03-basis-changes-and-coordinate-systems.md`'s "many
  coordinate systems, one space" applied twice and honestly reconciled. The
  pedagogy is the LIMIT of any one basis: neither alone is the "true"
  description, and cross-space disagreement is informative.

### Where the analogy breaks for RE specifically

The prior synthesis names: embeddings aren't quantum states (no phase, no Born);
Hermitian/unitary guarantees don't carry wholesale; squared loadings are
Pythagoras not Born; PCA is passive not active; infinite-dimensional cautions
irrelevant. RE adds three NEW breaks:

- **The adversarial setting has NO QM analogue.** There is no adversary trying
  to make your measurement basis fail in quantum mechanics — the observable's
  eigenbasis is fixed by the Hamiltonian. In RE, the "observable" (embedding
  model) is being ACTIVELY EVADED: packers, polymorphic engines, metamorphic
  generators, opaque predicates all exist to defeat the basis. This is the
  deepest disanalogy: the curriculum's measurement story assumes a fixed,
  trustworthy observable; RE measurement is of a hostilically-crafted input
  against a basis the input is shaped to defeat.
- **Decompiler output is a LOSSY, VERSION-DEPENDENT projection of the binary —
  there is no "true vector" being measured, only a noisy recovery.** QM
  measurement has a precise post-measurement state (collapse to an eigenstate);
  RE "measurement" (decompilation) is a lossy preprocessing step with no
  collapse analogue. The prior synthesis's "PCA is a passive change of basis on
  fixed vectors" assumes stable vectors; here the vectors themselves are noisy
  renderings.
- **No new LA is required for RE.** The curriculum reinforcement is that the
  SAME lessons (04 inner product, 05 projection, 06 rank/dependence) apply to a
  harder, noisier, adversarial input. The pedagogy is depth-of-practice, not
  new concepts — and the honesty of the analogy breaks (adversary, lossy
  decompilation, two-space agreement) is itself the lesson: the LA is
  unchanged, the EPISTEMIC STATUS of the measurements degrades, and the
  practitioner must carry that degradation forward into every score. This is
  the prior synthesis's "mathematical soundness ≠ product value" reframed as
  **"geometric identity ≠ epistemic reliability under adversary."**

## Recommended first step

1. **Ghidra export adapter** (`index/ghidra_import.rs`) — parse a Ghidra
   postScript JSON/CSV export; emit one `CodeUnit` per function with RE
   metadata; apply the `ReNormalizer` (deterministic, tiered literal mode);
   skip externals, flag thunks. This is the largest genuinely-new piece.
2. **`analyze/re/` shim** — defaults construction + reporting transform over
   existing concerns/compare `Output` (verdict labels, binary/address/size
   grouping, family coverage via `unit_tags`, shared-libs double-compare
   exclusion, address/binary spread post-hoc).
3. **Schema additions** — `binaries` table, nullable RE fields on `code_units`,
   `unit_tags` table.
4. **RE query pack** — data file of concern queries (NL + reference-code).
5. **Benchmark harness** — the three-set benchmark (cross-version dup pairs,
   known-family malware, shared-library FP stress) to settle model transfer and
   threshold tuning.

Drop `Query::FromUnit` from v1 (paste reference source as a query string
instead). Drop two-signal from v1 (start with pseudocode only; add assembly as
the second signal once the baseline is benchmarked).

## Honest limitations and confidence

**HIGH confidence:** architecture reuse (the concerns/compare pipelines do
~90% of the work); the adversarial-honesty framing; the curriculum mappings and
transfer boundary; the input-adapter design; the schema fit.

**MEDIUM confidence:** the qualitative cosine-drop ranges for adversarial
transformations (~0.9→~0.5–0.7 polymorphism, <0.3 metamorphism) — estimates,
not measured. The model-transfer degradation expectation — a priori likely but
empirical.

**LOW confidence (research bets):** whether source-trained models transfer well
enough for production use on decompiled pseudocode; the exact RE threshold
values; whether two-signal disagreement is a reliable evasion detector in
practice. These are irreducible without the benchmark.

The recurring theme across both syntheses: **mathematical soundness ≠ product
value**, and here sharpened to **geometric identity ≠ epistemic reliability
under adversary.** The LA is unchanged from the prior synthesis; the
epistemic status of every measurement degrades under the adversarial setting,
and the practitioner must carry that degradation forward into every score and
every report. The tool's honesty is not a disclaimer — it is the product.
