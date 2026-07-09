#!/usr/bin/env python3
"""Linear-algebra experiments over decombine embedding databases.

Runs the testable ideas from docs/ideas/linear-algebra-research-synthesis.md
against several model databases that embedded the *same* corpus:

  E1  spectrum gauge: eigenvalues of C = X^T X / n, effective rank,
      top-m energy, mean-vector norm, predicted vs measured background mean
  E2  all-but-the-top (ABTT): drop top-m principal directions, renormalize,
      re-measure background mean/std and ground-truth pair separation
  E3  cross-model agreement: Spearman correlation of pairwise-similarity
      structure and CCA canonical correlations between model spaces
  E4  ensemble: rank-based fusion of two models' similarities

Usage: embedding_experiments.py <run-dir> (defaults to runs/altium-rebuilds)
"""

import json
import sqlite3
import sys
from pathlib import Path

import numpy as np
from scipy.stats import spearmanr

MODELS = {
    "bge-small": "decombine.db",
    "minilm": "decombine-minilm.db",
    "gte-base": "decombine-gte.db",
    "arctic-m": "decombine-arctic.db",
    "arctic-m-long": "decombine-arcticlong.db",
    "nomic-v1.5": "decombine-nomic.db",
    "jina-code": "decombine-jina.db",
    "coderank": "decombine-coderank.db",
}

# Ground-truth semantic rename pairs (left name, right name) plus same-name
# pairs are derived below from unit metadata shared across databases.
RENAMES = [
    ("parse_params", "parse_entries"),
    ("preserves_duplicate_keys", "duplicate_keys_are_preserved_after_edit"),
    ("read_footprint_data", "decode_pcb_record"),
]


def load(run_dir: Path, db_file: str):
    """Return (hash -> vector) plus unit metadata rows (project, name, hash)."""
    con = sqlite3.connect(run_dir / db_file)
    vecs = {}
    for h, blob in con.execute(
        "SELECT normalized_body_hash, vector_blob FROM embeddings"
    ):
        v = np.frombuffer(blob, dtype=np.float32).astype(np.float64)
        n = np.linalg.norm(v)
        vecs[h] = v / n if n > 0 else v
    units = list(
        con.execute(
            """
            SELECT p.label, cu.name, cu.normalized_body_hash
            FROM code_units cu
            JOIN files f ON cu.file_id = f.id
            JOIN projects p ON f.project_id = p.id
            """
        )
    )
    con.close()
    return vecs, units


def effective_rank(eigvals: np.ndarray) -> float:
    p = eigvals / eigvals.sum()
    p = p[p > 1e-12]
    return float(np.exp(-(p * np.log(p)).sum()))


def spectrum(X: np.ndarray):
    n = X.shape[0]
    C = X.T @ X / n
    eig = np.linalg.eigvalsh(C)[::-1]
    eig = np.clip(eig, 0, None)
    return {
        "eff_rank": effective_rank(eig),
        "top1_energy": float(eig[0] / eig.sum()),
        "top3_energy": float(eig[:3].sum() / eig.sum()),
        "mean_norm": float(np.linalg.norm(X.mean(axis=0))),
    }


def abtt(X: np.ndarray, m: int) -> np.ndarray:
    """All-but-the-top: remove mean, drop top-m principal dirs, renormalize."""
    mu = X.mean(axis=0)
    Xc = X - mu
    _, _, Vt = np.linalg.svd(Xc, full_matrices=False)
    Xp = Xc - Xc @ Vt[:m].T @ Vt[:m]
    norms = np.linalg.norm(Xp, axis=1, keepdims=True)
    norms[norms == 0] = 1.0
    return Xp / norms


