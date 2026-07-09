# Code-Embedding Exploration Research

Date: 2026-07-08

Status: research synthesis. No implementation has started.

## Question

Now that the core duplicate detector is useful, what should decombine explore
next with code embeddings?

The motivating product question is:

> If the user gives a list of functions, for example decoder functions, can we
> derive a shared semantic axis like "decoder" and rank the rest of the codebase
> by projection onto that axis?

Short answer: yes, but only as a retrieval and navigation heuristic. The robust
version is contrastive, calibrated, and explainable. It should not be reported
as proof that a function is a decoder.

## Method

Local code inspected:

- `src/analyze/context.rs`
- `src/analyze/vector_store.rs`
- `src/analyze/concerns/mod.rs`
- `src/analyze/duplicate/*`
- `src/analyze/compare/mod.rs`
- `src/config.rs`
- `src/report/markdown.rs`
- `scripts/embedding_experiments.py`
- `docs/ideas/linear-algebra-analysis.md`
- `docs/ideas/linear-algebra-research-synthesis.md`

Parallel research lanes:

- concept directions and concept activation vectors
- modern code embeddings and code retrieval/clone detection
- structure-aware code exploration
- local decombine2 architecture fit

Primary external references:

- TCAV / Concept Activation Vectors:
  <https://arxiv.org/abs/1711.11279>
- Contrastive PCA:
  <https://www.nature.com/articles/s41467-018-04608-8>
- Linear-probe selectivity cautions:
  <https://aclanthology.org/D19-1275/>
- Iterative Nullspace Projection:
  <https://aclanthology.org/2020.acl-main.647/>
- CodeBERT:
  <https://arxiv.org/abs/2002.08155>
- GraphCodeBERT:
  <https://arxiv.org/abs/2009.08366>
- UniXcoder:
  <https://arxiv.org/abs/2203.03850>
- CodeT5:
  <https://arxiv.org/abs/2109.00859>
- CodeSearchNet:
  <https://arxiv.org/abs/1909.09436>
- CoIR benchmark:
  <https://arxiv.org/abs/2407.02883>
- CodeXEmbed:
  <https://arxiv.org/abs/2411.12644>
- CodeRankEmbed / CoRNStack:
  <https://arxiv.org/html/2412.01007v3>
- Qwen3 Embedding:
  <https://arxiv.org/abs/2506.05176>
- Aroma structural code search:
  <https://arxiv.org/abs/1812.01158>
- Reciprocal Rank Fusion:
  <https://research.google/pubs/reciprocal-rank-fusion-outperforms-condorcet-and-individual-rank-learning-methods/>
- Maximal Marginal Relevance:
  <https://kilthub.cmu.edu/articles/journal_contribution/The_Use_of_MMR_and_Diversity-Based_Reranking_in_Document_Reranking_and_Summarization/6610814>
- Rocchio relevance feedback:
  <https://nlp.stanford.edu/IR-book/pdf/09expand.pdf>
- BERTopic:
  <https://maartengr.github.io/BERTopic/>
- HDBSCAN:
  <https://hdbscan.readthedocs.io/en/latest/how_hdbscan_works.html>
- UMAP:
  <https://arxiv.org/abs/1802.03426>
- Code Property Graphs:
  <https://ieeexplore.ieee.org/document/6956589>

## Local Architecture Fit

decombine2 already has the right substrate for exploration:

- `AnalysisContext` loads code units, metadata, and model identity.
- `VectorStore` stores normalized vectors and exposes exact dot products and
  top-k search.
- concern analysis already embeds a query string and projects every code unit
  onto the normalized query vector.
- comparison analysis already has top-k cross-project retrieval, bounded
  path/name hinting, fanout suppression, background calibration, and ABTT.
- duplicate analysis already has product/test/docs sectioning, bounded
  connected components, exact-copy folding, and cross-directory dispersion.
- reports already present "candidate, not proof" wording for concern results.

That means a first exploration feature does not need a new embedding backend or
a new indexing pipeline. It can start as another analyzer over
`AnalysisContext`, reusing `VectorStore` and the report infrastructure.

The closest existing feature is `ConcernAnalyzer`:

```text
query text -> embed query -> normalize -> dot(query, unit) -> top units
```

The proposed exploration feature generalizes the query vector:

```text
text query -> one query vector
example list -> derived concept vector or subspace
good/bad feedback -> updated query vector or local reranker
```

## Finding 1: The "Decoder Axis" Is Plausible

The idea maps directly onto concept-vector methods:

- TCAV trains a linear classifier from examples of a human concept versus
  counterexamples. The classifier normal is a concept direction.
- Contrastive representation work commonly uses mean positive-minus-negative
  directions.
- Bias/direction work in word embeddings uses projection onto learned semantic
  directions.
- Linear probes are useful when they are treated as measurements, not proof of
  causal representation.

For decombine, the basic version is:

```text
P = vectors for positive decoder examples
N = vectors for matched negative/background examples
axis = normalize(mean(P) - mean(N))
score(unit) = dot(axis, vector(unit))
```

Because decombine stores normalized vectors, the projection is cheap and uses
the same dot-product machinery as concern analysis.

### Why Negatives Matter

Positive-only axes are tempting:

```text
axis = normalize(mean(P))
```

or:

```text
axis = normalize(mean(P) - corpus_mean)
```

Those are useful smoke tests, but they often capture the wrong thing:

- "functions in this directory"
- "long functions"
- "public API methods"
- "functions with decode in the name"
- "Rust functions from one module"
- "functions using a common helper"

Matched negatives are the main shield. For a decoder axis, good negatives might
include:

- functions from the same files or directories that are not decoders;
- same language and same `kind`;
- similar `body_node_count`;
- similar visibility or method/free-function status;
- hard negatives like encoders, validators, renderers, and parsers.

Random negatives are still useful as a sensitivity test, not as the only
background.

## Candidate Algorithms

### 1. Positive Centroid

```text
axis = normalize(mean(P))
score = dot(axis, x)
```

Use only as a baseline. It is fast and easy to explain, but it usually captures
the center of the positive examples plus whatever common embedding anisotropy
the model has.

### 2. Centroid Minus Corpus Mean

```text
axis = normalize(mean(P) - mean(all_units))
score = dot(axis, x)
```

This is the best "positives only" version. It partially removes the global
background, but it cannot distinguish decoder-ness from local module/style
correlates inside the positive set.

### 3. Centroid Minus Matched Negatives

```text
axis = normalize(mean(P) - mean(N))
score = dot(axis, x)
```

This is the first shippable candidate. It is simple, deterministic, easy to
debug, and does not require a training dependency.

Implementation shape:

```rust
fn concept_axis(positives: &[usize], negatives: &[usize], vectors: &VectorStore) -> Vec<f32>
```

Where unit indices map through `row_for_unit`, and missing embeddings are
reported explicitly.

### 4. Linear Probe / CAV

Train a tiny L2 logistic regression or linear SVM:

```text
label = 1 for positives
label = 0 for negatives
axis = learned weight vector
score = classifier margin
```

This is closest to TCAV. Benefits:

- handles unequal positive/negative covariance better than centroid subtraction;
- returns margins;
- supports feature ablations;
- can be bootstrapped over negative sets.

Risks:

- a high probe score can still reflect confounds;
- small positive sets in high dimensions are unstable;
- adding ML training dependencies to the Rust CLI may be overkill.

Pragmatic route: prototype in a Python experiment script over SQLite vectors
first, then port only if it beats centroid-minus-negatives.

### 5. Positive Subspace

If decoder functions form several families, one axis will underfit. Example
subfamilies:

- binary decoders;
- text/protocol decoders;
- AST or record decoders;
- JSON/YAML/TOML deserializers;
- low-level byte readers.

Compute the top `k` principal directions of centered positive examples and
rank by projection norm into that subspace:

```text
S = top-k PCA directions of centered P
score(x) = norm(project(x - mean(P), S))
```

This should be opt-in and reported as "near the positive example subspace",
not as one clean concept axis.

### 6. Contrastive PCA

Contrastive PCA finds directions with high variance in the target set and low
variance in a background set:

```text
C = covariance(P) - alpha * covariance(N)
directions = top eigenvectors(C)
score(x) = projection onto top contrastive directions
```

This is a better fit when positives are varied but share a theme that is not
dominant in background code. The operational problem is choosing `alpha`.
Treat `alpha` as a sweep parameter and judge by precision/stability.

### 7. Paired Difference Axes

When examples come in meaningful pairs:

```text
encode_foo <-> decode_foo
serialize_bar <-> deserialize_bar
write_record <-> read_record
```

derive:

```text
axis = normalize(mean(vector(decode_i) - vector(encode_i)))
```

This is a sharper test for "decoder rather than encoder" than a generic decoder
centroid. It requires better UX for specifying pairs.

## Required Validation Shields

### Bootstrap Stability

Repeatedly resample positives and negatives, recompute the axis, and record:

- axis cosine mean/std;
- top-20 overlap;
- precision@20 variance;
- examples that appear in nearly every run;
- examples that only appear under one negative sample.

Unstable axes should be reported as experimental or rejected.

### Negative-Set Sensitivity

Run at least four backgrounds:

1. random negatives;
2. same-language, same-kind negatives;
3. same-module/same-size negatives;
4. hard negatives such as encoders/parsers/validators.

If the top results change completely, the axis is not robust enough for a
default feature.

### Name Leakage Audit

The likely first success mode for "decoder" is lexical, not semantic. That can
still be useful, but it should be known.

Compare:

- full embedding text;
- unit name/scope/path only;
- identifier-scrubbed body text;
- body text with comments/docstrings stripped, as today;
- optional call/import/token features outside the embedding.

If name-only wins, label the feature as lexical discovery or hybrid search, not
semantic concept discovery.

### Confound Audit

For each top result, compute explanation features:

- path/module;
- language;
- unit kind;
- body node count;
- name/token overlap with positives;
- nearest positive example;
- projection score;
- fanout / generic-helper evidence;
- test/docs/product classification.

This makes false positives diagnosable.

## Finding 2: Code Exploration Should Be Hybrid

Dense embeddings alone are not enough for code exploration.

Reasons:

- identifiers are often the most important domain signal;
- exact API names matter;
- paths and modules encode architecture;
- tests and fixtures can dominate dense similarity;
- generic helpers become semantic magnets;
- embeddings can blur implementation code with tests that share identifiers.

The first exploration system should generate candidates from several cheap
views:

```text
vector search
lexical/identifier search
exact name and path hits
duplicate-cluster edges
comparison edges when present
module/package proximity
optional call/import/type edges later
```

Then fuse them with Reciprocal Rank Fusion:

```text
rrf(item) = sum(1 / (k + rank_i(item)))
```

RRF is attractive because lexical scores, vector cosine, and structural ranks
do not share a calibrated scale.

After fusion, use a transparent reranker:

```text
score =
    vector_score
  + lexical_score
  + name_overlap
  + receiver/type_overlap
  + path/module_bonus
  + margin_bonus
  - fanout_penalty
  - generic_name_penalty
  - test_docs_penalty_when_scope_is_product
```

This can stay deterministic at first. A learned local reranker can come later
once feedback labels exist.

## Query-By-Example Workflow

Initial CLI:

```sh
decombine qbe src/parser.rs:decode_record --neighbors 50 --why
```

Steps:

1. Resolve the seed to a `CodeUnitRef`.
2. Retrieve vector top-k neighbors.
3. Retrieve lexical/name/path neighbors.
4. Fuse with RRF.
5. Rerank with structural and noise features.
6. Apply MMR/diversity so results cover different subfamilies.
7. Write a report with score breakdowns.

Report sketch:

```text
# Query by example: src/parser.rs::decode_record

## Top Matches

1. src/wire.rs::decode_packet
   final score: 0.91
   vector rank: 2
   lexical rank: 5
   nearest positive: decode_record
   path distance: 2
   top identifiers: decode, packet, record, bytes
   warning: none

2. tests/parser.rs::decode_record_rejects_bad_tag
   final score: 0.73
   vector rank: 4
   lexical rank: 1
   warning: test/docs path
```

This is immediately useful even before learned axes.

## Semantic-Axis Workflow

Initial CLI shape:

```sh
decombine explore axis decoder \
  --positive src/parser.rs::decode_record \
  --positive src/wire.rs::decode_packet \
  --negative src/parser.rs::encode_record \
  --negative src/render.rs::render_record \
  --top 50 \
  --why
```

Config-file shape:

```yaml
analysis:
  exploration:
    axes:
      - name: decoder
        positives:
          - src/parser.rs::decode_record
          - src/wire.rs::decode_packet
        negatives:
          - src/parser.rs::encode_record
          - src/render.rs::render_record
        method: centroid_difference
        top_units: 50
        bootstrap_rounds: 50
```

