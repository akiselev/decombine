# Linear Algebra Ideas for Decombine

Date: 2026-07-02

Status: research note / product ideas. No implementation has started.

## Summary

`decombine` is already a linear algebra tool in practical form. The current
architecture turns code units into embedding vectors, compares them with cosine
similarity, builds clusters, and reports likely duplicate code. That maps
directly onto the linear algebra prereq vocabulary:

- A code unit embedding is a vector.
- The embedding model gives a coordinate system for code meaning.
- Cosine similarity is an inner product.
- A search query or concern vector is a linear functional: `dot(query, _)`.
- A similarity graph is a matrix/operator over the repo.
- Clustering is finding structure in that operator.
- Factorization finds a better repo-specific basis.

The best product expansion is not to replace duplicate detection. It is to add a
second analysis mode:

```text
duplicate analysis: which functions are too similar?
concern analysis: which semantic directions explain the repo, and which are
                  scattered across unrelated modules?
```

This would make `decombine` useful for finding cross-cutting concerns such as
logging, authorization, validation, metrics, retries, cache invalidation,
feature flags, database transaction handling, and error normalization.

## Current architecture anchors

The current architecture already gives us the core objects:

- `index` scans source files, extracts code units with Tree-sitter, and stores
  them in SQLite.
- `embed` creates local embedding vectors keyed by normalized body hash.
- `analyze` computes cosine-similar pairs, reranks distant pairs, clusters
  surviving edges, folds exact copies, and writes Markdown reports.
- The schema already has `code_units`, `embedding_models`, `embeddings`, and
  `analysis_runs`.
- Phase 6 already plans exact flat cosine search in blocks and union-find
  clustering.
- Phase 8 already plans benchmarks and calibration.

This is enough to add richer analysis without changing the indexing and
embedding base.

## Curriculum bridge

The useful mental model from the linear algebra prereqs is:

```text
choose a basis
  -> express the object in that basis
  -> do the operation that is easiest there
  -> convert back if needed
  -> check what stayed invariant
```

For `decombine`:

- The object is the code unit, function, method, module, or repo.
- The first representation is the embedding model's coordinate system.
- A better representation may be a concern basis learned from the repo.
- A projection onto one concern basis vector gives a per-function concern score.
- A graph Laplacian eigenbasis gives a structural/semantic coordinate system for
  the repo's similarity graph.
- Model migration can be treated as change of basis or approximate alignment.

Useful translations:

| Linear algebra idea | `decombine` interpretation |
| --- | --- |
| Vector | Code unit embedding |
| Basis | Embedding coordinate system, learned topic basis, or graph eigenbasis |
| Inner product | Similarity score or query score |
| Projection | "How much does this function contain this concern?" |
| Matrix/operator | Similarity graph, transformation between embedding spaces, or projection map |
| Eigenvector | Global mode of variation in the repo |
| Diagonalization | A basis where concerns decouple into independent coordinates |
| Rank/kernel | What semantic dimensions survive or collapse during transformation |
| Determinant | Coarse volume/lossiness gauge for square transforms, not a standalone quality metric |

## Product thesis

The current product shape finds near-duplicate functions. That is valuable, but
it answers a narrow pairwise question.

Linear algebra lets the tool answer repo-level questions:

- "What concerns are present?"
- "Which concerns cross module boundaries?"
- "Which modules mix too many independent concerns?"
- "Which semantic dimensions changed between two embedding models?"
- "Which ignored duplicate clusters remain stable after a model upgrade?"
- "Which functions are high projection on both domain logic and infrastructure
  concerns?"

The key shift is from pairwise duplicate detection to basis/projection analysis.

## Idea 1: Concern projection lenses

Let the user define a concern by text, examples, or both:

```yaml
concerns:
  - name: authz
    query: "permission checks authorization roles access control"
  - name: telemetry
    query: "metrics tracing logging spans counters audit events"
  - name: retries
    query: "retry exponential backoff timeout transient error"
```

Embed the query into a vector `c`. For every code unit vector `x`, compute:

```text
score(unit, concern) = dot(normalize(c), normalize(x))
```

This is exactly the "bra as function" view from the linear algebra prereqs:

