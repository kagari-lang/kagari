"""Measure the bounded Native fold fixture in sequential independent processes."""
from __future__ import annotations

import argparse
import csv
from datetime import datetime, timezone
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
ROUTES = ("rust", "managed_rust_scalar", "managed_rust_references", "bridge",
          "scalar", "references", "production", "bridge_gc", "references_gc",
          "production_gc")
DIAGNOSE_ROUTES = ("managed_rust_scalar", "managed_rust_references", "bridge",
                   "bridge_prepared", "bridge_raw", "bridge_raw_values",
                   "script_loop", "script_sum", "scalar", "references")
WARMUPS = 3
SAMPLES = 11
ITEMS = 2048


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def command(args: list[str], timeout: int = 600) -> subprocess.CompletedProcess:
    return subprocess.run(args, cwd=ROOT, capture_output=True, text=True,
                          encoding="utf-8", errors="strict", check=True, timeout=timeout)


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_state() -> dict:
    files = [ROOT / "Cargo.toml", ROOT / "Cargo.lock", Path(__file__).resolve()]
    for package in (ROOT / "crates").iterdir():
        if package.is_dir():
            files.extend(package.rglob("*.rs"))
            files.extend(package.glob("Cargo.toml"))
    return {str(path.relative_to(ROOT)): digest(path) for path in sorted(set(files))}


def cpu() -> str:
    if os.name == "nt":
        import winreg
        with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE,
                            r"HARDWARE\DESCRIPTION\System\CentralProcessor\0") as key:
            return winreg.QueryValueEx(key, "ProcessorNameString")[0].strip()
    return platform.processor()


def validate(raw: str, routes: tuple[str, ...] = ROUTES) -> dict[str, float]:
    rows = list(csv.DictReader(io.StringIO(raw)))
    require(len(rows) == len(routes) * (WARMUPS + SAMPLES), "incomplete row count")
    observed = set()
    samples = {route: [] for route in routes}
    for row in rows:
        route = row["route"]
        sample, folds, items, ns, checksum = (
            int(row[key]) for key in ("sample", "folds", "items", "ns", "checksum"))
        require(route in routes, f"unexpected route: {route}")
        expected_folds = (256 if route == "rust" else
                          16 if route.startswith("managed_rust_") else 1)
        require(0 <= sample < WARMUPS + SAMPLES and items == ITEMS and ns > 0,
                "invalid sample")
        require(folds == expected_folds and checksum == items * (items + 1) // 2 * folds,
                "workload/checksum mismatch")
        require((route, sample) not in observed, "duplicate row")
        observed.add((route, sample))
        if sample >= WARMUPS:
            samples[route].append(ns / (folds * items))
    require(all(len(values) == SAMPLES for values in samples.values()), "missing samples")
    return {route: statistics.median(values) for route, values in samples.items()}


