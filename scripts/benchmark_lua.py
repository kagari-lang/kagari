"""Build once, then run isolated matched Kagari/Lua measurements and save raw data."""
from __future__ import annotations

import argparse
import base64
from collections import defaultdict
import csv
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def command(*args: str) -> str:
    return subprocess.check_output(args, cwd=ROOT, text=True, encoding="utf-8").strip()


def machine() -> dict:
    info = {"platform": platform.platform(), "processor": platform.processor(),
            "logical_cpus": os.cpu_count()}
    if os.name == "nt":
        script = (
            "$cpu = Get-CimInstance Win32_Processor; "
            "$os = Get-CimInstance Win32_OperatingSystem; "
            "$infoJson = @{cpu=$cpu.Name; os=$os.Caption; version=$os.Version; "
            "memory_kib=$os.TotalVisibleMemorySize} | ConvertTo-Json -Compress; "
            "[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($infoJson))"
        )
        encoded = command("powershell", "-NoProfile", "-Command", script)
        info.update(json.loads(base64.b64decode(encoded).decode("utf-8")))
    return info


def summarize(rows: list[dict]) -> list[dict]:
    grouped = defaultdict(list)
    for row in rows:
        grouped[(row["phase"], row["workload"], row["engine"])].append(row["ns"] / row["batch"])
    return [
        {"phase": phase, "workload": workload, "engine": engine, "samples": len(values),
         "median_ns": statistics.median(values), "min_ns": min(values), "max_ns": max(values)}
        for (phase, workload, engine), values in sorted(grouped.items())
    ]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="One checked sample per route, without warmup")
    parser.add_argument("--runs", type=int, default=2, help="Sequential fresh processes; default: 2")
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs must be positive")
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    output = ROOT / "target/lua-comparison" / (stamp + ("-check" if args.check else ""))
    output.mkdir(parents=True, exist_ok=True)
    metadata = {"timestamp_utc": stamp, "revision": command("git", "rev-parse", "HEAD"),
                "git_status": command("git", "status", "--short"),
                "rustc": command("rustc", "-Vv"), "cargo": command("cargo", "-V"),
                "machine": machine(), "profile": "release (workspace defaults, opt-level=3)",
                "features": "kagari-embed source,native; mlua lua54,vendored",
                "parallelism": "Cargo default; benchmark single thread; processes sequential",
                "cache": "Rust build reused when available; fresh process/state per run; warmed execution measured separately from setup",
                "samples_per_process": 1 if args.check else 11,
                "warmups_per_route": 0 if args.check else 3,
                "setup_samples_per_workload": 1 if args.check else 3,
                "runs": args.runs, "check": args.check,
                "environment": {key: os.environ[key] for key in (
                    "CARGO_BUILD_JOBS", "CARGO_TARGET_DIR", "RUSTFLAGS", "CFLAGS", "CC"
                ) if key in os.environ}}
    sources = ROOT / "benchmarks/lua-comparison"
    metadata["source_sha256"] = {
        path.relative_to(ROOT).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(sources.rglob("*"))
        if path.is_file() and path.suffix in {".rs", ".kgr", ".lua", ".toml"}
    }
    metadata["driver_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    metadata["lock_sha256"] = hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest()
    print(f"Output: {output}", flush=True)
    start = time.perf_counter()
    with (output / "build.log").open("w", encoding="utf-8") as log:
        subprocess.run(["cargo", "build", "--release", "--locked", "-p", "kagari-lua-benchmark"],
                       cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True)
    metadata["build_wall_seconds"] = time.perf_counter() - start
    executable = ROOT / "target/release" / ("kagari-lua-benchmark.exe" if os.name == "nt" else "kagari-lua-benchmark")
    metadata["binary_sha256"] = hashlib.sha256(executable.read_bytes()).hexdigest()
    rows = []
    for run in range(args.runs):
        argv = [str(executable)]
        if args.check:
            argv.append("--check")
        if run % 2:
            argv.append("--reverse")
        start = time.perf_counter()
        with (output / f"run-{run}.csv").open("w", encoding="utf-8", newline="") as stdout, \
                (output / f"run-{run}.stderr.log").open("w", encoding="utf-8") as stderr:
            subprocess.run(argv, cwd=ROOT, stdout=stdout, stderr=stderr, check=True)
        metadata[f"run_{run}_wall_seconds"] = time.perf_counter() - start
        with (output / f"run-{run}.csv").open(encoding="utf-8", newline="") as raw:
            for row in csv.DictReader(raw):
                row.update({key: int(row[key]) for key in ("size", "batch", "sample", "ns", "checksum")})
                row["run"] = run
                rows.append(row)
        print(f"Run {run + 1}/{args.runs}: all checksums pass", flush=True)
    summary = summarize(rows)
    report = {"metadata": metadata, "summary": summary, "raw": rows}
    (output / "results.json").write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    print("\nExecution median per complete workload (microseconds):")
    results = {(row["workload"], row["engine"]): row["median_ns"] / 1_000
               for row in summary if row["phase"] == "execute"}
    for name in ("entry", "arithmetic", "branches", "calls", "fibonacci", "arrays", "maps"):
        vm, lua = results[name, "kagari_vm"], results[name, "lua54"]
        jit = results.get((name, "kagari_jit"))
        print(f"{name}: Kagari VM={vm:.3f}; Lua={lua:.3f}; VM/Lua={vm/lua:.2f}; "
              f"Kagari JIT={f'{jit:.3f}' if jit is not None else 'unsupported'}")
    print(f"Build wall time (excluded from execution): {metadata['build_wall_seconds']:.3f}s")


if __name__ == "__main__":
    main()