Output should include:

- positive and negative examples found/missing;
- axis method;
- projection-score distribution;
- top units;
- nearest positive/negative example per result;
- stability summary;
- warnings for lexical leakage and path/language concentration.

## Relevance Feedback

Exploration should support iterative refinement:

```sh
decombine feedback session.json --good 1,4,7 --bad 2,3 --rerank
```

First implementation: Rocchio vector update:

```text
q' = normalize(q + alpha * mean(good) - beta * mean(bad))
```

Second implementation: local logistic reranker over explainable features:

- vector score;
- RRF score;
- name similarity;
- token overlap;
- path distance;
- product/test/docs class;
- body node count;
- fanout;
- same receiver/type/scope;
- margin to next-best candidate.

Feedback labels should be saved as local session artifacts. They become
evaluation data for later model/reranker work.

## Topic Map Workflow

Topic maps are useful as navigation, but easy to overclaim. The correct frame
is "map of embedding neighborhoods", not "true architecture".

Pipeline:

```text
vectors
-> optional ABTT / common-direction removal
-> sparse kNN graph
-> community detection or HDBSCAN
-> labels from identifiers, paths, scopes, imports/calls
-> Markdown/HTML map
```

Use UMAP/t-SNE only for display. Do not cluster in 2D layout space.

Initial CLI:

```sh
decombine explore map --scope src --label identifiers
decombine topic inspect 12 --examples 10
```

Report contents:

- representative units per topic;
- top identifiers and path terms;
- dominant modules;
- product/test/docs split;
- outliers;
- bridge functions;
- overlap with duplicate clusters;
- optional call/import/type overlays later.

## Modern Code Embeddings

The older CodeBERT family is still useful background:

- CodeBERT established bimodal NL-code pretraining for search and generation.
- GraphCodeBERT adds data-flow structure.
- UniXcoder combines code/comment/AST modalities.
- CodeT5 adds identifier-aware pretraining for understanding and generation.

For decombine's immediate product direction, however, modern retrieval-tuned
models and reranking are more relevant than encoder architecture novelty.

Relevant current directions:

- CodeRankEmbed / CoRNStack: local-ish code retriever already evaluated in this
  repo through custom ONNX, with strong quality on the altium and OSS runs.
- CodeXEmbed and CoIR: benchmark work showing that no single model dominates
  every code retrieval task.
- Qwen3 Embedding and similar frontier embedding models: strong retrieval
  claims, but operationally heavier and not necessarily local/Rust-friendly.
- Hosted code embeddings such as Voyage Code: useful benchmark comparators, but
  less aligned with decombine's local/private design center.

Evaluation should remain repo-specific. Leaderboards do not directly measure:

- semantic rewrite matching;
- duplicate-report usefulness;
- test/docs boilerplate suppression;
- high-fanout semantic magnets;
- cross-module exploration;
- user-labeled query-by-example success.

## Where ABTT and PCA Fit

ABTT is already in the comparator and should remain an optional preprocessing
step for exploration experiments.

Potential uses:

- remove corpus-wide common directions before deriving example axes;
- reduce model anisotropy before topic maps;
- compare stability of axes with and without common-direction removal.

Global PCA axes should not be user-facing concern labels by default. Earlier
research already identified the failure mode: top axes may separate language,
framework, tests, boilerplate, or body length rather than concerns. PCA is a
diagnostic and preprocessing tool unless experiments prove the axes label
cleanly on real corpora.

## Evaluation Protocol

### Experiment 1: Decoder-Axis Smoke Test

Pick a corpus with obvious decoders/parsers. Candidate corpora:

- altium rebuild pair for read/parse/decode functions;
- cadabra/cadabra2 for geometry/topology conversion and parsing;
- one OSS corpus with serialization/deserialization code.

Create labels:

- 10-30 positive decoder examples;
- 30-100 matched negatives;
- a held-out review set if available.

Compare:

- positive centroid;
- centroid minus corpus mean;
- centroid minus matched negatives;
- linear probe/CAV;
- positive subspace projection;
- optional ABTT before axis derivation.

Metrics:

- precision@10 and precision@20;
- average precision;
- top-k overlap under bootstrap;
- false-positive taxonomy;
- axis cosine stability;
- path/language/name concentration.

