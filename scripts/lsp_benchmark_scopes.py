"""Measure B03/B04/B07 scope isolation through the existing component probe.

uv run python scripts/lsp_benchmark_scopes.py --output target/lsp-benchmark/SCOPES
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys

from benchmark_lsp import command, freeze, save, source_state
from lsp_benchmark_inputs import ROOT, digest
from lsp_benchmark_memory import build
from lsp_benchmark_results import require


def summarize(rows):
    groups = {}
    for row in rows:
        groups.setdefault((row["unrelated_projects"], row["phase"]), []).append(row)
    summary = []
    for (projects, phase), samples in groups.items():
        require(len(samples) == 3, "scope phase must have three independent samples")
        summary.append({"unrelated_projects": projects, "phase": phase, "samples": len(samples),
            "median_query_ms": statistics.median(row["query_ns"] for row in samples) / 1e6,
            "median_capture_ms": statistics.median(row["capture_ns"] for row in samples) / 1e6,
            "median_scope_selection_ms": statistics.median(row["scope_selection_ns"] for row in samples) / 1e6,
            "indexed_contexts": sorted({row["indexed_contexts"] for row in samples}),
            "selected_contexts": sorted({row["selected_contexts"] for row in samples}),
            "examined_contexts": sorted({row["examined_contexts"] for row in samples}),
            "source_input_bytes": sorted({row["source_input_bytes"] for row in samples}),
            "index_row_bytes": sorted({row["index_row_bytes"] for row in samples})})
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--candidate", type=Path)
    parser.add_argument("--power-mode", default="unspecified")
    args = parser.parse_args()
    try:
        output = args.output.resolve()
        require(output.is_relative_to((ROOT / "target").resolve()) and not output.exists(), "output must be a new target/ directory")
        output.mkdir(parents=True)
        before = source_state()
        artifact, build_ns = (args.candidate.resolve(), None) if args.candidate else build()
        require(source_state() == before, "source changed during scope probe build")
        image = freeze(artifact, output, "lsp_memory")
        probe = json.loads(command([str(output / image["path"]), "--scope-isolation"], timeout=300))
        require(probe["workload"] == "B03+B04+B07.scope-isolation-v1", "unexpected scope probe workload")
        require(len(probe["checkpoints"]) == 63, "scope probe omitted a project/sample/lifecycle phase")
        require(source_state() == before, "source changed during scope probe campaign")
        metadata = {"rustc": command(["rustc", "-Vv"]), "cargo": command(["cargo", "-V"]),
            "platform": platform.platform(), "cpu": platform.processor(), "logical_cpus": os.cpu_count(),
            "profile": "release", "features": ["verification"], "power_mode": args.power_mode,
            "allocator": "lsp_memory counting System allocator; instrumented component timings",
            "build_ns": build_ns, "parallelism": "one synchronous probe; Cargo default build parallelism",
            "cache_state": "native baseline prepared per project count; contexts retired between samples; OS caches uncontrolled",
            "timing_scope": "acquisition worker round trip and feature query timed separately; query starts after input maintenance; fixture materialization/native setup excluded; no actor or transport",
            "source": before, "lock_sha256": digest((ROOT / "Cargo.lock").read_bytes()),
            "harness_sha256": digest(Path(__file__).read_bytes()),
            "limits": "three samples per phase; zero timing means no operation in that phase; source leases/index rows are not RSS; no paired baseline, percentile or speedup claim"}
        report = {"schema": 1, "metadata": metadata, "image": image, "probe": probe,
            "summary": summarize(probe["checkpoints"])}
        save(output / "report.json", report)
        print(json.dumps({"output": str(output), "summary": report["summary"]}, indent=2))
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        print(f"LSP scope benchmark failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
