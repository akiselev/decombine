# Linear Algebra Research Synthesis for Decombine

Date: 2026-07-02

Status: research synthesis. No implementation has started.

Source: structured deepthink analysis connecting `architecture.md` to the
quantum-physics linear-algebra curriculum at `~/git/quantum-physics/prereqs/`
(lessons `02`–`07`, `EQUIVALENCES.md`, and `programmer-math/04`). The goal was
two-fold: find linear-algebra techniques that make decombine more useful beyond
pairwise duplicate detection (especially for cross-cutting concerns), and tie
each technique to a specific curriculum lesson so building the tool is
homework for the QM prereq sequence.

## The keystone insight

decombine's embeddings are L2-normalized, so **cosine similarity *is* the inner
product** (curriculum `04-dot-products-inner-products-and-norm.md`: inner product
= "similarity/query function"). The all-pairs cosine step *already computes*
the Gram matrix `G = XXᵀ` — and then throws away everything except thresholded
edges to feed union-find. The whole opportunity is to stop throwing it away.

The catch: the `n×n` Gram is infeasible at repo scale (~40 GB at `n = 10⁵`,
~4 TB at `n = 10⁶`). The fix is the **dual small matrix** `C = XᵀX`
(`d×d`, `d ≈ 384–768` ≈ 590 KB, eigendecomposition in milliseconds). The
nonzero spectra of `XᵀX` and `XXᵀ` are **identical**, so the cheap route gives
exactly the principal directions the expensive route would. This dual trick is
the backbone of everything below.

Concrete cost comparison (`n = 10⁵`, `d = 384`, f32):

| Matrix | Shape | Memory | Eig cost |
| --- | --- | --- | --- |
| `G = XXᵀ` (Gram) | `[n, n] = [10⁵, 10⁵]` | 40 GB | `O(n³) = 10¹⁵` — infeasible |
| `C = XᵀX` (covariance) | `[d, d] = [384, 384]` | 590 KB | `O(d³) ≈ 5.7×10⁷` — milliseconds |
| `X` itself (loadings) | `[n, d]` | 153 MB (streamable) | — |

The only `n`-dependent memory is `X`, which can be streamed in row batches
(`C += xᵢᵀxᵢ` accumulation) so it is never held all at once.

## Technique catalog

### 1. Diagonalize the covariance `C = XᵀX` (PCA) — SOUND, CHEAP, THE KEYSTONE

