# Name/Code Embedding Consistency Research

Date: 2026-07-08

Status: research synthesis. No implementation has started.

## Question

Can decombine embed names separately from whole code blocks and use the
relationship between the two to detect:

- function names that diverge from functionality;
- inconsistent naming conventions across a codebase;
- semantically similar functions whose names do not line up;
- similar names that hide different behavior;
- local rename suggestions or review candidates?

Short answer: yes, but the useful version is multi-channel and calibrated. The
current "full unit" embedding probably already includes the function name in the
signature. The new opportunity is to split the naming signal from the behavior
signal and compare them deliberately.

## Method

Local code inspected:

- `src/index/extractor.rs`
- `src/db/models.rs`
- `src/db/migrations.rs`
- `src/analyze/context.rs`
- `src/analyze/vector_store.rs`
- `src/analyze/concerns/mod.rs`
- `src/analyze/compare/mod.rs`
- `docs/research/code-embedding-exploration.md`
- `docs/ideas/code-embedding-exploration.md`

External research consulted:

- `code2vec`: <https://arxiv.org/abs/1803.09473>
- `code2seq`: <https://csaws.cs.technion.ac.il/~yahave/blog/code2seq.html>
- Suggesting Accurate Method and Class Names:
  <https://miltos.allamanis.com/publications/2015suggesting/>
- MNire / Suggesting Natural Method Names to Check Name Consistencies:
  <https://ml4code.github.io/publications/nguyen2020suggesting/>
- Deep Learning Based Identification of Inconsistent Method Names:
  <https://arxiv.org/html/2501.12617v1>
- NameChecker:
  <https://ieeexplore.ieee.org/document/9712079/>
- CodeT5:
  <https://arxiv.org/abs/2109.00859>
- GraphCodeBERT:
  <https://arxiv.org/abs/2009.08366>
- UniXcoder:
  <https://arxiv.org/abs/2203.03850>

## Local Architecture Fit

The current schema stores:

```text
code_units.name
code_units.scope
code_units.kind
code_units.language_id
code_units.normalized_body_hash
code_units.display_source
code_units.embedding_text
embeddings(model_id, normalized_body_hash, vector_blob)
```

`AnalysisContext` currently loads `name`, `scope`, `kind`, `language_id`,
`display_source`, and the body-hash vector. That is enough for a read-only
experiment that computes name features at analysis time.

Important caveat: `extractor.rs` builds `embedding_text` from the full unit range
with comments stripped. For most languages, that means the existing embedding
contains the function/method signature and therefore the name. This is good for
duplicate detection and code search, but it makes name/functionality mismatch
harder to isolate because the name can leak into the body vector.

The research path should therefore create separate channels:

```text
full_unit_embedding       existing embedding; name likely included
name_embedding            name/scope/kind only
body_without_name_embedding or body_only_embedding
signature_embedding       name + params + return/type context, no body
neighbor_name_distribution derived from body neighbors
```

The first prototype can compute `name_embedding` and simple lexical features
outside the DB. A proper implementation probably needs either new embedding
tables keyed by `(model_id, unit_id, channel)` or analysis artifacts that store
derived channel vectors for a run.

## Research Background

### Name Prediction Is a Real Code-Modeling Task

`code2vec` demonstrated method-name prediction from code vectors at large scale.
The paper also reports that learned method-name vectors capture semantic
similarities and analogies. This supports the basic premise that names and code
behavior can be placed in related representation spaces.

`code2seq` extended this idea from fixed-label prediction to sequence
generation, which matters because names are usually multi-subtoken sequences
such as `decode_packet_header`, not single labels.

Allamanis et al.'s method/class naming work is directly relevant because it
focuses on descriptive, idiomatic names and learns embeddings for names based
on usage contexts. It also highlights that method/class naming is harder than
local variable naming because good names need to be functionally descriptive.

MNire is especially relevant to the exact product idea. It checks consistency
between a method name and implementation by generating a candidate name and
comparing the candidate against the current name. Its reported insight is
useful for decombine: many method-name tokens can be found in three contexts:
method body, interface, and enclosing class name. That maps well to decombine's
available `display_source`, `name`, `scope`, and future signature extraction.

