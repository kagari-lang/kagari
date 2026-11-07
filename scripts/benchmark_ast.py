"""Run the fixed AST suite sequentially; retain samples and paired process ratios."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import shutil
from statistics import median
import subprocess

ROOT = Path(__file__).resolve().parents[1]
NAMES = ("small", "medium", "large", "trivia", "syntax_corpus", "recovery", "limits", "declarations")
VALID = NAMES[:5]


def command(*args: str) -> str:
    return subprocess.check_output(args, cwd=ROOT, text=True, encoding="utf-8").strip()


def build_executable() -> Path:
    """Discover the bench executable from Cargo, including fresh cached artifacts."""
    output = command("cargo", "build", "--release", "--locked", "-p", "kagari-hir",
                     "--bench", "ast_baseline", "--message-format=json-render-diagnostics")
    executables = []
    for line in output.splitlines():
        message = json.loads(line)
        target = message.get("target", {})
        if (message.get("reason") == "compiler-artifact"
                and target.get("name") == "ast_baseline"
                and target.get("kind") == ["bench"]
                and message.get("executable")):
            executables.append(Path(message["executable"]))
    if len(executables) != 1:
        raise ValueError(f"expected one ast_baseline bench executable artifact, got {len(executables)}")
    return executables[0].resolve()


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate_comparison_metadata(baseline: dict, candidate: dict) -> None:
    """Require the frozen image and identical dependency/environment configuration."""
    if baseline["executable_sha256"]["candidate"] != candidate["executable_sha256"]["baseline"]:
        raise ValueError("baseline image differs from its frozen metadata")
    for key in ("lock_sha256", "rustc", "cargo", "platform", "processor", "logical_cpus",
                "profile", "features", "parallelism", "cache", "environment"):
        if baseline[key] != candidate[key]:
            raise ValueError(f"baseline configuration mismatch: {key}")


def validate(report: dict) -> dict:
    if (report["schema"], report["warmups"], report["samples"], report["check_only"]) != (1, 3, 11, False):
        raise ValueError("unexpected measurement configuration")
    rows = report["workloads"]
    if tuple(row["name"] for row in rows) != NAMES:
        raise ValueError("missing, duplicate or reordered workload")
    result = {}
    for row in rows:
        if row["bytes"] <= 0 or row["files"] <= 0 or len(row["blake3"]) != 64:
            raise ValueError("invalid workload identity")
        evidence = row["evidence"]
        keys = {"diagnostics", "functions", "field_checksum", "lexical_tokens"}
        if set(evidence) != keys or any(type(value) is not int or value < 0 for value in evidence.values()):
            raise ValueError("incomplete or invalid correctness evidence")
        erroneous = row["name"] in ("recovery", "limits")
        if (row["evidence"]["diagnostics"] > 0) != erroneous:
            raise ValueError("unexpected diagnostics")
        expected = ["lex_drop", "parse_drop", "parse_and_drop_split"]
        if row["name"] in VALID:
            expected += ["parse_hir_drop", "field_read"]
        if [stage["stage"] for stage in row["stages"]] != expected:
            raise ValueError("missing measurement stage")
        for stage in row["stages"]:
            if not isinstance(stage["iterations"], int) or stage["iterations"] <= 0:
                raise ValueError("invalid batch count")
            for key in ("ns_per_workload", "drop_ns_per_workload"):
                values = stage[key]
                if key == "drop_ns_per_workload" and stage["stage"] != "parse_and_drop_split":
                    if values is not None:
                        raise ValueError("unexpected split timing")
                    continue
                if len(values) != 11 or any(not math.isfinite(value) or value <= 0 for value in values):
                    raise ValueError("invalid timing samples")
                name = "drop_only" if key == "drop_ns_per_workload" else stage["stage"]
                result[row["name"], name] = median(values)
    return result


def identity(report: dict) -> list:
    # Counts are evidence of workload preservation, not raw CST shape equivalence.
    return [{key: row[key] for key in ("name", "files", "bytes", "blake3", "evidence")}
            for row in report["workloads"]]


def aggregate(runs: list[dict]) -> list[dict]:
    if not runs:
        raise ValueError("at least one process group required")
    images = set(runs[0])
    if images not in ({"candidate"}, {"candidate", "baseline"}) or any(set(run) != images for run in runs):
        raise ValueError("process groups must be uniformly paired or candidate-only")
    reference = identity(runs[0]["candidate"])
    series = {}
    for run in runs:
        candidate = validate(run["candidate"])
        if identity(run["candidate"]) != reference:
            raise ValueError("candidate workload/evidence changed")
        baseline = None
        if "baseline" in run:
            baseline = validate(run["baseline"])
            if identity(run["baseline"]) != reference:
                raise ValueError("baseline workload/evidence mismatch")
        for key, value in candidate.items():
            row = series.setdefault(key, {"candidate": [], "baseline": [], "ratios": []})
            row["candidate"].append(value)
            if baseline is not None:
                row["baseline"].append(baseline[key])
                row["ratios"].append(value / baseline[key])
    result = []
    for (name, stage), values in series.items():
        timings = values["candidate"]
        ratios = values["ratios"]
        result.append({"workload": name, "stage": stage, "median_ns": median(timings),
                       "min_process_ns": min(timings), "max_process_ns": max(timings),
                       "candidate_spread": (max(timings) - min(timings)) / median(timings),
                       "baseline_spread": ((max(values["baseline"]) - min(values["baseline"]))
                                           / median(values["baseline"])) if ratios else None,
                       "paired_ratio": median(ratios) if ratios else None})
    return result


def validate_memory(report: dict) -> dict:
    if (report["schema"], report["diagnostic_only"], report["lifecycles"]) != (1, True, 4):
        raise ValueError("unexpected memory configuration")
    if tuple(row["name"] for row in report["workloads"]) != (*NAMES, "inline_retention"):
        raise ValueError("missing memory workload")
    result = {}
    keys = {"requests", "requested_bytes", "frees", "freed_bytes", "live_bytes", "peak_bytes"}
    for row in report["workloads"]:
        if type(row["bytes"]) is not int or row["bytes"] <= 0 or len(row["blake3"]) != 64:
            raise ValueError("invalid memory workload identity")
        previous = {key: 0 for key in keys}
        for stage in ("parsed", "with_view", "parse_released", "released"):
            counts = row[stage]
            if set(counts) != keys or any(type(v) is not int or v < 0 for v in counts.values()):
                raise ValueError("invalid allocation counters")
            if (counts["live_bytes"] != counts["requested_bytes"] - counts["freed_bytes"]
                    or counts["peak_bytes"] < counts["live_bytes"]):
                raise ValueError("inconsistent allocation accounting")
            if any(counts[key] < previous[key] for key in keys - {"live_bytes"}):
                raise ValueError("nonmonotonic cumulative allocation counters")
            previous = counts
        if row["parsed"]["requests"] == 0 or row["parsed"]["live_bytes"] == 0:
            raise ValueError("unmeasured parser allocations")
        if row["released"]["live_bytes"] != 0:
            raise ValueError("leaked parse/view backing")
        if row["name"] == "inline_retention" and row["parse_released"]["live_bytes"] == 0:
            raise ValueError("unmeasured retained view")
        result[row["name"]] = row
    return result


def summarize_memory(runs: list[dict], memory: dict) -> dict:
    """Report memory levels and allocation deltas without performance thresholds."""
    aggregate(runs)
    if set(memory) != set(runs[0]):
        raise ValueError("memory images must match timing images")
    measured = {image: validate_memory(report) for image, report in memory.items()}
    candidate, baseline = measured["candidate"], measured.get("baseline")
    identities = {row["name"]: row for row in identity(runs[0]["candidate"])}
    memory_rows, allocations = [], []
    for name, row in candidate.items():
        previous = baseline[name] if baseline is not None else None
        if previous is not None and any(row[key] != previous[key] for key in ("bytes", "blake3")):
            raise ValueError("memory workload mismatch")
        if name in identities and any(row[key] != identities[name][key] for key in ("bytes", "blake3")):
            raise ValueError("memory/timing workload mismatch")
        for stage, metric in (("released", "peak_bytes"), ("parsed", "live_bytes"),
                              ("parse_released", "live_bytes")):
            value = row[stage][metric]
            before = previous[stage][metric] if previous is not None else None
            memory_rows.append({"workload": name, "stage": stage, "metric": metric,
                                "baseline": before, "candidate": value,
                                "difference": value - before if before is not None else None,
                                "ratio": value / before if before else None})
        for image, workloads in measured.items():
            prior = {key: 0 for key in ("requests", "requested_bytes", "frees", "freed_bytes")}
            for stage in ("parsed", "with_view", "parse_released", "released"):
                current = workloads[name][stage]
                allocations.append({"workload": name, "image": image, "stage": stage,
                                    **{key: current[key] - prior[key] for key in prior}})
                prior = {key: current[key] for key in prior}
    return {"memory": memory_rows, "allocations": allocations}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", type=Path,
                        help="use an existing executable; otherwise build the release ast_baseline bench")
    parser.add_argument("--baseline-executable", type=Path)
    parser.add_argument("--runs", type=int, default=7)

    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs must be positive")
    images = {"candidate": args.executable.resolve() if args.executable is not None else build_executable()}
    if args.baseline_executable:
        images["baseline"] = args.baseline_executable.resolve()
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    output = ROOT / "target/ast-baseline" / stamp
    output.mkdir(parents=True)
    hashes = {name: sha(path) for name, path in images.items()}
    metadata = {
        "revision": command("git", "rev-parse", "HEAD"),
        "status": command("git", "status", "--short"),
        "rustc": command("rustc", "-Vv"), "cargo": command("cargo", "-V"),
        "platform": platform.platform(), "processor": platform.processor(),
        "logical_cpus": os.cpu_count(), "profile": "workspace release",
        "features": "default", "parallelism": "default Cargo; single-thread measurement",
        "cache": "resident source, warmed allocator; fresh parse each iteration",
        "environment": {key: os.environ.get(key) for key in
                        ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_JOBS", "RUSTC_WRAPPER")},
        "lock_sha256": sha(ROOT / "Cargo.lock"), "executable_sha256": hashes,
        "harness_sha256": {str(path.relative_to(ROOT)): sha(path) for path in [
            Path(__file__).resolve(), *sorted((ROOT / "crates/kagari-hir/benches/ast_baseline").glob("*.rs"))]},
        "runs": args.runs,
        "scope": "input loading, source construction, correctness walks and compilation excluded",
        "comparison_policy": "identical Cargo.lock SHA-256 and environment/toolchain configuration required; "
                             "dependency changes are not supported; no performance pass/fail thresholds",
    }
    if "baseline" in images:
        baseline_metadata = json.loads((images["baseline"].parent / "metadata.json").read_text(encoding="utf-8"))
        validate_comparison_metadata(baseline_metadata, metadata)
        metadata["baseline_metadata"] = baseline_metadata
    (output / "metadata.json").write_text(json.dumps(metadata, indent=2), encoding="utf-8")
    # The frozen image remains usable after the next cargo build overwrites target/release.
    shutil.copy2(images["candidate"], output / images["candidate"].name)
    shutil.copy2(ROOT / "Cargo.lock", output / "Cargo.lock")
    for relative in metadata["harness_sha256"]:
        archived = output / "harness" / relative
        archived.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / relative, archived)
    (output / "inputs.json").write_text(command(str(images["candidate"]), "--inputs"), encoding="utf-8")
    runs = []
    for index in range(args.runs):
        order = list(images) if index % 2 == 0 else list(reversed(images))
        run = {}
        for name in order:
            report = json.loads(command(str(images[name])))
            (output / f"{index:02}-{name}.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
            validate(report)
            run[name] = report
        runs.append(run)
        print(f"Completed process group {index + 1}/{args.runs}", flush=True)
    if hashes != {name: sha(path) for name, path in images.items()}:
        raise ValueError("executable changed during measurement")
    summary = aggregate(runs)
    (output / "summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
    print(output)
    for row in summary:
        if row["stage"] == "parse_drop":
            print(f"{row['workload']}: {row['median_ns'] / 1000:.3f} us; paired ratio={row['paired_ratio']}")
    memory = {name: json.loads(command(str(image), "--memory")) for name, image in images.items()}
    for name, report in memory.items():
        validate_memory(report)
        (output / f"memory-{name}.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    memory_summary = summarize_memory(runs, memory)
    (output / "memory-summary.json").write_text(json.dumps(memory_summary, indent=2), encoding="utf-8")
    if "baseline" in images:
        print("Paired timing and memory comparison; no performance pass/fail thresholds.")
    else:
        print("Standalone baseline characterization and memory evidence; no comparison claim.")


if __name__ == "__main__":
    main()
