#!/usr/bin/env python3
"""Benchmark harness for decombine (Phase 8).

Generates synthetic fixture repositories with *known* duplicate,
near-duplicate, scattered, local, and unrelated functions, then runs the
full index/embed/analyze pipeline per candidate model and reports:

- duplicate-detection quality on the known pairs (found / missed / false)
- wall time for index, embed, analyze
- database size and model cache size
- peak RSS of the embed step

Usage:
    scripts/benchmark.py --binary target/release/decombine \
        --models BGESmallENV15 BGEBaseENV15 JinaEmbeddingsV2BaseCode \
        --sizes 200 1000

Results are printed as a Markdown fragment for docs/benchmarks.md.
"""

import argparse
import json
import re
import resource
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

# --- fixture generation ----------------------------------------------------

NEAR_DUP_TEMPLATE = """def {name}(records, threshold):
    {c}
    kept = []
    for record in records:
        value = record.get("{field}", 0)
        if value >= threshold:
            {var} = value * {mult}
            kept.append(({var}, record["name"]))
    kept.sort()
    return kept
"""

UNRELATED_TEMPLATES = [
    """def {name}(path):
    {c}
    with open(path) as handle:
        lines = handle.read().splitlines()
    return [line.strip() for line in lines if line and not line.startswith("#")]
""",
    """def {name}(width, height, points):
    {c}
    grid = [[0] * width for _ in range(height)]
    for x, y in points:
        if 0 <= x < width and 0 <= y < height:
            grid[y][x] += 1
    return grid
""",
    """def {name}(base_url, params):
    {c}
    query = "&".join(f"{{k}}={{v}}" for k, v in sorted(params.items()))
    if not query:
        return base_url
    return base_url + "?" + query
""",
    """def {name}(items, size):
    {c}
    batches = []
    current = []
    for item in items:
        current.append(item)
        if len(current) >= size:
            batches.append(current)
            current = []
    if current:
        batches.append(current)
    return batches
""",
]


def generate_fixture(root: Path, unrelated_count: int) -> dict:
    """Create a synthetic repo. Returns the expected findings."""
    expected_pairs = []
    modules = ["billing", "shipping", "reports", "api", "core", "utils"]
    for m in modules:
        (root / m).mkdir(parents=True, exist_ok=True)

    # 10 scattered near-duplicate pairs across module boundaries: same
    # logic, different names/fields/comments. These SHOULD be detected.
    for i in range(10):
        a_mod, b_mod = modules[i % len(modules)], modules[(i + 2) % len(modules)]
        for mod, suffix, var, comment in [
            (a_mod, "a", "scaled", "# keep entries over the threshold"),
            (b_mod, "b", "weighted", "# retain qualifying records"),
        ]:
            name = f"filter_records_{i}_{suffix}"
            (root / mod / f"near_{i}_{suffix}.py").write_text(
                NEAR_DUP_TEMPLATE.format(
                    name=name, field=f"score_{i}", mult=3, var=var, c=comment
                )
            )
        expected_pairs.append((f"near_{i}_a.py", f"near_{i}_b.py"))

    # 5 exact-copy pairs (identical bodies, different files/modules).
    for i in range(5):
        body = NEAR_DUP_TEMPLATE.format(
            name=f"exact_copy_{i}", field=f"level_{i}", mult=7, var="scaled",
            c="# shared exact body",
        )
        (root / "billing" / f"exact_{i}_a.py").write_text(body)
        (root / "reports" / f"exact_{i}_b.py").write_text(body)
        expected_pairs.append((f"exact_{i}_a.py", f"exact_{i}_b.py"))

    # Unrelated filler functions. These should NOT cluster together.
    for i in range(unrelated_count):
        template = UNRELATED_TEMPLATES[i % len(UNRELATED_TEMPLATES)]
        mod = modules[i % len(modules)]
        (root / mod / f"fill_{i}.py").write_text(
            template.format(name=f"unique_{i}_task", c=f"# variant {i}")
        )
    return {"expected_pairs": expected_pairs, "unrelated": unrelated_count}