Recent empirical work on inconsistent method-name detection adds a warning:
reported performance drops when moving from artificial balanced datasets to
more realistic benchmarks where true inconsistent names are rare. It also
calls out weak assumptions in retrieval-based methods, including the assumption
that similar bodies should always have similar names. This is directly relevant
to decombine: we should rank review candidates, not auto-declare bad names.

### Identifier Awareness Matters

CodeT5 explicitly targets identifiers as a special signal, not just ordinary
tokens. That supports treating the name channel as first-class rather than
burying it inside a long code-body embedding. GraphCodeBERT and UniXcoder are
less directly about naming, but reinforce the broader pattern: code meaning is
multi-channel, and structure/identifiers/comments should not be collapsed into
one undifferentiated string if the product question depends on the channels.

## Core Product Ideas

## Idea 1: Name/Body Alignment Score

Compute a name-only vector and compare it to a behavior vector.

Naive version:

```text
name_text = "function name: decode packet header"
body_text = body with function name removed or downweighted
name_vec = embed(name_text)
body_vec = embed(body_text)
alignment = dot(name_vec, body_vec)
```

Low alignment means the name and body are semantically far apart.

But raw alignment is not enough. Short names and long code snippets have
different score distributions, and some models handle code/text asymmetrically.
The score must be calibrated per language, kind, and name length:

```text
z = (alignment - mean_alignment_bucket) / std_alignment_bucket
```

Possible buckets:

- language;
- unit kind;
- body size range;
- name subtoken count;
- product/test/docs;
- method/free function.

Report only the bottom tail, not every below-average unit.

## Idea 2: Body-Neighbor Name Divergence

This is likely more robust than direct name/body dot product.

Algorithm:

1. For each unit, find body-vector nearest neighbors using a body-only or
   name-suppressed embedding.
2. Extract neighbor name subtokens.
3. Compare the unit's name subtokens to the neighbor distribution.
4. Flag units whose body neighbors agree on naming language but the unit's name
   diverges.

Example:

```text
body neighbors mostly: parse_*, decode_*, read_* record
current name: handle_stuff
finding: weak name/body consistency; nearest behavior family suggests parse/decode/read
```

This mirrors the intuition behind method-name recommendation while staying
local to the repo. It avoids requiring a global generative name model.

Scoring options:

```text
neighbor_name_consensus = entropy(neighbor_name_tokens)
name_fit = token_similarity(current_name, neighbor_distribution)
finding_score = high consensus * low name_fit * high body-neighbor similarity
```

High neighbor consensus matters. If similar bodies use many different naming
schemes, the codebase itself does not offer a clear naming convention.

## Idea 3: Same-Name / Different-Body Ambiguity

The inverse problem is also valuable:

```text
names are similar
bodies are not similar
```

This can detect overloaded or misleading names:

- several `process` functions doing unrelated things;
- `handle` used for both HTTP requests and binary records;
- `parse` used for validation-only functions;
- `get_*` functions with side effects;
- boolean predicate names that perform mutation or I/O.

Algorithm:

1. Group by normalized name subtokens or high name-embedding similarity.
2. Within each group, compute body-vector spread.
3. Flag groups with high name similarity but high body dispersion.

This should be reported as "ambiguous naming cluster", not a bug. Some APIs
intentionally repeat names across trait implementations or language conventions.

## Idea 4: Body-Similar / Name-Different Inconsistency

This is the naming analogue of duplicate detection:

```text
bodies are similar
names are not similar
```

Potential findings:

- copy-paste with local naming drift;
- same concept named `decode`, `parse`, `read`, and `load` inconsistently;
- migration/rewrite pairs where names drifted more than behavior;
- wrappers that should follow a convention.

This can reuse duplicate clusters:

```text
for each duplicate cluster:
  compute name-token entropy
  compute dominant verb/noun tokens
  flag high body similarity + high name entropy
```

This should be gated by product/test/docs and exact-copy context. Test fixtures
often intentionally use long descriptive names that diverge from product naming.

## Idea 5: Naming Vocabulary Map

Build a repo-local map of name subtokens:

```text
verb tokens: parse, decode, read, load, build, render, validate
noun tokens: record, packet, schema, request, response
modifier tokens: async, unsafe, raw, cached, default
```