def pair_stats(X: np.ndarray, left_idx, right_idx, gt_pairs, rng):
    """Background stats plus ground-truth pair separation in row-rank terms."""
    li = rng.choice(left_idx, size=2048)
    ri = rng.choice(right_idx, size=2048)
    bg = np.einsum("ij,ij->i", X[li], X[ri])
    S = X[left_idx] @ X[right_idx].T  # full cross-similarity, small corpus
    pos_l = {u: i for i, u in enumerate(left_idx)}
    pos_r = {u: i for i, u in enumerate(right_idx)}
    z, top1_margin, ranks = [], [], []
    for l, r in gt_pairs:
        if l not in pos_l or r not in pos_r:
            continue
        row = S[pos_l[l]]
        s = row[pos_r[r]]
        z.append((s - bg.mean()) / bg.std())
        others = np.delete(row, pos_r[r])
        top1_margin.append(float(s - others.max()))
        ranks.append(int((others >= s).sum()))  # 0 == top-1
    return {
        "bg_mean": float(bg.mean()),
        "bg_std": float(bg.std()),
        "gt_pairs": len(z),
        "gt_z_mean": float(np.mean(z)),
        "gt_z_min": float(np.min(z)),
        "gt_top1_rate": float(np.mean([r == 0 for r in ranks])),
        "gt_margin_mean": float(np.mean(top1_margin)),
    }


def cca_corrs(XA: np.ndarray, XB: np.ndarray, k: int = 16) -> np.ndarray:
    """Canonical correlations between top-k PCA score subspaces.

    Full-rank whitening is degenerate when n < d (all correlations hit 1),
    so restrict each side to its top-k principal-component scores first.
    """

    def scores(X):
        Xc = X - X.mean(axis=0)
        U, s, Vt = np.linalg.svd(Xc, full_matrices=False)
        return U[:, :k]

    s = np.linalg.svd(scores(XA).T @ scores(XB), compute_uv=False)
    return s