# --- pipeline runner --------------------------------------------------------

def run_timed(cmd, cwd):
    start = time.monotonic()
    before = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    proc = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    if proc.returncode != 0:
        sys.exit(f"command failed: {cmd}\n{proc.stdout}\n{proc.stderr}")
    after = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    return time.monotonic() - start, max(before, after), proc.stdout


def evaluate_report(report_dir: Path, expected_pairs) -> dict:
    """Check which expected pairs ended up in a shared cluster."""
    cluster_files = sorted(report_dir.glob("cluster-*.md"))
    clusters = [f.read_text() for f in cluster_files]
    found = sum(
        1
        for a, b in expected_pairs
        if any(a in text and b in text for text in clusters)
    )
    # False grouping: any cluster containing two distinct unique_* fillers.
    false_groups = sum(
        1
        for text in clusters
        if len(set(re.findall(r"fill_(\d+)\.py", text))) > 1
    )
    return {
        "clusters": len(clusters),
        "expected": len(expected_pairs),
        "found": found,
        "false_filler_groups": false_groups,
    }


def bench(binary: str, model: str, unrelated: int, cache_dir: Path) -> dict:
    workdir = Path(tempfile.mkdtemp(prefix=f"decombine-bench-{model}-"))
    src = workdir / "src"
    fixture = generate_fixture(src, unrelated)
    (workdir / "decombine.yaml").write_text(
        f"""source_dir: src
embedding:
  model: {model}
  cache_dir: {cache_dir}
analysis:
  body_node_count_threshold: 8
"""
    )
    config = ["--config", "decombine.yaml"]
    t_index, _, index_out = run_timed([binary, *config, "index"], workdir)
    units = int(re.search(r"units=(\d+)", index_out).group(1))
    t_embed, rss_embed, _ = run_timed([binary, *config, "embed"], workdir)
    t_analyze, _, _ = run_timed([binary, *config, "analyze"], workdir)
    quality = evaluate_report(workdir / "decombine-report", fixture["expected_pairs"])
    db_size = (workdir / "decombine.db").stat().st_size
    result = {
        "model": model,
        "unrelated": unrelated,
        "units": units,
        "index_s": round(t_index, 2),
        "embed_s": round(t_embed, 2),
        "analyze_s": round(t_analyze, 2),
        "embed_peak_rss_mb": round(rss_embed / 1024, 1),
        "db_mb": round(db_size / 1e6, 2),
        **quality,
    }
    shutil.rmtree(workdir, ignore_errors=True)
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default="target/release/decombine")
    parser.add_argument("--models", nargs="+", default=["BGESmallENV15"])
    parser.add_argument("--sizes", nargs="+", type=int, default=[200])
    parser.add_argument("--cache-dir", default=str(Path.home() / ".cache/decombine-bench"))
    args = parser.parse_args()

    args.binary = str(Path(args.binary).resolve())
    cache_dir = Path(args.cache_dir)
    cache_dir.mkdir(parents=True, exist_ok=True)
    results = []
    for model in args.models:
        for size in args.sizes:
            print(f"benchmarking {model} with {size} filler functions...", file=sys.stderr)
            results.append(bench(args.binary, model, size, cache_dir))

    print("| Model | Units | Index s | Embed s | Analyze s | Embed RSS MB | DB MB | Dup pairs found | False filler groups |")
    print("| --- | --- | --- | --- | --- | --- | --- | --- | --- |")
    for r in results:
        print(
            f"| {r['model']} | {r['units']} | {r['index_s']} | {r['embed_s']} | "
            f"{r['analyze_s']} | {r['embed_peak_rss_mb']} | {r['db_mb']} | "
            f"{r['found']}/{r['expected']} | {r['false_filler_groups']} |"
        )
    print()
    print(json.dumps(results, indent=2), file=sys.stderr)


if __name__ == "__main__":
    main()
