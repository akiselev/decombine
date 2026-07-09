#!/usr/bin/env python3
"""CodeRankEmbed ONNX calibration, quantization, and verification utility.

This script is intentionally outside the Rust CLI: it needs the Python ML stack
(`onnxruntime`, `transformers`, `torch`) only when producing or validating model
artifacts. The decombine binary should keep loading a concrete custom ONNX file
through `embedding.custom`.

Typical flow:

  scripts/coderank_onnx.py build-corpus --db runs/altium-rebuilds/decombine-coderank.db
  scripts/coderank_onnx.py quantize \
      --source-dir ~/.cache/decombine/custom/coderankembed \
      --calibration runs/model-calibration/coderank-calibration.jsonl \
      --output-dir runs/model-calibration/coderankembed-int8-static-avx512_vnni
  scripts/coderank_onnx.py verify \
      --holdout runs/model-calibration/coderank-holdout.jsonl \
      --reference-dir ~/.cache/decombine/custom/coderankembed \
      --candidate-dir runs/model-calibration/coderankembed-int8-static-avx512_vnni \
      --torch-reference nomic-ai/CodeRankEmbed
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import sqlite3
import sys
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable


TOKENIZER_FILES = [
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
]
DEFAULT_ONNX_FILE = "onnx/model.onnx"
DEFAULT_MODEL_NAME = "nomic-ai/CodeRankEmbed"


def die(message: str) -> None:
    raise SystemExit(message)


def utc_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat()


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def sha256_named_files(root: Path, names: Iterable[str]) -> str:
    h = hashlib.sha256()
    for name in names:
        path = root / name
        data = path.read_bytes()
        h.update(name.encode())
        h.update(b"\0")
        h.update(len(data).to_bytes(8, "little"))
        h.update(b"\0")
        h.update(data)
    return h.hexdigest()


def read_jsonl(path: Path, limit: int | None = None) -> list[dict[str, Any]]:
    rows = []
    with path.open() as f:
        for line in f:
            if line.strip():
                rows.append(json.loads(line))
                if limit is not None and len(rows) >= limit:
                    break
    return rows


def write_jsonl(path: Path, rows: Iterable[dict[str, Any]]) -> int:
    path.parent.mkdir(parents=True, exist_ok=True)
    count = 0
    with path.open("w") as f:
        for row in rows:
            f.write(json.dumps(row, sort_keys=True) + "\n")
            count += 1
    return count


def is_test_path(relative_path: str, name: str) -> bool:
    parts = re.split(r"[/\\]", relative_path.lower())
    if any(p in {"test", "tests", "testing", "spec", "specs", "fixtures"} for p in parts):
        return True
    basename = parts[-1] if parts else relative_path.lower()
    return (
        basename.startswith("test_")
        or "_test." in basename
        or basename.endswith("_test")
        or ".test." in basename
        or ".spec." in basename
        or name.lower().startswith("test")
    )


def length_bucket(chars: int, token_count: int | None, max_length: int) -> str:
    if token_count is not None:
        if token_count >= int(max_length * 0.9):
            return "near-max"
        if token_count >= int(max_length * 0.5):
            return "long"
        if token_count <= 64:
            return "short"
        return "medium"
    if chars >= max_length * 3:
        return "near-max"
    if chars >= max_length:
        return "long"
    if chars <= 300:
        return "short"
    return "medium"


def boilerplate_heavy(text: str, name: str) -> bool:
    lower = text.lower()
    markers = [
        "todo",
        "panic(",
        "unimplemented",
        "notimplemented",
        "throw new",
        "assert",
        "return null",
        "return none",
        "return nil",
    ]
    marker_hits = sum(1 for marker in markers if marker in lower)
    return marker_hits >= 2 or name in {"update", "fmt", "serialize", "deserialize"}


def stable_id(*parts: object) -> str:
    h = hashlib.sha256()
    for part in parts:
        h.update(str(part).encode())
        h.update(b"\0")
    return h.hexdigest()


def maybe_load_tokenizer(tokenizer_dir: Path | None):
    if tokenizer_dir is None:
        return None
    try:
        from transformers import AutoTokenizer
    except ImportError as exc:
        die("install transformers to use --tokenizer-dir for token-aware stratification")
    return AutoTokenizer.from_pretrained(tokenizer_dir, trust_remote_code=True)


def count_tokens(tokenizer: Any, text: str, max_length: int) -> int | None:
    if tokenizer is None:
        return None
    encoded = tokenizer(
        text,
        truncation=True,
        max_length=max_length,
        add_special_tokens=True,
    )
    return min(len(encoded["input_ids"]), max_length)


def load_db_rows(db_path: Path, tokenizer: Any, max_length: int) -> list[dict[str, Any]]:
    con = sqlite3.connect(db_path)
    con.row_factory = sqlite3.Row
    rows = []
    query = """
        SELECT
          p.label AS project,
          f.relative_path AS relative_path,
          cu.language_id AS language,
          cu.kind AS kind,
          cu.name AS name,
          cu.scope AS scope,
          cu.body_node_count AS body_node_count,
          cu.normalized_body_hash AS normalized_body_hash,
          cu.embedding_text AS text
        FROM code_units cu
        JOIN files f ON cu.file_id = f.id
        JOIN projects p ON f.project_id = p.id
        WHERE cu.embedding_text IS NOT NULL AND length(cu.embedding_text) > 0
    """
    try:
        for row in con.execute(query):
            text = row["text"]
            token_count = count_tokens(tokenizer, text, max_length)
            chars = len(text)
            test = is_test_path(row["relative_path"], row["name"])
            boilerplate = boilerplate_heavy(text, row["name"])
            bucket = length_bucket(chars, token_count, max_length)
            item = {
                "id": stable_id(
                    db_path,
                    row["project"],
                    row["relative_path"],
                    row["normalized_body_hash"],
                ),
                "source_db": str(db_path),
                "project": row["project"],
                "relative_path": row["relative_path"],
                "language": row["language"],
                "kind": row["kind"],
                "name": row["name"],
                "scope": row["scope"],
                "body_node_count": row["body_node_count"],
                "normalized_body_hash": row["normalized_body_hash"],
                "body_chars": chars,
                "tokens": token_count,
                "strata": {
                    "language": row["language"],
                    "length": bucket,
                    "test": test,
                    "boilerplate": boilerplate,
                },
                "text": text,
            }
            rows.append(item)
    finally:
        con.close()
    return rows


def split_corpus(
    rows: list[dict[str, Any]],
    calibration_size: int,
    holdout_size: int,
    holdout_ratio: float,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    groups: dict[str, list[dict[str, Any]]] = {}
    for row in rows:
        strata = row["strata"]
        key = "|".join(
            [
                strata["language"],
                strata["length"],
                "test" if strata["test"] else "prod",
                "boilerplate" if strata["boilerplate"] else "normal",
            ]
        )
        groups.setdefault(key, []).append(row)
    for group in groups.values():
        group.sort(key=lambda row: row["id"])

    calibration: list[dict[str, Any]] = []
    holdout: list[dict[str, Any]] = []
    for key in sorted(groups):
        group = groups[key]
        for row in group:
            want_holdout = (
                len(holdout) < holdout_size
                and int(row["id"][:8], 16) / 0xFFFFFFFF < holdout_ratio
            )
            if want_holdout:
                holdout.append(row)
            elif len(calibration) < calibration_size:
                calibration.append(row)

    remaining = [
        row
        for group in groups.values()
        for row in group
        if row not in calibration and row not in holdout
    ]
    remaining.sort(key=lambda row: row["id"])
    for row in remaining:
        if len(holdout) < holdout_size:
            holdout.append(row)
        elif len(calibration) < calibration_size:
            calibration.append(row)
        else:
            break
    return calibration, holdout


def cmd_build_corpus(args: argparse.Namespace) -> None:
    tokenizer = maybe_load_tokenizer(args.tokenizer_dir)
    rows = []
    for db in args.db:
        db_path = Path(db)
        if not db_path.exists():
            die(f"database does not exist: {db_path}")
        loaded = load_db_rows(db_path, tokenizer, args.max_length)
        print(f"loaded {len(loaded)} retained embedding texts from {db_path}", file=sys.stderr)
        rows.extend(loaded)
    rows.sort(key=lambda row: row["id"])
    if not rows:
        die("no retained embedding texts found; use index.retention: full or rerun index")

    calibration, holdout = split_corpus(
        rows,
        calibration_size=args.calibration_size,
        holdout_size=args.holdout_size,
        holdout_ratio=args.holdout_ratio,
    )
    out_dir = Path(args.out_dir)
    calibration_path = out_dir / "coderank-calibration.jsonl"
    holdout_path = out_dir / "coderank-holdout.jsonl"
    write_jsonl(calibration_path, calibration)
    write_jsonl(holdout_path, holdout)
    metadata = {
        "created_at": utc_now(),
        "source_dbs": [str(Path(db)) for db in args.db],
        "max_length": args.max_length,
        "tokenizer_dir": str(args.tokenizer_dir) if args.tokenizer_dir else None,
        "calibration_rows": len(calibration),
        "holdout_rows": len(holdout),
    }
    (out_dir / "coderank-corpus-metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(f"wrote {len(calibration)} calibration rows to {calibration_path}")
    print(f"wrote {len(holdout)} holdout rows to {holdout_path}")


def make_calibration_data_reader(
    model_dir: Path,
    rows: list[dict[str, Any]],
    input_names: set[str],
    max_length: int,
    batch_size: int,
):
    from onnxruntime.quantization import CalibrationDataReader
    from transformers import AutoTokenizer

    class JsonlCalibrationDataReader(CalibrationDataReader):
        def __init__(self) -> None:
            self.tokenizer = AutoTokenizer.from_pretrained(model_dir, trust_remote_code=True)
            self.offset = 0

        def get_next(self) -> dict[str, Any] | None:
            import numpy as np

            if self.offset >= len(rows):
                return None
            batch = rows[self.offset : self.offset + batch_size]
            self.offset += batch_size
            encoded = self.tokenizer(
                [row["text"] for row in batch],
                padding=True,
                truncation=True,
                max_length=max_length,
                return_tensors="np",
            )
            feed = {}
            for name in input_names:
                if name in encoded:
                    feed[name] = encoded[name]
                elif name == "token_type_ids":
                    feed[name] = np.zeros_like(encoded["input_ids"])
            return feed

        def rewind(self) -> None:
            self.offset = 0

    return JsonlCalibrationDataReader()


def prepare_output_dir(path: Path, force: bool) -> None:
    if path.exists():
        if not force:
            die(f"output directory exists; pass --force to replace: {path}")
        shutil.rmtree(path)
    path.mkdir(parents=True)


def copy_support_files(source_dir: Path, output_dir: Path, onnx_file: str) -> None:
    for name in TOKENIZER_FILES:
        shutil.copy2(source_dir / name, output_dir / name)
    source_onnx = source_dir / onnx_file
    output_onnx = output_dir / onnx_file
    output_onnx.parent.mkdir(parents=True, exist_ok=True)
    if source_onnx.resolve() != output_onnx.resolve():
        shutil.copy2(source_onnx, output_onnx.with_suffix(".fp32.onnx"))


def quantization_method(name: str):
    from onnxruntime.quantization import CalibrationMethod

    methods = {
        "minmax": CalibrationMethod.MinMax,
        "entropy": CalibrationMethod.Entropy,
        "percentile": CalibrationMethod.Percentile,
    }
    try:
        return methods[name]
    except KeyError:
        die(f"unknown calibration method {name!r}")


def quant_type(name: str):
    from onnxruntime.quantization import QuantType

    types = {
        "qint8": QuantType.QInt8,
        "quint8": QuantType.QUInt8,
    }
    try:
        return types[name]
    except KeyError:
        die(f"unknown quant type {name!r}")


def write_manifest(path: Path, artifact: dict[str, Any]) -> None:
    if path.exists():
        manifest = json.loads(path.read_text())
    else:
        manifest = {
            "schema_version": 1,
            "model": DEFAULT_MODEL_NAME,
            "created_at": utc_now(),
            "artifacts": [],
        }
    manifest["updated_at"] = utc_now()
    artifacts = [a for a in manifest.get("artifacts", []) if a.get("name") != artifact["name"]]
    artifacts.append(artifact)
    manifest["artifacts"] = artifacts
    path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")


def cmd_quantize(args: argparse.Namespace) -> None:
    try:
        import onnxruntime as ort
        from onnxruntime.quantization import QuantFormat, quantize_static
    except ImportError:
        die("install onnxruntime to quantize CodeRankEmbed")

    source_dir = Path(args.source_dir).expanduser()
    source_onnx = source_dir / args.onnx_file
    if not source_onnx.exists():
        die(f"source ONNX file does not exist: {source_onnx}")
    rows = read_jsonl(Path(args.calibration), limit=args.max_calibration_rows)
    if not rows:
        die("calibration file is empty")

    output_dir = Path(args.output_dir)
    prepare_output_dir(output_dir, args.force)
    copy_support_files(source_dir, output_dir, args.onnx_file)
    output_onnx = output_dir / args.onnx_file

    session = ort.InferenceSession(str(source_onnx), providers=["CPUExecutionProvider"])
    input_names = {inp.name for inp in session.get_inputs()}
    reader = make_calibration_data_reader(
        source_dir,
        rows,
        input_names=input_names,
        max_length=args.max_length,
        batch_size=args.batch_size,
    )
    quantize_static(
        model_input=str(source_onnx),
        model_output=str(output_onnx),
        calibration_data_reader=reader,
        quant_format=QuantFormat.QDQ,
        activation_type=quant_type(args.activation_type),
        weight_type=quant_type(args.weight_type),
        per_channel=args.per_channel,
        reduce_range=args.reduce_range,
        calibrate_method=quantization_method(args.method),
        op_types_to_quantize=args.op_type,
    )

    artifact_name = args.artifact_name or output_dir.name
    manifest_path = Path(args.manifest) if args.manifest else output_dir / "decombine-model-manifest.json"
    artifact = {
        "name": artifact_name,
        "created_at": utc_now(),
        "source_dir": str(source_dir),
        "source_onnx": args.onnx_file,
        "artifact_dir": str(output_dir),
        "onnx_file": args.onnx_file,
        "model_sha256": sha256_file(output_onnx),
        "tokenizer_sha256": sha256_named_files(output_dir, TOKENIZER_FILES),
        "quantization": {
            "kind": "int8-static",
            "format": "QDQ",
            "activation_type": args.activation_type,
            "weight_type": args.weight_type,
            "calibration_method": args.method,
            "per_channel": args.per_channel,
            "reduce_range": args.reduce_range,
            "op_types": args.op_type,
            "calibration_rows": len(rows),
            "max_length": args.max_length,
        },
        "verification": None,
    }
    write_manifest(manifest_path, artifact)
    print(f"wrote quantized ONNX artifact to {output_onnx}")
    print(f"wrote manifest to {manifest_path}")


def mean_pool(hidden: Any, attention_mask: Any):
    import numpy as np

    mask = attention_mask.astype(np.float32)
    masked = hidden * mask[:, :, None]
    denom = mask.sum(axis=1, keepdims=True).clip(min=1.0)
    return masked.sum(axis=1) / denom


def normalize_rows(vectors: Any):
    import numpy as np

    vectors = vectors.astype(np.float32)
    norms = np.linalg.norm(vectors, axis=1, keepdims=True)
    norms[norms == 0] = 1.0
    return vectors / norms


def embed_onnx(model_dir: Path, onnx_file: str, texts: list[str], max_length: int, batch_size: int):
    import numpy as np
    import onnxruntime as ort
    from transformers import AutoTokenizer

    tokenizer = AutoTokenizer.from_pretrained(model_dir, trust_remote_code=True)
    session = ort.InferenceSession(str(model_dir / onnx_file), providers=["CPUExecutionProvider"])
    input_names = {inp.name for inp in session.get_inputs()}
    vectors = []
    for start in range(0, len(texts), batch_size):
        batch = texts[start : start + batch_size]
        encoded = tokenizer(
            batch,
            padding=True,
            truncation=True,
            max_length=max_length,
            return_tensors="np",
        )
        feed = {}
        for name in input_names:
            if name in encoded:
                feed[name] = encoded[name]
            elif name == "token_type_ids":
                feed[name] = np.zeros_like(encoded["input_ids"])
        output = session.run(None, feed)[0]
        if output.ndim == 3:
            pooled = mean_pool(output, encoded["attention_mask"])
        elif output.ndim == 2:
            pooled = output
        else:
            die(f"unsupported ONNX output rank {output.ndim}")
        vectors.append(pooled)
    return normalize_rows(np.vstack(vectors))


def embed_torch(model_name: str, texts: list[str], max_length: int, batch_size: int):
    import numpy as np
    import torch
    from transformers import AutoModel, AutoTokenizer

    tokenizer = AutoTokenizer.from_pretrained(model_name, trust_remote_code=True)
    model = AutoModel.from_pretrained(model_name, trust_remote_code=True)
    model.eval()
    vectors = []
    with torch.no_grad():
        for start in range(0, len(texts), batch_size):
            batch = texts[start : start + batch_size]
            encoded = tokenizer(
                batch,
                padding=True,
                truncation=True,
                max_length=max_length,
                return_tensors="pt",
            )
            output = model(**encoded)
            hidden = getattr(output, "last_hidden_state", output[0]).detach().cpu().numpy()
            pooled = mean_pool(hidden, encoded["attention_mask"].detach().cpu().numpy())
            vectors.append(pooled)
    return normalize_rows(np.vstack(vectors).astype(np.float32))


@dataclass
class VerificationMetrics:
    mean_pooled_cosine: float
    min_pooled_cosine: float
    p95_pairwise_delta: float
    max_pairwise_delta: float
    top10_recall: float

    def to_json(self) -> dict[str, float]:
        return {
            "mean_pooled_cosine": self.mean_pooled_cosine,
            "min_pooled_cosine": self.min_pooled_cosine,
            "p95_pairwise_delta": self.p95_pairwise_delta,
            "max_pairwise_delta": self.max_pairwise_delta,
            "top10_recall": self.top10_recall,
        }


def compare_embeddings(reference: Any, candidate: Any) -> VerificationMetrics:
    import numpy as np

    if reference.shape != candidate.shape:
        die(f"embedding shapes differ: {reference.shape} vs {candidate.shape}")
    pooled = np.sum(reference * candidate, axis=1)
    ref_sim = reference @ reference.T
    cand_sim = candidate @ candidate.T
    n = reference.shape[0]
    if n > 1:
        mask = ~np.eye(n, dtype=bool)
        delta = np.abs(ref_sim[mask] - cand_sim[mask])
        p95_delta = float(np.percentile(delta, 95))
        max_delta = float(delta.max())
        k = min(10, n - 1)
        recall = []
        for row in range(n):
            ref_order = np.argsort(ref_sim[row])[::-1]
            cand_order = np.argsort(cand_sim[row])[::-1]
            ref_top = [idx for idx in ref_order if idx != row][:k]
            cand_top = [idx for idx in cand_order if idx != row][:k]
            recall.append(len(set(ref_top) & set(cand_top)) / k)
        top10_recall = float(np.mean(recall))
    else:
        p95_delta = 0.0
        max_delta = 0.0
        top10_recall = 1.0
    return VerificationMetrics(
        mean_pooled_cosine=float(pooled.mean()),
        min_pooled_cosine=float(pooled.min()),
        p95_pairwise_delta=p95_delta,
        max_pairwise_delta=max_delta,
        top10_recall=top10_recall,
    )


def require_gate(label: str, ok: bool, value: float, threshold: str) -> None:
    status = "PASS" if ok else "FAIL"
    print(f"{status} {label}: {value:.6f} ({threshold})")
    if not ok:
        raise SystemExit(2)


def update_manifest_verification(manifest_path: Path | None, artifact_dir: Path, metrics: dict[str, Any]) -> None:
    if manifest_path is None:
        manifest_path = artifact_dir / "decombine-model-manifest.json"
    if not manifest_path.exists():
        return
    manifest = json.loads(manifest_path.read_text())
    artifact_dir_str = str(artifact_dir)
    for artifact in manifest.get("artifacts", []):
        if artifact.get("artifact_dir") == artifact_dir_str or artifact.get("name") == artifact_dir.name:
            artifact["verification"] = metrics
    manifest["updated_at"] = utc_now()
    manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")


def cmd_verify(args: argparse.Namespace) -> None:
    rows = read_jsonl(Path(args.holdout), limit=args.max_holdout_rows)
    if not rows:
        die("holdout file is empty")
    texts = [row["text"] for row in rows]
    reference_dir = Path(args.reference_dir).expanduser()
    reference = embed_onnx(reference_dir, args.reference_onnx, texts, args.max_length, args.batch_size)

    torch_metrics = None
    if args.torch_reference:
        torch_reference = embed_torch(args.torch_reference, texts, args.max_length, args.batch_size)
        torch_metrics = compare_embeddings(torch_reference, reference)
        print("fp32 ONNX vs Torch")
        require_gate(
            "min pooled cosine",
            torch_metrics.min_pooled_cosine >= args.fp32_torch_min_cosine,
            torch_metrics.min_pooled_cosine,
            f">= {args.fp32_torch_min_cosine}",
        )

    candidate_metrics = None
    if args.candidate_dir:
        candidate_dir = Path(args.candidate_dir).expanduser()
        candidate = embed_onnx(candidate_dir, args.candidate_onnx, texts, args.max_length, args.batch_size)
        candidate_metrics = compare_embeddings(reference, candidate)
        print("candidate ONNX vs fp32 ONNX")
        require_gate(
            "mean pooled cosine",
            candidate_metrics.mean_pooled_cosine >= args.int8_mean_cosine,
            candidate_metrics.mean_pooled_cosine,
            f">= {args.int8_mean_cosine}",
        )
        require_gate(
            "min pooled cosine",
            candidate_metrics.min_pooled_cosine >= args.int8_min_cosine,
            candidate_metrics.min_pooled_cosine,
            f">= {args.int8_min_cosine}",
        )
        require_gate(
            "p95 pairwise delta",
            candidate_metrics.p95_pairwise_delta <= args.int8_p95_pairwise_delta,
            candidate_metrics.p95_pairwise_delta,
            f"<= {args.int8_p95_pairwise_delta}",
        )
        require_gate(
            "max pairwise delta",
            candidate_metrics.max_pairwise_delta <= args.int8_max_pairwise_delta,
            candidate_metrics.max_pairwise_delta,
            f"<= {args.int8_max_pairwise_delta}",
        )
        require_gate(
            "top-10 recall",
            candidate_metrics.top10_recall >= args.int8_top10_recall,
            candidate_metrics.top10_recall,
            f">= {args.int8_top10_recall}",
        )
        update_manifest_verification(
            Path(args.manifest) if args.manifest else None,
            candidate_dir,
            {
                "verified_at": utc_now(),
                "holdout": str(args.holdout),
                "holdout_rows": len(rows),
                "fp32_vs_torch": torch_metrics.to_json() if torch_metrics else None,
                "int8_vs_fp32": candidate_metrics.to_json(),
            },
        )

    result = {
        "holdout_rows": len(rows),
        "fp32_vs_torch": torch_metrics.to_json() if torch_metrics else None,
        "candidate_vs_fp32": candidate_metrics.to_json() if candidate_metrics else None,
    }
    print(json.dumps(result, indent=2, sort_keys=True))


def add_common_embedding_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--max-length", type=int, default=2048)
    parser.add_argument("--batch-size", type=int, default=4)


def main() -> None:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)

    build = sub.add_parser("build-corpus")
    build.add_argument("--db", action="append", required=True, help="decombine SQLite database")
    build.add_argument("--out-dir", default="runs/model-calibration")
    build.add_argument("--tokenizer-dir", type=Path)
    build.add_argument("--max-length", type=int, default=2048)
    build.add_argument("--calibration-size", type=int, default=4096)
    build.add_argument("--holdout-size", type=int, default=512)
    build.add_argument("--holdout-ratio", type=float, default=0.2)
    build.set_defaults(func=cmd_build_corpus)

    quant = sub.add_parser("quantize")
    quant.add_argument("--source-dir", required=True)
    quant.add_argument("--onnx-file", default=DEFAULT_ONNX_FILE)
    quant.add_argument("--calibration", required=True)
    quant.add_argument("--output-dir", required=True)
    quant.add_argument("--artifact-name")
    quant.add_argument("--manifest")
    quant.add_argument("--force", action="store_true")
    add_common_embedding_args(quant)
    quant.add_argument("--max-calibration-rows", type=int)
    quant.add_argument("--method", choices=["minmax", "entropy", "percentile"], default="percentile")
    quant.add_argument("--activation-type", choices=["qint8", "quint8"], default="qint8")
    quant.add_argument("--weight-type", choices=["qint8", "quint8"], default="qint8")
    quant.add_argument("--per-channel", action=argparse.BooleanOptionalAction, default=True)
    quant.add_argument("--reduce-range", action="store_true")
    quant.add_argument("--op-type", action="append")
    quant.set_defaults(func=cmd_quantize)

    verify = sub.add_parser("verify")
    verify.add_argument("--holdout", required=True)
    verify.add_argument("--reference-dir", required=True)
    verify.add_argument("--reference-onnx", default=DEFAULT_ONNX_FILE)
    verify.add_argument("--candidate-dir")
    verify.add_argument("--candidate-onnx", default=DEFAULT_ONNX_FILE)
    verify.add_argument("--torch-reference", default=DEFAULT_MODEL_NAME)
    verify.add_argument("--manifest")
    verify.add_argument("--max-holdout-rows", type=int)
    add_common_embedding_args(verify)
    verify.add_argument("--fp32-torch-min-cosine", type=float, default=0.99999)
    verify.add_argument("--int8-mean-cosine", type=float, default=0.999)
    verify.add_argument("--int8-min-cosine", type=float, default=0.995)
    verify.add_argument("--int8-p95-pairwise-delta", type=float, default=0.005)
    verify.add_argument("--int8-max-pairwise-delta", type=float, default=0.02)
    verify.add_argument("--int8-top10-recall", type=float, default=0.98)
    verify.set_defaults(func=cmd_verify)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