This rotates from the embedding model's arbitrary basis to the codebase's
*own* principal basis. It is **literally the curriculum's headline question** —
`programmer-math/04-eigen-thinking-and-diagonalization.md`: *"Which basis makes
the operator simple?"* — applied to the covariance operator. It is a **passive**
change of basis (`03-basis-changes-and-coordinate-systems.md`: *"The vector
never moved. We changed the graph paper."*).

Mathematical soundness: `C = XᵀX` is symmetric positive-semidefinite, so
eigenvalues `λ_k ≥ 0` with the claimed sign — guaranteed, no sign ambiguity.
SVD `X = U Σ Vᵀ` (`σ_k ≥ 0`) gives `XᵀX = V Σ² Vᵀ`, so columns of `V` (right
singular vectors) are exactly the eigenvectors of `C` and `λ_k = σ_k²`. Unit
norm fixes `tr C = Σ‖x_i‖² = n`, so `p_k = λ_k/n` is a probability distribution
for free.

Do **not** center the vectors (centering unit vectors destroys cosine geometry);
do uncentered PCA = principal directions. Add a config flag `center: bool`,
default `false`.

**Critical implementation caveats:**

- **The first uncentered eigenvector points along the corpus mean direction** =
  boilerplate + model anisotropy. Embedding anisotropy is usually
  *multi*-directional, and real spectra decay smoothly with no clear eigengap
  — so **drop the top-m common directions** (configurable default 2–3, or
  cumulative-energy threshold; eigengap only if clearly present) and use axes
  `m+1..k` for "concept" structure.
- **Multi-language repos**: the top axes will separate *languages*, swamping
  concern axes. Run PCA **per-language** (or regress out language) for the
  cross-cutting use. For an 8-language tool this is a required design choice.
- **Linear limit**: PCA finds linear directions only; concern structure may be
  nonlinear (a manifold) and is then undercaptured. UMAP/t-SNE are out of the
  curriculum's linear scope — a reason to treat PCA as a refinement, not a
  foundation.
- **Local vs global**: concerns are *local* to a subsystem. A concern within
  one subsystem may not align with any global principal direction. Compute PCA
  **per-language / per-subsystem / per-cluster** for concern alignment, not
  just globally. Compute stays cheap (many small `d×d` problems).

**Product role:** axes are not human-meaningful unlabeled, so PCA is an
**internal engine** (features for clustering, loadings for cross-cutting, a
`--visualize` 2D map, the spectrum gauge) — *not* a user-facing "axis 3"
report. The assumption that principal directions correspond to *concerns*
(rather than frequency/token-distribution artifacts) is **the untested
hypothesis**; a future LSA-style labeling pass (top-loaded functions + shared
identifiers) could promote PCA to a user-facing "latent concepts" report if
labels prove meaningful. Deferred, not permanently cut.

**Curriculum mapping:** `03` (change of basis = "adapter between encodings"),
`06` (covariance as operator), `07` (eigenbasis / diagonalization =
"decoupling / elementwise fast path"), `programmer-math/04` (*"convert → fast
path → convert back"*). This is the tightest curriculum fit of any technique.

### 2. Spectral clustering (graph-Laplacian eigenvectors) — SOUND MATH, INFEASIBLE DENSE AT SCALE

Replaces union-find's transitive-chaining noise (`A~B, B~C, C~D` ⇒ one bad
30-item blob) with geometry-respecting cuts. Curriculum `07-eigenvectors…`
(eigenvector = "decoupled mode"), though the fit is **looser** than PCA's:
Laplacian low-eigenmodes minimize the Dirichlet energy `fᵀLf` (cut cost),
they do not "diagonalize `L` to decouple its action." Genuine but weaker.

Soundness: `A` symmetric nonneg (thresholded cosine); `L = D − A` satisfies
`fᵀLf = ½ Σ A_ij(f_i − f_j)² ≥ 0`, so `L` is symmetric PSD with `mult(0) =
# connected components`. Ng–Jordan–Weiss / Shi–Malik use
`L_sym = I − D^{-1/2} A D^{-1/2}`, often better when degrees vary.

**But** the dense `n×n` Laplacian is the same 40 GB problem; **no
`linfa-spectral` exists** in 2026 (`linfa` ships KMeans/GMM/DBSCAN/hierarchical
only); a pure-Rust sparse symmetric eigensolver is a genuine **ecosystem gap**
(the only off-the-shelf option is `ndarray-linalg`'s LOBPCG, which drags in
LAPACK).

**Phasing:**

- **v1**: keep union-find as default.
- **Opt-in research path**: (a) **Nyström landmark spectral embedding**
  (recognized, grounded — subsample + propagate labels) on a representative
  subset (`n' ≤ ~5000`, dense is fine), or (b) hand-rolled **Lanczos** over a
  sparse k-NN Laplacian (~150 LoC; matvec is `O(edges)`). Feature-gate it;
  do not make it the default.
- **Watch the knob trap**: spectral clustering swaps the threshold knob for
  `k`. Pick `k` automatically (eigengap) or you have traded one knob for a
  worse one.
- **The open empirical bet**: whether Nyström spectral *empirically* beats
  union-find on code-clone clustering. The method's existence is grounded;
  the win is not. Needs a labeled benchmark, not more reasoning.

### 3. Cross-cutting concerns — HONEST REFRAME, BASELINE-FIRST

Embeddings encode lexical/semantic similarity, **not** architectural
responsibility. High loading-entropy alone flags `hash()` / `log_error()` as
"smeared responsibility" — the false positive that kills user trust. The
honest, actionable reframe is **"this function is a semantic bridge between
duplication clusters that span unrelated scope-trees."**

**v1 — build the simple baseline first (no LA required):** a function is
flagged when it satisfies all three of:

1. **Cluster membership / bridging**: the function belongs to a real
   duplication cluster *and* bridges distinct clusters (not vaguely "near"
   several).
2. **Dispersion (geographic)**: the function's semantic neighbors are far
   apart in the repo. Note: decombine stores only *pair* path-distance
   (rerank adds up to 15% cross-directory, up to 10% same-file line distance),
   so **per-function dispersion must be derived** — mean or upper-quantile
   distance to the function's `k` nearest semantic neighbors. Cheap, but a
   real derivation step, not a free lookup.
3. **Kind / scope gating**: exclude free functions in `utils/` / `common/`
   scope (dispersion is expected there). Keep methods whose *receiver scope*
   differs across their semantic cousins — same logic living in different
   classes is the actual smell.

Gate on all three. Output section: **"Cross-directory duplication"** —
clusters whose members span `≥ 2` top-level modules, with the bridge method
and its `N` receiver scopes listed. **Rank by a combined score, cap to top-N,
each with an explicit action** ("same logic in 4 classes — extract"). No
spectrum shown.

**LA refinement (optional, validate against the baseline):** projection
loadings onto per-language PCA axes `m+1..k`. Because `‖x_i‖ = 1`, **Parseval
gives `Σ_k loading²_ik = 1` exactly** — squared loadings are a rigorous
partition of unity over orthonormal axes. This is a *formal echo* of
`05-projections-and-measurement.md`'s `P(i) = |c_i|²`, but it is **Pythagoras,
not the Born postulate** — no collapse, no probability, no phase.

Dropping the top-m anisotropy directions mitigates the entropy/genericity
confound: near-centroid boilerplate (whose energy was in the dropped common
directions) then has low spread on the residual axes, while a genuinely
cross-cutting function has substantial residual spread. Residual ambiguity
(moderate spread) is exactly what the 3-signal gate resolves.

**The honest status:** the LA signal's **marginal value over the simple
baseline is itself the open empirical question** — do not lead with it. The
baseline-first design deliberately avoids betting the product on the
untested "axes = concerns" hypothesis.

### 4. Effective rank / spectrum gauge — SOUND, LOW PRODUCT VALUE

This is the **sound salvage** of the user's determinant idea (see §5).

Because `λ_k ≥ 0` and `tr C = n` (unit-norm), `p_k = λ_k/n` is a probability
distribution and the **effective rank** `exp(H(p))` (Roy–Vetterli) rigorously
measures how many independent semantic dimensions the corpus uses: `1` if one
eigenvalue holds all mass, `r` if spread evenly over `r`. Cumulative top-k
energy is equally sound. All are orthogonal-basis invariant.

**Caveats:**

- It is **model-relative**: it conflates embedding-model anisotropy with
  codebase structure. A clean codebase and spaghetti can share the same
  number. Compute it on the **bulk spectrum after dropping the top-m
  anisotropy directions**, never on the raw spectrum.
- With no baseline it is uninterpretable; with a baseline the action is
  unclear ("rank went 12 → 14, now what?").

**Product role:** demote to a `doctor` footnote / internal sanity metric — at
most a one-line curiosity beside real findings. Not a user headline.

**Curriculum mapping:** `06-matrices-as-linear-transformations.md` —
determinant = **"lossiness gauge"**; rank-nullity `dimensions_in = kept +
lost`; the **dimension-collapse chain** (`det A = 0 ⇔ columns dependent ⇔ … ⇔
0 is an eigenvalue`). Counting non-negligible eigenvalues is applying the
lossiness gauge: rank = dimensions kept, near-zero eigenvalues = the kernel a
top-k truncation would forget.

### 5. Cross-model "determinant of the transform" — ILL-POSED AS STATED

The user's specific proposal: take the transformation from one embedding to
another, then analyze its determinant. The verdict: **ill-posed as stated.**

- **Non-square** when the two models have different dimensions ⇒ determinant
  undefined.
- Even when square, the determinant is **coordinate-dependent** — each
  model's latent basis is arbitrary, and `det(P T Q) = det P · det T · det Q`,
  so rescaling or reordering a model's latent axes rescales the determinant
  arbitrarily.
- The fit is typically **rank-deficient** (embeddings lie on a low-dim
  manifold), so `det ≈ 0` and noise-dominated.
- It is an **active** map between two *distinct* spaces, not a basis
  relabeling within one space — which violates the passive/active distinction
  that `03-basis-changes…` exists to enforce.

**Sound salvages that keep the spirit:**

- *One model*: `det(C)` (= log-volume of the embedding ellipsoid) **is**
  well-posed (orthogonal-invariant: `det(Pᵀ C P) = det C` for orthogonal
  `P`). Less useful than effective rank, but a valid "volume" reading.
- *Two models*: **CCA canonical correlations** `ρ_k ∈ [0,1]` (basis-invariant
  singular values of the whitened cross-covariance), or **principal angles**
  between the two embedding subspaces. These are the coordinate-independent
  quantities the user's intuition was reaching for.
- *Two snapshots of the same repo*: do a **cluster-ID diff** ("these 3
  clusters merged, this one's dispersion grew"), not an LA quantity. Users
  want membership diffs, which come from cluster IDs, not from a determinant.

## Curriculum map — building decombine is QM homework

| Technique | Curriculum lesson(s) | The CS-frame it instantiates (quoted from `EQUIVALENCES.md` / lesson files) |
| --- | --- | --- |
| Cosine similarity | `04-dot-products…` | inner product = "similarity/query function"; bra = `fn(Vec) -> Scalar` ("`dot(u, _)` is *literally* the bra `⟨u|`") |
| Diagonalize `C = XᵀX` | `03`, `06`, `07`, `programmer-math/04` | change-of-basis = "adapter between encodings"; diagonalization = "decoupling … pay conversion once, run elementwise"; *"Which basis makes the operator simple?"*; *"convert → fast path → convert back"* |
| Projection loadings | `05-projections…` | projection = "query then rebuild"; *"the bra is the query, the ket is the direction to rebuild the answer in"*; squared loading = formal echo of `\|c_i\|²` |
| Effective rank | `06-matrices…` | determinant = "lossiness gauge"; kernel = "ignored inputs"; *"Engineer's read: `dimensions_in = dimensions_kept + dimensions_lost`"* |
| Spectral clustering | `07-eigenvectors…` | eigenvector = "decoupled mode" (looser fit — cut-energy minimization, not operator diagonalization) |

### What genuinely transfers from QM

`C = XᵀX` and a graph Laplacian `L` are real **symmetric positive-semidefinite**,
so the finite-dimensional shadow of the Hermitian spectral theorem (`08`
preview) **does** carry over — real non-negative eigenvalues + orthonormal
eigenbasis are *genuine* here, not analogical. For PCA and Laplacian clustering
the nice spectral behavior is real.

### Where the analogy breaks (do not overreach)

- **Embeddings are not quantum states.** No normalization-to-probability (we
  L2-normalize for cosine by convention, not because physics demands
  `‖ψ‖ = 1`); no complex amplitudes; no phase — global or relative. Lesson
  `04`'s phase/interference machinery and `09`'s unitary time-evolution story
  do not apply.
- **Hermitian/unitary guarantees do not carry over wholesale** — a generic
  codebase matrix need not be Hermitian. The one specific thing that *does*
  transfer is the symmetric-PSD case above.
- **Squared loadings are a geometric identity, not the Born postulate.**
  Lesson `05`'s `|c_i|²` is a probability because of the Born postulate plus
  state normalization; squared PCA loadings on a normalized vector and
  normalized axis sum to 1 by Pythagoras. There is no collapse; projection
  here is an idempotent linear map, not `05`'s separate collapse rule.
- **PCA is a PASSIVE change of basis** (`03`'s explicit active/passive
  distinction). We re-describe fixed function vectors against new axes; we
  are not actively rotating the arrows. Confusing this with the
  active-transformation reading is the exact mistake `03` exists to prevent.
- **Infinite-dimensional cautions** (`12`, `13`) are irrelevant — embeddings
  are finite-dimensional (dim = embedding width). Do not invoke
  domain/convergence guardrails.

## Recommended first step

1. **`analyze/subspace.rs`** — load embeddings from SQLite, stream-build
   `C = XᵀX` (`C += xᵢᵀxᵢ`, never hold all `X`), symmetric eig via
   **`nalgebra::SymmetricEigen`** (pure-Rust, no LAPACK — keeps
   `cargo install` clean; avoid `ndarray-linalg`/LAPACK by default,
   feature-gate it as `fast`). Expose axes `V_k` and loadings `L = XV_k`.
2. **`analyze/spectra.rs`** — effective rank / spectrum gauge: sort `λ`,
   compute entropy, `eff_rank = exp(H(p))` on the bulk after dropping top-m.
   Pure arithmetic over a length-`d` vector; no extra dep.
3. **Defer `analyze/spectral.rs`** until the Nyström/Lanczos path is validated.
4. **v1 recompute `C` from scratch** when the index is dirty. `C` formation is
   `O(n·d²)` (seconds), eig is ms, and the dominant cost is re-loading
   embeddings from SQLite (`O(n·d)` I/O) — which `clustering.rs` /
   `similarity.rs` need anyway, so incremental SVD saves no I/O. Keep a dirty
   flag in SQLite (`last_analyzed < last_indexed`). Future nice-to-have:
   cache `C` and apply `±` rank-1 updates (`C ← C ± x xᵀ`) per changed unit.

**Pair the first module with a small labeled benchmark** (reuse Slopo
fixtures or hand-label a few repos) so each technique is *measured*, not just
built. Every product-value claim above is otherwise unvalidated.

## Rust crate picks (verified 2026 status)

| Crate | Status (2026) | LAPACK? | Verdict for decombine |
| --- | --- | --- | --- |
| `ndarray` | healthy, planned | no (opt `blas`) | Keep — core for `X`, `C`, loadings |
| `nalgebra` | active | no (pure-Rust `SymmetricEigen`, `SVD`) | **Primary pick for the `d×d` eigendecomposition.** `nalgebra::linalg::SymmetricEigen::new` does pure-Rust symmetric eig; fine for `d ≤ 768`. Convert the `d×d` slice from `ndarray` via `nshare` or a plain copy (590 KB) |
| `ndarray-linalg` | v0.18.1 Jan 2026, active | yes (openblas/netlib/intel-mkl) | Avoid by default — `openblas-static` needs gcc+gfortran+make, `intel-mkl-static` downloads binaries; both break a clean `cargo install`. Use only as an opt-in `fast` feature-gate (it is the only crate exposing LOBPCG / `TruncatedEig` for sparse spectral eig) |
| `faer` | rising pure-Rust LA | no | Strong alternative (faster eig/SVD, pure Rust); consider as the `fast` pure-Rust path instead of LAPACK |
| `linfa` / `linfa-reduction` | v0.8.1 Dec 2025, active; pure-Rust default, BLAS optional | optional | Has `Pca` (LOBPCG) + diffusion maps. **No `linfa-spectral` exists** (clustering = KMeans/GMM/DBSCAN/hierarchical only). Don't pull in for v1 — its `Dataset` abstraction + centering-by-convention over-constrain; hand-roll the `d×d` eig. Revisit `linfa-nn` for the k-NN graph |
| `smartcore` | maintained, modular, opt ndarray | no | Has PCA but overlaps `ndarray`; not needed |
| `rusty-machine` | abandoned/deprecated (superseded by `linfa`) | — | Do not use |

## Honest limitations and confidence

**HIGH confidence:** mathematical soundness (PSD eigenvalues, SVD ↔
covariance equivalence, Parseval, ill-posedness of the cross-model
determinant); feasibility (`d×d` vs `n×n` numbers); the curriculum mappings
and the transfer boundary.

**MEDIUM confidence:** product value of techniques 3 and 4 — plausible and
honestly framed, but unvalidated on real codebases.

**LOW confidence (research bets):** whether Nyström spectral empirically
beats union-find on code-clone clustering; whether PCA axes correspond to
*concerns* vs frequency/token artifacts; whether loading-entropy adds
marginal value over the simple baseline. These are irreducible without
experiments and are exactly why the design is baseline-first and benchmark-paired.

The recurring theme across the analysis: **mathematical soundness ≠ product
value.** The techniques that are most elegant pedagogically (PCA diagonalization,
effective rank) are not necessarily the ones that pay the bills (a tighter
cluster report, a cross-directory duplication section). The synthesis stages
them accordingly — elegant LA as internal engine and validation target, simple
metadata-driven signals as the shipped product surface.