Decision gate:

Ship only if centroid-minus-negatives or better beats lexical baseline and is
stable under bootstrap and negative-set changes.

### Experiment 2: Negative Sensitivity

Run the same positives against:

- random negatives;
- same-language/same-kind negatives;
- same-module/same-size negatives;
- hard negatives.

Record:

- top-20 overlap;
- axis cosine;
- AP/P@20;
- high-fanout false positives.

Decision gate:

If top results only work for one convenient negative set, keep the feature
experimental.

### Experiment 3: Name Leakage Audit

Create alternative inputs:

- full embedding text;
- unit names/scopes/paths only;
- identifier-scrubbed body;
- body with comments/docstrings stripped.

Compare P@20 and AP.

Decision gate:

If name-only dominates, implement hybrid lexical search first and describe the
axis as lexical/topic discovery rather than semantic behavior discovery.

### Experiment 4: Hybrid RRF Ablation

Use existing altium/cadabra probes and manual top-20 review.

Compare:

- vector-only;
- lexical-only;
- vector + lexical RRF;
- vector + lexical + path/name/fanout rerank;
- RRF + MMR diversity.

Metrics:

- known probe recovery;
- precision@20;
- duplicate semantic magnets;
- product/test/docs contamination;
- qualitative report usefulness.

### Experiment 5: Query-By-Example Prototype

Build read-only output over existing DBs:

```sh
decombine qbe <unit-selector> --neighbors 50 --why
```

No new embeddings, no ANN, no persistent schema changes required.

Decision gate:

If users can find related functions and explain false positives from the score
breakdown, this becomes the first exploration CLI.

### Experiment 6: Sparse Topic Map

Build a sparse kNN graph, not a dense all-pairs graph.

Evaluate:

- whether topics provide useful entry points;
- whether labels are readable;
- whether product/test/docs separation remains necessary;
- whether UMAP-style visualization adds value beyond Markdown clusters.

Decision gate:

Keep as navigation only unless labels and clusters prove stable across models
and preprocessing.

## Product Shape

Recommended command family:

```text
decombine search --query "parse schematic records" --hybrid --why
decombine qbe src/foo.rs:parse_block --neighbors 30 --why
decombine explore axis decoder --positive ... --negative ... --top 50 --why
decombine feedback session.json --good ... --bad ... --rerank
decombine explore map --scope src --label identifiers
decombine topic inspect 12 --examples 10
```

Initial implementation should be report-oriented, not interactive UI-heavy.
Markdown output fits the current product and is enough to validate ranking.

## Data and Config Needs

Likely new config block:

```yaml
analysis:
  exploration:
    enabled: false
    top_units: 50
    axes: []
    hybrid:
      vector_top_k: 200
      lexical_top_k: 200
      rrf_k: 60
      diversity: true
```

Likely transient session file for feedback:

```json
{
  "query": "decoder",
  "method": "axis",
  "good": ["unit-id-or-location"],
  "bad": ["unit-id-or-location"],
  "notes": {}
}
```

Avoid DB schema changes until the first prototype proves useful. If feedback
sessions become important, persist them under report/artifact directories first.

## Deferrals

Defer these until simpler baselines win:

- GNN training over Code Property Graphs;
- dense spectral clustering;
- global PCA axes as user-facing concerns;
- hosted-only embedding models as product requirements;
- ANN search before exact top-k becomes a measured bottleneck;
- precise call graphs for Rust traits/macros/generics.

## Decisions

1. The "decoder axis" idea is worth testing.
2. The first implementation should be contrastive example-derived axes, not
   global unsupervised PCA.
3. Query-by-example is probably the best first user-facing exploration command.
4. Hybrid retrieval with RRF is the right default exploration architecture.
5. Every result needs a `--why` explanation: vector rank, lexical rank,
   path/scope evidence, nearest examples, fanout, and product/test/docs class.
6. Topic maps are navigation aids only.
7. Code Property Graphs and GNNs stay research-only for now.

## Related Files

- `RESEARCH.md`
- `EXPERIMENTS.md`
- `docs/ideas/code-embedding-exploration.md`
- `scripts/embedding_experiments.py`
- `src/analyze/concerns/mod.rs`
- `src/analyze/context.rs`
- `src/analyze/vector_store.rs`
- `src/analyze/compare/mod.rs`