def main():
    run_dir = Path(sys.argv[1] if len(sys.argv) > 1 else "runs/altium-rebuilds")
    rng = np.random.default_rng(7)

    data = {}
    for tag, db in MODELS.items():
        if not (run_dir / db).exists():
            print(f"skip {tag}: {db} missing", file=sys.stderr)
            continue
        data[tag] = load(run_dir, db)

    # Align: hashes common to every model, one unit row per hash+side.
    common = set.intersection(*(set(v.keys()) for v, _ in data.values()))
    _, units = next(iter(data.values()))
    by_side = {"left": [], "right": []}
    seen = set()
    name_of = {}
    for label, name, h in units:
        if h not in common or h in seen:
            continue
        seen.add(h)
        side = "left" if label == "rebuild" else "right"
        by_side[side].append(h)
        name_of[h] = name.split(".")[-1]
    hashes = by_side["left"] + by_side["right"]
    order = {h: i for i, h in enumerate(hashes)}
    left_idx = np.array([order[h] for h in by_side["left"]])
    right_idx = np.array([order[h] for h in by_side["right"]])

    # Ground truth: rename pairs plus unique same-name cross-side pairs
    # (excluding identical bodies, which exact-copy handles without vectors).
    gt = []
    left_by_name, right_by_name = {}, {}
    for h in by_side["left"]:
        left_by_name.setdefault(name_of[h], []).append(h)
    for h in by_side["right"]:
        right_by_name.setdefault(name_of[h], []).append(h)
    for name, ls in left_by_name.items():
        rs = right_by_name.get(name, [])
        if len(ls) == 1 and len(rs) == 1 and ls[0] != rs[0]:
            gt.append((order[ls[0]], order[rs[0]]))
    for ln, rn in RENAMES:
        ls, rs = left_by_name.get(ln, []), right_by_name.get(rn, [])
        if len(ls) == 1 and len(rs) == 1:
            gt.append((order[ls[0]], order[rs[0]]))
    print(
        f"aligned units: {len(hashes)} "
        f"({len(left_idx)} left, {len(right_idx)} right), gt pairs: {len(gt)}\n"
    )

    matrices = {
        tag: np.stack([vecs[h] for h in hashes]) for tag, (vecs, _) in data.items()
    }

    print("== E1: spectrum gauge (raw spaces)")
    for tag, X in matrices.items():
        s = spectrum(X)
        b = pair_stats(X, left_idx, right_idx, gt, np.random.default_rng(7))
        print(
            f"{tag:14s} d={X.shape[1]:4d} eff_rank={s['eff_rank']:7.2f} "
            f"top1={s['top1_energy']:.3f} top3={s['top3_energy']:.3f} "
            f"|mean|={s['mean_norm']:.3f} bg={b['bg_mean']:+.3f}±{b['bg_std']:.3f} "
            f"gt_z={b['gt_z_mean']:5.2f} (min {b['gt_z_min']:5.2f}) "
            f"top1_rate={b['gt_top1_rate']:.2f}"
        )

    print("\n== E2: all-but-the-top (m dropped directions)")
    for tag, X in matrices.items():
        for m in (0, 1, 2, 3):
            Xp = X if m == 0 else abtt(X, m)
            b = pair_stats(Xp, left_idx, right_idx, gt, np.random.default_rng(7))
            s = spectrum(Xp)
            print(
                f"{tag:14s} m={m} bg={b['bg_mean']:+.3f}±{b['bg_std']:.3f} "
                f"eff_rank={s['eff_rank']:7.2f} gt_z={b['gt_z_mean']:5.2f} "
                f"(min {b['gt_z_min']:5.2f}) top1_rate={b['gt_top1_rate']:.2f} "
                f"margin={b['gt_margin_mean']:+.4f}"
            )
        print()

    print("== E3: cross-model agreement")
    tags = list(matrices)
    tri = np.triu_indices(len(hashes), k=1)
    sims = {t: (matrices[t] @ matrices[t].T)[tri] for t in tags}
    print("spearman correlation of pairwise-similarity structure:")
    for i, a in enumerate(tags):
        for b_ in tags[i + 1 :]:
            rho = spearmanr(sims[a], sims[b_]).statistic
            c = cca_corrs(matrices[a], matrices[b_])
            print(
                f"  {a:14s} vs {b_:14s} rho={rho:.3f} "
                f"cca_top8={np.round(c[:8], 3).tolist()}"
            )

    print("\n== E4: rank-fusion ensemble (mean of per-row rank percentiles)")

    def rank_percentile(S):
        r = S.argsort(axis=1).argsort(axis=1).astype(np.float64)
        return r / (S.shape[1] - 1)

    def gt_metrics_from_S(S):
        z, ranks = [], []
        for l, r in gt:
            row = S[np.where(left_idx == l)[0][0]] if l in left_idx else None
        # reuse pair_stats-style logic inline
        pos_l = {u: i for i, u in enumerate(left_idx)}
        pos_r = {u: i for i, u in enumerate(right_idx)}
        for l, r in gt:
            row = S[pos_l[l]]
            s = row[pos_r[r]]
            others = np.delete(row, pos_r[r])
            ranks.append(int((others >= s).sum()))
        return float(np.mean([r == 0 for r in ranks]))

    singles = {}
    for t in tags:
        S = matrices[t][left_idx] @ matrices[t][right_idx].T
        singles[t] = gt_metrics_from_S(S)
    best_pairs = []
    for i, a in enumerate(tags):
        for b_ in tags[i + 1 :]:
            SA = matrices[a][left_idx] @ matrices[a][right_idx].T
            SB = matrices[b_][left_idx] @ matrices[b_][right_idx].T
            fused = (rank_percentile(SA) + rank_percentile(SB)) / 2
            best_pairs.append((gt_metrics_from_S(fused), a, b_))
    for t in tags:
        print(f"  single {t:14s} gt_top1_rate={singles[t]:.3f}")
    for rate, a, b_ in sorted(best_pairs, reverse=True)[:6]:
        print(f"  fused  {a:14s}+{b_:14s} gt_top1_rate={rate:.3f}")


if __name__ == "__main__":
    main()
