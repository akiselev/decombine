# Code Embedding Exploration Ideas

Date: 2026-07-08

Status: research synthesis. No implementation has started.

## Thesis

The duplicate detector is now a useful foundation. The next product expansion
should be an exploration layer over the same indexed code units and embeddings:

```text
duplicate analysis: which code units are too similar?
exploration: which code units align with a topic, example, or concern, and why?
```

The most promising path is not a single new embedding model or a single global
PCA report. It is a multi-view explorer:

1. build semantic candidates from vectors,
2. add lexical and structural evidence,
3. derive user-defined axes from examples when useful,
4. rerank with margins, fanout, diversity, and path/scope signals,
5. show why each result was returned,
6. let the user mark good and bad examples and rerun.

## Current Anchors

The repo already has most of the substrate:

- `AnalysisContext` loads code units, metadata, and normalized vectors.
- `VectorStore` supports exact dot products, all-pairs search, and top-k search.
- duplicate analysis already handles reranking, bounded clustering, exact-copy
  folding, test/docs sectioning, and cross-directory dispersion.
- concern analysis already embeds text queries and projects every unit onto the
  normalized query vector.
- comparison analysis already has background calibration, fanout suppression,
  and opt-in ABTT common-direction removal.
- `scripts/embedding_experiments.py` already tests spectrum, ABTT, CCA, and
  rank-fusion ideas over existing embedding databases.

That means the first exploration experiments should extend concern/query
projection and reporting. They do not need a new indexing pipeline.

## Idea 1: Example-Derived Concept Axes

User question: if the user gives a list of decoder functions, can decombine
find the "decoder axis" and rank the codebase by projection onto that axis?

Yes, as a practical retrieval heuristic. The safe version is contrastive:

```text
positive examples: known decoder functions
negative/background examples: matched non-decoders
axis = normalize(mean(positives) - mean(negatives))
score(unit) = dot(axis, unit_vector)
```

Better variants:

- **Centroid minus corpus mean**: works when only positives are available, but
  is vulnerable to module/language/style leakage.
- **Linear CAV/probe**: train a small L2 logistic regression or linear SVM on
  positive vs negative examples; use the weight vector as the axis and margin as
  the score.
- **Positive subspace**: if "decoder" splits into binary, text, AST, protocol,
  and format-specific families, use the top `k` positive principal directions
  and rank by projection norm into that subspace.
- **Contrastive PCA**: compare the positive set to a matched background set and
  search for directions enriched in positives but not background.
- **Paired difference axes**: when there are paired examples such as
  `encode_*` versus `decode_*`, average the pairwise differences.

Required shields:

- report the axis as a navigation aid, not proof that a unit is a decoder;
- bootstrap positives and negatives, then report top-k stability;
- run a name-leakage audit: full body embeddings versus identifier/name-only
  or identifier-scrubbed variants;
- use matched negatives from similar files, sizes, visibility, and syntactic
  kinds before trusting random-negative results;
- expose top false positives and high-fanout magnets.

## Idea 2: Query By Example

Given a seed function, retrieve similar or related functions by combining:

- vector neighbors of the seed;
- identifier/token overlap;
- same receiver/type or module;
- path distance and package boundary;
- optional call/import overlap when available;
- fanout penalty and generic-helper penalty.

Initial CLI shape:

```text
decombine qbe src/parser.rs:decode_record --neighbors 50 --why
```

The report should group results by structural pattern and show score reasons:

```text
decode_foo_record
  vector rank: 3
  lexical rank: 2
  same receiver: yes
  package distance: 1
  margin: 0.18
```

This is the code-exploration analogue of the comparison analyzer's semantic
matching, but single-project and interactive.

## Idea 3: Hybrid Search

Dense vectors miss exact identifiers and project-specific names. Lexical search
misses semantic renames. The explorer should use both.

Candidate sources:

- vector top-k from `VectorStore` or a future ANN index;
- BM25/identifier search over unit names, scopes, paths, and body tokens;
- exact symbol/name hits;
- structural neighbors from file/module/package scope;
- duplicate-cluster and comparison edges when available.

Fuse with Reciprocal Rank Fusion first because it avoids score calibration
between unrelated ranking systems:

```text
rrf_score(item) = sum(1 / (k + rank_from_source))
```

Then apply MMR/diversity so the top results do not become twenty near-identical
helpers.

## Idea 4: Topic Map Export

A map is useful if it stays honest:

```text
embeddings -> optional ABTT -> sparse kNN graph -> community detection -> labels
```

Labels should come from identifiers, paths, scopes, imports/callees when
available, and c-TF-IDF-like cluster terms. UMAP/t-SNE can be used for display,
but clustering should happen in the original vector or graph space.

Initial CLI shape:

```text
decombine explore map --scope src --label identifiers
decombine topic inspect 12 --examples 10
```

Useful output:

- representative units;
- top identifiers and paths;
- dominant modules;
- outliers;
- bridge functions between clusters;
- product/test/docs split.

## Idea 5: Relevance Feedback

Exploration should become better when the user marks examples:

```text
decombine feedback session.json --good A,B --bad C,D --rerank
```

First implementation: Rocchio-style update over normalized vectors:

```text
query' = normalize(query + alpha * mean(good) - beta * mean(bad))
```

Second implementation: a tiny local logistic reranker over explainable features:
vector score, margin, lexical score, name similarity, path distance, receiver
overlap, fanout, test/docs status, and generic-name penalties.

This creates reusable local supervision without committing to training a neural
model.

## Recommended Experiment Order

1. **Decoder-axis smoke test**
   - Pick one real corpus with obvious decoder/parser functions.
   - Hand-label 10-30 positives and matched negatives.
   - Compare positive centroid, mean-difference axis, and linear probe.
   - Metrics: precision@20, average precision, top-k overlap under bootstrap,
     and inspected false positives.

2. **Negative sensitivity and leakage audit**
   - Random negatives versus same-module/same-size/hard negatives.
   - Full embeddings versus name-only or identifier-scrubbed inputs.
   - Decision: only ship example-derived axes if top results survive these
     perturbations.

3. **Hybrid RRF ablation**
   - Combine vector rank, lexical rank, name/path hints, fanout penalty, and
     diversity.
   - Compare against vector-only on known altium/cadabra probes and manual
     top-20 inspection.

4. **Query-by-example CLI prototype**
   - Read-only report over existing DBs.
   - No new model work.
   - Output score breakdowns and clustered neighbor groups.

5. **Topic-map prototype**
   - Sparse kNN graph only; no dense all-pairs graph.
   - Label clusters from identifiers and paths.
   - Treat the map as navigation, not architecture truth.

## Defer

- GNNs over Code Property Graphs: interesting, but blocked on labeled data and
  a much stronger graph extractor.
- Dense spectral clustering: use sparse kNN or landmarks only.
- Global PCA axes as user-facing "concerns": useful as an internal diagnostic,
  but too likely to capture language, style, or boilerplate unless validated.
- Hosted-only frontier embedding models: useful for comparison, but local,
  reproducible, private analysis remains the product center.