Then analyze:

- which verbs align to which body clusters;
- whether synonyms are used consistently or chaotically;
- whether some modules prefer `parse` while others prefer `decode` for the same
  behavior;
- whether name tokens have unusually broad body-vector spread.

This is more product-friendly than "bad name" warnings. It gives maintainers a
reviewable naming convention report.

## Idea 6: Suggested Rename Candidates

Avoid generating authoritative new names in v1. Start with local suggestions
from neighbors:

```text
current: handle_entry
nearest body-neighbor names:
  decode_record
  decode_entry
  read_record
candidate subtokens:
  decode, read, record, entry
suggestion: consider decode_entry or read_record
```

This is safer than asking an LLM to generate names because:

- suggestions are grounded in the local codebase;
- users can see which neighbors support the suggestion;
- no external model or API is required;
- it respects repo vocabulary.

The report should say "candidate naming alternatives", not "rename to".

## Feature Channels

### Channel A: Current Full Unit Embedding

Pros:

- already exists;
- works with current `AnalysisContext`;
- no extra embedding cost.

Cons:

- name likely leaks into the vector;
- body/name mismatch can be hidden because the vector already contains both;
- exact body hash changes if the name changes, depending on language/unit range.

Use for initial smoke tests only.

### Channel B: Name-Only Embedding

Text format:

```text
function name: decode packet header
scope: Parser
kind: method
language: rust
```

Subtokenize snake_case, camelCase, PascalCase, kebab, and acronyms. Keep both
the raw name and split tokens.

Pros:

- cheap;
- no source reparse needed;
- can be computed from `CodeUnitRef`.

Cons:

- short text embeddings can be noisy;
- generic names like `new`, `run`, `handle`, `process`, `execute` need special
  treatment.

### Channel C: Signature/Interface Embedding

Text format:

```text
method decode packet header
params bytes reader offset
returns packet header result
scope Parser
```

This follows MNire's observation that method body, interface, and enclosing
class context all contribute to good method names.

decombine does not currently extract parameter/return metadata as structured
fields. It could recover them from display source with Tree-sitter captures or
adapter hooks later.

### Channel D: Body Without Name

This is the most important new embedding channel if we want true name/body
mismatch detection.

Options:

1. body node only, excluding the function signature;
2. full unit with the declared name replaced by a placeholder;
3. full unit with all identifiers anonymized by role;
4. full unit with only the function name removed, preserving local variable and
   call identifiers.

Recommended first experiment:

```text
body_without_decl_name = full unit source with declared name replaced by FUNC
```

That is simpler than full identifier anonymization and directly targets leakage
from the name under review.

### Channel E: Lexical Name Features

Embeddings should not replace simple name features:

- subtoken Jaccard similarity;
- edit distance over subtokens;
- first verb token;
- noun-token overlap;
- polarity prefixes: `is`, `has`, `can`, `should`, `no`, `not`, `disable`;
- side-effect verbs: `set`, `update`, `delete`, `write`, `send`, `emit`;
- generic names list: `handle`, `process`, `do`, `run`, `execute`, `new`.

These features are explainable and likely beat embeddings on many naming
convention checks.

## Candidate Analyses

### Analysis 1: Low Name/Body Alignment Outliers

Input:

- name-only vector;
- body-without-name vector;
- bucket calibration.

Report:

- bottom-N alignment outliers;
- nearest body-neighbor names;
- nearest name-neighbor bodies;
- score z-score and bucket;
- warnings for generic names and tests.

### Analysis 2: Duplicate Cluster Naming Entropy

Input:

- existing duplicate clusters;
- name subtokens for members.

Report clusters where:

```text
body similarity high
name entropy high
```

This answers: "we found near-duplicate logic; are we naming it consistently?"

### Analysis 3: Naming Collision Clusters

Input:

- name similarity graph;
- body vectors.

Report groups where:

```text
name similarity high
body dispersion high
```

This answers: "are we using the same name for unrelated behavior?"

### Analysis 4: Verb/Noun Convention Drift

Input:

- body-neighbor graph;
- first verb token and noun tokens.

Report:

- semantic neighborhoods with several competing verbs;
- modules that use different verbs for the same body family;
- synonyms that may need normalization.

Example:

```text
semantic family: binary record ingestion
verbs used: parse(12), decode(9), read(2), load(1)
modules: parser/, wire/, importer/
suggestion: review parse/decode/read convention
```

### Analysis 5: API Surface Naming Risk

Public APIs should be treated differently:

- bad internal names are easy to fix;
- public names may be intentionally stable;
- generated suggestions should be softer.

Report public/exported functions separately if visibility is available. If
visibility is not available, use path/scope heuristics for now.

## Experiment Plan

### E1: Name-Only Embedding Smoke Test

Goal: test whether the selected embedding model gives useful distances for
function names alone.

Procedure:

1. Extract `name_text` for every unit from `CodeUnitRef`.
2. Embed names with the same model.
3. Compare name-nearest neighbors against lexical subtoken similarity.
4. Inspect clusters for generic-name collapse.

Metrics:

- top-k agreement between name embeddings and subtoken similarity;
- generic-name fanout;
- qualitative review of top name clusters.

Decision:

If short name embeddings are unstable or generic names dominate, rely more on
lexical subtoken features and less on name embeddings.

### E2: Body Without Declared Name

Goal: remove the name leakage from behavior embeddings.

Procedure:

1. Create an experimental embedding text mode that replaces the declared name
   with `FUNC`.
2. Embed a small corpus.
3. Compare duplicate/search quality against current full-unit embeddings.
4. Check whether rename pairs become easier to evaluate.

Metrics:

- duplicate cluster quality;
- known rename pair recovery;
- same-body/different-name sensitivity;
- runtime/storage cost.

Decision:

If removing the declared name hurts duplicate detection too much, keep it as a
separate analysis channel rather than changing the default embedding.

### E3: Injected Name-Mismatch Benchmark

Goal: create a cheap ground truth before trusting real findings.

Procedure:

1. Take an indexed corpus.
2. Artificially swap function names among units of similar kind/size.
3. Recompute name/body scores without changing bodies.
4. Measure whether swapped names rank in the bottom alignment tail.

Controls:

- random swaps;
- same-module swaps;
- hard swaps among semantically close units;
- generic-name swaps.

Metrics:

- recall@N for injected mismatches;
- false positives among unchanged functions;
- z-score separation.

Decision:

Only inspect real codebase outliers after injected mismatches are detectable.

### E4: Duplicate-Cluster Naming Entropy

Goal: use existing duplicate findings to detect naming inconsistency.

Procedure:

1. Run duplicate analysis.
2. For each cluster, compute name-token entropy and dominant tokens.
3. Inspect high-entropy clusters in product code.

Metrics:

- number of review-worthy clusters;
- pair-level false positives;
- whether suggestions are obvious from local names.

Decision:

This may be the fastest path to a useful naming report because duplicate
clusters already define behavior families.

### E5: Real-World Manual Review

Goal: check whether the tool finds meaningful naming issues, not just model
oddities.

Procedure:

1. Run on repos already used in OSS eval.
2. Review top 20 findings per analysis.
3. Classify:
   - true naming issue;
   - acceptable synonym/convention;
   - public API intentionally stable;
   - generated/test/framework artifact;
   - model false positive.

Decision:

If fewer than a handful of top findings are actionable, keep this as a research
report rather than a default analyzer.

## Scoring Sketches

### Direct Alignment

```text
alignment = dot(name_vec, body_without_name_vec)
bucket = (language, kind, name_subtoken_count_bucket, body_size_bucket)
z = (alignment - bucket_mean) / bucket_std
```

Flag low `z`, but only if the name is not too generic and the body vector has
enough reliable neighbors.

### Neighbor Consensus Fit

```text
neighbors = top_k_body_neighbors(unit)
token_counts = weighted_subtokens(neighbor.names, by = body_similarity)
consensus = 1 - normalized_entropy(token_counts)
fit = token_similarity(unit.name, token_counts)
score = consensus * (1 - fit) * mean_neighbor_similarity
```

This should be the preferred v1 score because it is repo-local and explainable.

### Naming Collision Spread