def run(args: argparse.Namespace) -> None:
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    state_path = output / "state.json"
    state_path.write_text(json.dumps({"status": "running"}), encoding="utf-8")
    try:
        require(args.runs >= 1, "runs must be positive")
        require(not (args.baseline and (args.counts or args.sample)),
                "baseline comparison is a throughput mode")
        forbidden = ("CARGO_TARGET_DIR", "CARGO_BUILD_TARGET")
        require(not any(os.environ.get(key) for key in forbidden),
                "use the default Cargo target directory and host target")
        require(not any(key.startswith("CARGO_PROFILE_") for key in os.environ),
                "use the workspace release profile")
        before = source_state()
        metadata = {
            "started_utc": datetime.now(timezone.utc).isoformat(),
            "revision": command(["git", "rev-parse", "HEAD"]).stdout.strip(),
            "status": command(["git", "status", "--porcelain"]).stdout,
            "rustc": command(["rustc", "-Vv"]).stdout,
            "cargo": command(["cargo", "-V"]).stdout.strip(),
            "platform": platform.platform(), "cpu": cpu(),
            "logical_cpus": os.cpu_count(), "profile": "release",
            "features": "kagari-vm defaults; normal example dev-dependencies",
            "backend": "bytecode interpreter; no JIT",
            "parallelism": "Cargo default; sequential benchmark processes",
            "cache": "incremental/cached build; three warmups per route/process",
            "environment": {key: value for key, value in os.environ.items()
                            if key.startswith(("CARGO_BUILD_", "RUSTFLAGS", "RUSTUP_TOOLCHAIN"))
                            or key == "CARGO_ENCODED_RUSTFLAGS"},
            "runs": args.runs, "warmups": WARMUPS, "samples": SAMPLES,
            "items": ITEMS, "source_sha256": before,
            "workload": "diagnosis" if args.diagnose else "original",
        }
        build = ["cargo", "build", "--release", "-p", "kagari-vm",
                 "--example", "managed_fold"]
        if args.counts:
            require(not args.sample, "counting and sampling are separate modes")
            build.extend(["--features", "diagnostics"])
            metadata["features"] += "; diagnostics (not timed)"
        if args.sample:
            # The user authorized full symbols for diagnosis. Keep ordinary
            # throughput on the workspace's unmodified release settings.
            build.extend(["--config", "profile.release.debug=2"])
            metadata["profile"] = "release optimization with full debuginfo; sampling only"
            metadata["symbol_debug_level"] = 2
            metadata["cache"] = "reuse compatible Cargo products; three warmups per sampling route"
        start = time.perf_counter()
        built = command(build, timeout=1200 if args.sample else 600)
        metadata["build_seconds"] = time.perf_counter() - start
        metadata["build_command"] = build
        (output / "build.log").write_text(built.stdout + built.stderr, encoding="utf-8")
        suffix = ".exe" if os.name == "nt" else ""
        binary = output / f"managed_fold{suffix}"
        shutil.copy2(ROOT / "target" / "release" / "examples" / binary.name, binary)
        metadata["binary_sha256"] = digest(binary)
        if args.counts:
            metadata["workload"] = "counts"
            result = command([str(binary), "--counts"])
            (output / "counts.log").write_text(result.stdout, encoding="utf-8")
            (output / "setup.log").write_text(result.stderr, encoding="utf-8")
            require(sum(line.startswith("COUNTS,") for line in result.stdout.splitlines())
                    == len(DIAGNOSE_ROUTES), "missing count routes")
            require(sum(line.startswith("PROBE,") for line in result.stdout.splitlines()) == 2,
                    "missing mapping probes")
            require(before == source_state(), "source changed during counting")
            (output / "metadata.json").write_text(json.dumps(metadata, indent=2), encoding="utf-8")
            state_path.write_text(json.dumps({"status": "complete"}), encoding="utf-8")
            print(result.stdout, flush=True)
            return
        if args.sample:
            metadata["workload"] = "sampling"
            metadata["sample_routes"] = args.sample
            require(os.name == "nt", "sampling uses the existing Windows x64 sampler")
            from profile_lua_comparison import sample
            pdb = binary.with_suffix(".pdb")
            shutil.copy2(ROOT / "target" / "release" / "examples" / pdb.name, pdb)
            metadata["pdb_sha256"] = digest(pdb)
            metadata["sampler_sha256"] = digest(ROOT / "scripts" / "profile_lua_comparison.py")
            (output / "metadata.json").write_text(json.dumps(metadata, indent=2), encoding="utf-8")
            for route in args.sample:
                sample(route, binary, output, False)
            require(before == source_state(), "source changed during sampling")
            require(metadata["binary_sha256"] == digest(binary), "sampling binary changed")
            require(metadata["pdb_sha256"] == digest(pdb), "sampling PDB changed")
            state_path.write_text(json.dumps({"status": "complete"}), encoding="utf-8")
            return
        routes = DIAGNOSE_ROUTES if args.diagnose else ROUTES
        baseline = None
        baseline_estimates = []
        if args.baseline:
            previous = json.loads((args.baseline / "report.json").read_text(encoding="utf-8"))
            baseline = previous["metadata"]
            for key in ("rustc", "cargo", "platform", "cpu", "profile", "features",
                        "environment", "warmups", "samples", "items", "workload"):
                require(metadata[key] == baseline[key], f"baseline mismatch: {key}")
            for name, value in before.items():
                if name.endswith("Cargo.toml") or name == "Cargo.lock" or (
                        name.startswith(str(Path("crates/kagari-vm/examples/managed_fold")))
                        and name.endswith(".rs")):
                    require(baseline["source_sha256"].get(name) == value,
                            f"baseline workload/build input changed: {name}")
            source = args.baseline / binary.name
            require(digest(source) == baseline["binary_sha256"], "baseline image hash mismatch")
            old_binary = output / f"baseline-{binary.name}"
            shutil.copy2(source, old_binary)
        estimates = []
        for process in range(args.runs):
            order = ["baseline", "candidate"] if baseline else ["candidate"]
            if process % 2:
                order.reverse()
            for label in order:
                arguments = [str(old_binary if label == "baseline" else binary), str(process)]
                if args.diagnose:
                    arguments.append("--diagnose")
                result = command(arguments, timeout=300)
                prefix = "baseline-" if label == "baseline" else ""
                (output / f"{prefix}process-{process}.csv").write_text(result.stdout, encoding="utf-8")
                (output / f"{prefix}process-{process}.stderr.log").write_text(result.stderr, encoding="utf-8")
                (baseline_estimates if label == "baseline" else estimates).append(
                    validate(result.stdout, routes))
            print(f"process {process + 1}/{args.runs} complete", flush=True)
        require(before == source_state(), "source changed during measurement")
        require(metadata["binary_sha256"] == digest(binary), "binary changed during measurement")
        if baseline:
            require(baseline["binary_sha256"] == digest(old_binary), "baseline image changed")
        summary = {}
        lines = ["# Managed fold characterization", "",
                 "Median of process medians; ranges are process medians, not confidence intervals.",
                 "", "| Route | ns/item | Range | us/fold |",
                 "| --- | ---: | ---: | ---: |"]
        for route in routes:
            values = [estimate[route] for estimate in estimates]
            middle = statistics.median(values)
            summary[route] = {"median_ns_per_item": middle,
                              "min_process_median": min(values),
                              "max_process_median": max(values)}
            if baseline:
                old = [estimate[route] for estimate in baseline_estimates]
                ratio = statistics.median(new / prior for new, prior in zip(values, old))
                summary[route].update(baseline_ns_per_item=statistics.median(old), paired_ratio=ratio)
            lines.append(f"| {route} | {middle:.3f} | {min(values):.3f}-{max(values):.3f} "
                         f"| {middle * ITEMS / 1000:.3f} |")
        comparisons = {}
        for candidate, baseline in (("managed_rust_scalar", "rust"),
                                    ("managed_rust_references", "managed_rust_scalar"),
                                    ("scalar", "bridge"), ("references", "bridge"),
                                    ("references_gc", "bridge_gc"),
                                    ("production", "scalar"),
                                    ("production_gc", "references_gc"),
                                    ("bridge_prepared", "bridge"),
                                    ("bridge_raw", "bridge"),
                                    ("bridge_raw_values", "bridge_raw"),
                                    ("script_loop", "script_sum")):
            if candidate not in routes or baseline not in routes:
                continue
            ratios = [e[candidate] / e[baseline] for e in estimates]
            comparisons[f"{candidate}/{baseline}"] = statistics.median(ratios)
        report = {"metadata": metadata, "process_estimates": estimates,
                  "summary": summary, "paired_ratios": comparisons}
        if baseline:
            report.update(baseline_metadata=baseline, baseline_process_estimates=baseline_estimates)
            lines.extend(["", "| Route | Rerun baseline ns/item | Paired change |",
                          "| --- | ---: | ---: |"])
            for route in routes:
                values = summary[route]
                lines.append(f"| {route} | {values['baseline_ns_per_item']:.3f} "
                             f"| {(values['paired_ratio'] - 1) * 100:+.2f}% |")
        (output / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
        (output / "report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
        state_path.write_text(json.dumps({"status": "complete"}), encoding="utf-8")
        print("\n".join(lines), flush=True)
    except Exception as error:
        state_path.write_text(json.dumps({"status": "failed", "reason": str(error)}),
                              encoding="utf-8")
        raise


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--diagnose", action="store_true")
    parser.add_argument("--counts", action="store_true")
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--sample", choices=DIAGNOSE_ROUTES, action="append")
    parser.add_argument("--output", type=Path, required=True)
    run(parser.parse_args())