```text
concern vector c
  -> partially apply dot(c, _)
  -> get a function Vec -> Scalar
  -> score every code unit
```

Report:

- top functions for each concern
- top directories/modules for each concern
- functions that project highly onto multiple concerns
- concerns that are semantically coherent but structurally scattered

This is the cheapest high-value feature because it reuses the existing embedding
store and exact dot-product code.

### Example report shape

```text
Concern: telemetry
Top terms/examples: metrics, tracing, audit, span, counter
Semantic coherence: 0.81
Structural spread: 0.73
Cross-cutting score: 0.59

High-projection units:
  src/auth/session.rs::validate_session       0.84
  src/payments/charge.rs::capture_payment    0.79
  src/jobs/scheduler.rs::run_job             0.76

Interpretation:
  Telemetry-like behavior appears in auth, payments, and background jobs.
  Review whether this should be centralized, wrapped, or intentionally local.
```

## Idea 2: Cross-cutting score

A cross-cutting concern is not just a semantic cluster. It is a semantic cluster
whose members are dispersed across structural boundaries.

One practical scoring formula:

```text
cross_cutting_score =
    semantic_coherence
  * structural_spread
  * reviewability
```

Where:

- `semantic_coherence` is average pairwise cosine similarity inside the
  candidate concern group, or average projection onto the concern vector.
- `structural_spread` is entropy over directories/modules, mean path distance,
  or number of top-level components touched.
- `reviewability` penalizes huge unreviewable groups and tiny groups with weak
  evidence.

More explicit:

```text
semantic_coherence = mean(dot(x_i, centroid))
module_entropy = -sum_m p_m log(p_m) / log(module_count)
path_spread = mean(path_distance(unit_i, unit_j))
size_penalty = clamp_to_peak(group_size, ideal_range = 3..30)

cross_cutting_score =
    semantic_coherence
  * max(module_entropy, normalized_path_spread)
  * size_penalty
```

This turns aspect-mining language into an explainable report metric. It should
still be presented as a candidate seed requiring human validation, not as a
confirmed architectural fact.

## Idea 3: Learn a repo-specific concern basis with SVD/LSI

The embedding model gives generic coordinates. A repo-specific basis can expose
the dominant semantic directions inside this particular codebase.

Build a matrix:

```text
X = n_code_units x embedding_dimensions
```

Each row is a normalized code-unit embedding.

Then compute a low-rank factorization:

```text
X ~= U_k S_k V_k^T
```

Interpretation:

- rows of `V_k^T` are repo semantic axes
- rows of `U_k S_k` are each code unit's coordinates in those axes
- singular values show how much variation each axis explains

This is the same idea as latent semantic indexing: reduce a high-dimensional
term/document or artifact matrix into latent semantic dimensions. Older source
code comprehension work used LSA/LSI and SVD to cluster functions and support
program understanding.

Possible report:

```text
Repo semantic basis

Axis 01: request/session/user/auth/token
  high positive units:
    src/auth/session.rs::...
    src/api/middleware.rs::...

Axis 02: file/path/read/write/parse
  high positive units:
    src/index/scanner.rs::...
    src/report/filesystem.rs::...

Axis 03: retry/error/backoff/timeout
  high positive units:
    src/embed/model_cache.rs::...
    src/network/client.rs::...
```

### Why this matters

Pairwise duplicate detection asks "are these two functions close?" A learned
basis asks "what independent semantic directions explain this whole repo?"

That supports:

- onboarding and comprehension
- architecture review
- concern discovery
- drift detection between commits
- benchmark fixture labeling

### Caveat

PCA/SVD axes are useful but can be hard to name. They are often holistic
directions with positive and negative loadings. That is powerful for math, but
not always best for report UX.

## Idea 4: Use NMF for interpretable additive parts

Non-negative matrix factorization (NMF) is worth evaluating for concern reports.
The reason is interpretability.

SVD/PCA can combine positive and negative directions. That can create axes that
are mathematically important but hard to explain. NMF constrains the basis and
loadings to non-negative values:

```text
X ~= W H
```

Interpretation:

- `H` contains additive concern parts
- `W` says how much each code unit uses each part