```text
name_group = units with similar name subtokens or name_vec
body_spread = mean_pairwise_distance(body_vectors in group)
score = name_similarity * body_spread * reviewability
```

Downrank trait implementations, framework callbacks, constructors, tests, and
generated code.

### Duplicate Cluster Name Entropy

```text
for cluster:
  semantic_strength = top_raw_or_mean_similarity
  name_entropy = entropy(first_verb_tokens) + entropy(noun_tokens)
  score = semantic_strength * name_entropy * product_weight
```

## Product Shape

Possible command family:

```text
decombine analyze names
decombine names alignment --top 50
decombine names clusters --from duplicates
decombine names collisions --top 50
decombine names suggest <unit-selector>
```

Report sections:

1. Name/body alignment outliers.
2. Similar behavior, divergent names.
3. Similar names, divergent behavior.
4. Naming vocabulary map.
5. Candidate rename suggestions grounded in local neighbors.

Each finding should include:

- current name;
- file/scope;
- alignment score or entropy score;
- nearest body-neighbor names;
- nearest name-neighbor functions;
- explanation tokens;
- warning tags.

## Guardrails

- Do not call findings "incorrect names" by default.
- Treat public APIs as compatibility-sensitive.
- Downrank tests, generated code, fixtures, framework callbacks, trait impls,
  constructors, and `new`/`default`/`run`/`handle` style generic names.
- Separate lexical inconsistency from semantic inconsistency.
- Calibrate by language/kind/body-size/name-length.
- Keep name/body channels model-versioned and reproducible.
- Use injected mismatch benchmarks before trusting real outliers.
- Prefer local neighbor-grounded suggestions over global/generated names.

## Implementation Notes

### Minimal Read-Only Prototype

No schema change:

1. Load `AnalysisContext`.
2. Build name texts from `CodeUnitRef`.
3. Embed names in memory with the configured embedder.
4. Use existing body vectors as behavior proxy.
5. Compute direct alignment and name-neighbor/body-neighbor disagreements.
6. Write a Markdown research report.

Limitation: body vectors still include names, so direct alignment is
contaminated. The neighbor-based analyses may still be useful.

### Better Prototype

Add an experimental script:

```text
scripts/name_consistency_experiments.py <db>
```

It can:

- read names/scopes/body source from SQLite;
- generate name-only and body-name-masked texts;
- embed them through an external or local embedding runner;
- compute all scores;
- output CSV/Markdown for manual inspection.

This avoids committing a schema before we know which scores work.

### Production Shape

If experiments work, add channel-aware embeddings:

```text
embedding_channel:
  full_unit
  name
  signature
  body_masked_name
```

Schema option:

```text
unit_embeddings(
  model_id,
  unit_id,
  channel,
  text_hash,
  vector_blob,
  norm,
  created_at,
  primary key(model_id, unit_id, channel)
)
```

This is different from current `embeddings`, which are keyed by
`normalized_body_hash` and intentionally deduplicate exact body texts. Name
channels are unit-specific and should not be keyed only by body hash.

## Recommended Next Step

Start with duplicate-cluster naming entropy and body-neighbor name divergence.
They are most aligned with decombine's existing strengths:

- they reuse current body vectors and duplicate clusters;
- they are repo-local;
- they produce explainable findings;
- they do not require a generative model;
- they are less brittle than raw name/body vector dot product.

Then run the injected mismatch benchmark before adding any user-facing
"misleading name" language.

## Decisions

1. Name embeddings are worth testing, but not as a single raw dot-product
   threshold.
2. The strongest v1 is multi-channel: name-only, body-neighbor names, duplicate
   cluster name entropy, and eventually body-with-declared-name-masked.
3. The current full-unit embedding likely includes the function name, so it
   cannot by itself prove name/function mismatch.
4. Repo-local naming consistency is a better first product than global name
   generation.
5. Any report must use review language: "naming consistency candidate", not
   "bad name".

## Related Files

- `RESEARCH.md`
- `EXPERIMENTS.md`
- `docs/research/code-embedding-exploration.md`
- `src/index/extractor.rs`
- `src/analyze/context.rs`
- `src/analyze/vector_store.rs`
- `src/analyze/duplicate/mod.rs`
- `src/analyze/concerns/mod.rs`