For user-facing reports, "this function is 0.7 validation + 0.2 persistence +
0.1 logging" is often easier to act on than an SVD component with cancellations.

NMF works best on non-negative feature matrices. Raw dense embeddings may have
negative entries, so better candidates are:

- token/identifier TF-IDF matrices
- non-negative projection scores against a concern dictionary
- shifted/clipped embedding-derived features, only if benchmarked carefully

NMF should be an evaluation path, not an MVP dependency.

## Idea 5: Spectral clustering over the similarity graph

The current plan uses thresholded edges plus union-find. That is deterministic
and a good v1 choice, but it can over-merge transitive chains:

```text
A similar to B
B similar to C
C similar to D
therefore A/B/C/D become one component
```

Spectral clustering treats the similarity matrix as a graph operator.

Build an affinity matrix:

```text
A_ij = similarity(unit_i, unit_j)
```

or a sparse k-nearest-neighbor graph. Then compute a normalized graph matrix or
Laplacian and use eigenvectors as a new coordinate system for graph structure.

The intuition:

- raw embeddings give local semantic coordinates
- graph eigenvectors give global repository coordinates
- clusters become easier to separate in the graph eigenbasis

Use cases:

- split large connected components into more coherent subclusters
- find repo-level semantic communities
- detect bridge functions that connect otherwise separate concerns
- find low-frequency concerns spread smoothly across modules
- find high-frequency anomalies local to one area

This maps nicely to the "basis where the operation decouples" lesson: the graph
eigenbasis is the coordinate system where the graph diffusion/smoothing operator
is diagonal.

### Graph Fourier angle

If the repo is a graph, a concern score over functions is a signal on that graph.
The Laplacian eigenvectors act like Fourier modes:

- low-frequency modes: broad concerns spread across many related functions
- high-frequency modes: local spikes, anomalies, or sharp boundaries

That can help distinguish "a real repo-wide concern" from "one weird helper."

## Idea 6: Model alignment as change of basis

The architecture already plans to persist detailed model identity. That makes it
possible to compare embedding spaces across model versions.

Suppose the same code units are embedded by two models:

```text
X = anchor embeddings from old model
Y = anchor embeddings from new model
```

Fit an alignment transform from `X` to `Y`.

For same-dimensional embeddings, use Orthogonal Procrustes:

```text
find R minimizing ||X R - Y||_F
subject to R^T R = I
```

This is a change-of-basis-like move. If the alignment residual is low, then the
new model may preserve the old geometry well enough that reports and ignore
history remain comparable.

Report metrics:

- anchor count
- mean alignment residual
- pairwise dot-product drift before/after alignment
- nearest-neighbor recall before/after alignment
- cluster stability across models

This is better than treating each new embedding model as a totally unrelated
world.

## Determinants: useful, but not the headline metric

The determinant idea is worth keeping, but it needs guardrails.

### Orthogonal alignment

If the learned transform is orthogonal, then:

```text
det(R) = +1 or -1
```

That only tells whether the alignment preserves or flips orientation. It does
not tell whether the models align well. The important metrics are residual error
and dot-product preservation.

### General square transform

If we fit an unconstrained square linear map `A`, then:

```text
det(A) = product of eigenvalues
       = product of singular values, up to sign
```

This is a volume-scaling gauge. If `det(A)` is near zero, at least one direction
was collapsed. That connects to the dimension-collapse chain:

```text
det A = 0
<=> columns dependent
<=> no inverse
<=> nontrivial kernel
<=> 0 is an eigenvalue
```

But determinant compresses too much information into one number. A transform can
have one huge singular value and one tiny singular value with a moderate
determinant, while still being bad for retrieval.

Prefer:

- singular values
- rank
- condition number
- log absolute determinant
- alignment residual
- pairwise cosine drift
- nearest-neighbor recall

### Rectangular transform

If dimensions differ, determinant is not defined. Use singular values and rank.

## Idea 7: Basis mismatch and active/passive debugging

The curriculum's active/passive distinction is useful for explaining model and
report behavior.

Active transformation:

```text
code changed
old function vector -> new function vector
```

Passive representation change:

```text
same code
old embedding basis -> new embedding basis
```

This distinction matters for incremental analysis. If a report changes after a
model upgrade, the code may not have changed at all. The coordinates changed.

Possible command:

```text
decombine models compare --from old --to new
```

Report:

- "same code, new basis" drift
- stable clusters
- unstable clusters
- ignored clusters that should be revalidated
- model-pair compatibility score

## Idea 8: Module-concern matrix

Build a matrix:

```text
M = modules x concerns
```

Each entry is the aggregate projection score of a module onto a concern.

This gives an architecture-level view:

```text
              authz  telemetry  persistence  parsing  retry
api             .72       .41        .22        .04    .08
db              .05       .12        .91        .02    .17
worker          .31       .77        .49        .01    .66
parser          .00       .03        .02        .88    .01
```

Useful derived metrics:

- module concern entropy: modules mixing many concerns
- concern spread entropy: concerns scattered across many modules
- module similarity: modules with similar concern profiles
- architecture drift between commits

This is a concrete place where "basis = schema" becomes product UI. The concern
basis is the schema; each module is serialized as coordinates in that schema.

## Idea 9: Pair duplicate analysis with concern analysis

Duplicate clusters become more useful when labeled by concern coordinates.

Instead of:

```text
Cluster 12: 5 similar functions, top score 0.94
```

Report:

```text
Cluster 12: repeated retry/timeout wrapper
Top concern projections:
  retry/backoff     0.83
  error handling    0.76
  network IO        0.55
Structural spread:
  src/api/client.rs
  src/embed/model_cache.rs
  src/report/uploader.rs
```

That makes the report easier for an AI coding agent to triage.

## Fit with the existing plan

Do not put this in the Phase 0-6 critical path. The current plan should still
ship the duplicate detector first.

Add this as a later analysis lane:

```text
Phase 6: duplicate analysis
Phase 6b: concern projection analysis
Phase 8: benchmark and calibrate concern quality
Phase 9+: optional spectral/NMF/factorization analysis
```

Suggested modules:

```text
src/analyze/
  similarity.rs      # existing exact cosine
  clustering.rs      # existing union-find clusters
  rerank.rs          # existing distance boosts
  projection.rs      # concern query vectors and scores
  spread.rs          # module/path entropy and distance metrics
  factor.rs          # SVD/NMF/LSI experiments
  spectral.rs        # graph Laplacian experiments
  alignment.rs       # model-to-model transforms
```

Suggested commands:

```text
decombine analyze duplicates
decombine analyze concerns
decombine analyze all
decombine models compare
```

If command surface should stay small:

```text
decombine analyze --mode duplicates
decombine analyze --mode concerns
decombine analyze --mode all
```

## Minimal viable concern analysis

The smallest useful version:

1. Let config define named concern queries.
2. Embed concern query text with the same model as code units.
3. Compute projection scores for every code unit.
4. Keep top `N` units per concern above a threshold.
5. Compute module entropy/path spread for each concern.
6. Write `decombine-report/concerns/index.md`.

No SVD. No NMF. No spectral clustering. No new model dependency.

Config sketch:

```yaml
analysis:
  concerns:
    enabled: true
    min_projection: 0.45
    top_units_per_concern: 50
    queries:
      - name: telemetry
        text: "logging tracing metrics spans audit counters"
      - name: authorization
        text: "authorization permission role access policy"
      - name: retry
        text: "retry backoff timeout transient failure"
```

Report sketch:

```text
decombine-report/
  index.md
  cluster-001.md
  concerns/
    index.md
    telemetry.md
    authorization.md
    retry.md
```

Acceptance tests:

- query embedding uses the selected model identity
- projection scores are deterministic with fixture vectors
- top results are sorted stably
- structural spread handles same-file, same-dir, and cross-dir cases
- report output is deterministic
- concern analysis can run without duplicate analysis rerun when embeddings exist

## Storage additions

For MVP, concern reports can be ephemeral. If persistence is useful, add tables
after the report format stabilizes.

Possible tables:

```sql
concern_queries(
  id INTEGER PRIMARY KEY,
  model_id INTEGER NOT NULL REFERENCES embedding_models(id),
  name TEXT NOT NULL,
  query_text TEXT NOT NULL,
  vector_blob BLOB NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(model_id, name)
)

concern_runs(
  id INTEGER PRIMARY KEY,
  model_id INTEGER NOT NULL REFERENCES embedding_models(id),
  config_json TEXT NOT NULL,
  created_at TEXT NOT NULL
)

concern_scores(
  run_id INTEGER NOT NULL REFERENCES concern_runs(id) ON DELETE CASCADE,
  query_id INTEGER NOT NULL REFERENCES concern_queries(id) ON DELETE CASCADE,
  code_unit_id INTEGER NOT NULL REFERENCES code_units(id) ON DELETE CASCADE,
  projection REAL NOT NULL,
  rank INTEGER NOT NULL,
  PRIMARY KEY(run_id, query_id, code_unit_id)
)
```

For factorization/alignment experiments:

```sql
analysis_artifacts(
  id INTEGER PRIMARY KEY,
  run_id INTEGER NOT NULL REFERENCES analysis_runs(id),
  artifact_kind TEXT NOT NULL,
  method TEXT NOT NULL,
  params_json TEXT NOT NULL,
  metrics_json TEXT NOT NULL,
  blob BLOB,
  created_at TEXT NOT NULL
)
```

Keep this generic until the feature proves itself.

## Rust dependency notes

The existing architecture already lists `ndarray`, optionally with `rayon`.
That is enough for MVP projection scoring.

Avoid adding heavy LAPACK/BLAS dependencies early. For factorization:

- prototype with small dense matrices first
- consider `nalgebra` or `faer` for local dense linear algebra
- consider sparse graph crates only after benchmark data shows a need
- keep spectral and NMF behind experimental features or benchmark harnesses

The Phase 8 benchmark pass is the right place to decide whether factorization
belongs in the main binary.

## Research connections

These sources support the direction:

- Maletic and Marcus applied Latent Semantic Analysis to source code and
  internal documentation, using function-level clustering to support program
  understanding:
  <https://www.cs.kent.edu/~jmaletic/papers/ICTAI00.pdf>
- Kuhn, Ducasse, and Girba introduced semantic clustering based on Latent
  Semantic Indexing to group source artifacts by vocabulary, label topics, and
  inspect how topics distribute over system structure:
  <https://research.cs.queensu.ca/home/ahmed/home/teaching/CISC880/F11/papers/SemanticClustering_IST2007.pdf>
- Ng, Jordan, and Weiss describe spectral clustering with eigenvectors of
  matrices derived from pairwise data:
  <https://papers.nips.cc/paper/2092-on-spectral-clustering-analysis-and-an-algorithm>
- Lee and Seung show why non-negative matrix factorization can produce
  parts-based, additive representations that are often more interpretable than
  PCA-style holistic components:
  <https://www.cs.columbia.edu/~blei/fogm/2020F/readings/LeeSeung1999.pdf>
- Marin, Moonen, and van Deursen frame aspect mining around cross-cutting
  concern candidate seeds that require human validation:
  <https://arxiv.org/pdf/cs/0606113>
- Recent embedding alignment work studies when two embedding spaces can be
  aligned by an orthogonal Procrustes transformation while preserving geometry:
  <https://arxiv.org/html/2510.13406v1>

## Recommended next decision

Add "concern projection analysis" to the plan as a post-MVP analysis mode.

The first implementation should be:

```text
named concern query -> embedding vector -> projection scores -> structural spread -> Markdown report
```

This gives immediate value, teaches the linear algebra idea directly, and does
not require committing to SVD, NMF, spectral clustering, or model-alignment
storage yet.

## Deferred experiments

Keep these as benchmark/research items:

- SVD/LSI repo semantic basis
- NMF on token/identifier matrices
- graph Laplacian spectral clustering
- graph Fourier view of concern signals
- model-to-model Procrustes alignment
- singular-value/rank analysis for model drift
- determinant/log-determinant as secondary diagnostics only

## Main caution

Do not present any of this as automatic truth. Embeddings and factorizations
produce candidate structure. The report should say "candidate concern",
"evidence", "projection score", "spread", and "examples", then let a human or AI
coding agent validate whether the concern is real and actionable.
