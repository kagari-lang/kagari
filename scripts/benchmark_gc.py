"""Matched Kagari/Lua graph lifecycles; timing and allocator images stay separate."""
from __future__ import annotations

import argparse
import csv
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import statistics
import subprocess
import time

from benchmark_lua import ROOT, command, machine

EXPLICIT = {"points_512": 512, "points_8192": 8192, "cycles_4096": 4096}
PHASES = {"construct", "live_gc", "sparsify", "sparse_gc", "release_gc"}
ENGINES = {"kagari", "lua54"}
AUTO = {"points", "nodes"}


def timing(path: Path, check: bool = False) -> dict:
    rows = {}
    with path.open() as stream:
        for kind, engine, name, phase, size, count, median, low, high, raw in csv.reader(stream):
            assert kind == "timing" and engine in ENGINES
            key = (engine, name, phase)
            assert key not in rows
            samples = [int(value) for value in raw.split(";")]
            repetitions = 1 if check else 11
            expected_count = repetitions * (128 if phase == "auto_batch" else 1)
            assert int(count) == len(samples) == expected_count
            expected_size = (256 if phase == "auto_batch" else 32768) if name in AUTO else EXPLICIT[name]
            assert int(size) == expected_size
            ordered = sorted(samples)
            assert [int(median), int(low), int(high)] == [ordered[len(ordered) // 2], ordered[0], ordered[-1]]
            rows[key] = {"median": int(median), "p99": ordered[math.ceil(len(ordered) * .99) - 1],
                         "max": int(high), "samples_ns": samples}
    expected = {(e, n, p) for e in ENGINES for n in EXPLICIT for p in PHASES}
    expected |= {(e, n, p) for e in ENGINES for n in AUTO for p in ["auto_batch", "auto_total"]}
    assert rows.keys() == expected, "missing/unexpected timing rows"
    return rows


def memory(path: Path, check: bool = False) -> dict:
    rows = {}
    with path.open() as stream:
        for kind, engine, name, repetition, phase, value in csv.reader(stream):
            assert kind == "memory" and engine in ENGINES
            key = (engine, name, phase)
            repeat = int(repetition)
            assert repeat not in rows.setdefault(key, {})
            rows[key][repeat] = int(value)
            if phase == "teardown":
                assert int(value) == 0, "nonzero runtime teardown"
    expected = {(e, n, p) for e in ENGINES for n in EXPLICIT
                for p in PHASES | {"setup", "post_setup_peak", "lifecycle_peak", "teardown"}}
    expected |= {(e, "auto_" + n, p) for e in ENGINES for n in AUTO
                 for p in ["setup", "after_batches", "live", "empty", "post_setup_peak", "lifecycle_peak", "teardown"]}
    assert rows.keys() == expected, "missing/unexpected memory rows"
    for values in rows.values():
        assert set(values) == set(range(1 if check else 3))
    return {key: max(values.values()) for key, values in rows.items()}


def summarize(processes: list[dict], memory_mode: bool) -> list:
    result = []
    for _, name, phase in sorted(key for key in processes[0] if key[0] == "kagari"):
        metrics = ["bytes"] if memory_mode else (["median", "p99", "max"] if phase == "auto_batch" else ["median"])
        for metric in metrics:
            def values(engine):
                return [p[(engine, name, phase)] if memory_mode else p[(engine, name, phase)][metric]
                        for p in processes]
            kagari, lua = values("kagari"), values("lua54")
            ratios = [k / l for k, l in zip(kagari, lua)] if all(l > 0 for l in lua) else []
            result.append(dict(workload=name, phase=phase, metric=metric,
                               kagari=statistics.median(kagari), lua=statistics.median(lua),
                               median_pair_ratio=statistics.median(ratios) if ratios else None,
                               paired_ratios=ratios, kagari_processes=kagari, lua_processes=lua))
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runs", type=int, default=7)
    parser.add_argument("--check", action="store_true", help="Build/check both modes without performance claims")
    args = parser.parse_args()
    assert args.runs > 0
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    output = ROOT / "target/gc-comparison" / stamp
    output.mkdir(parents=True)
    print(f"Output: {output}", flush=True)
    sources = sorted((ROOT / "benchmarks/lua-comparison/examples/gc_comparison").glob("*"))
    sources += [ROOT / "scripts/benchmark_gc.py"]
    digest = hashlib.sha256()
    for path in sources:
        if path.is_file():
            digest.update(str(path.relative_to(ROOT)).encode() + b"\0" + path.read_bytes())
    metadata = dict(revision=command("git", "rev-parse", "HEAD"), status=command("git", "status", "--short"),
                    machine=machine(), rustc=command("rustc", "-Vv"), cargo=command("cargo", "-V"),
                    lock_sha256=hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest(),
                    workload_sha256=digest.hexdigest(), runs=0 if args.check else args.runs,
                    profile="workspace release", parallelism="default Cargo; sequential measurement",
                    features="Kagari source,native, interpreter only; mlua 0.12.2 lua54,vendored",
                    lua_gc="PUC 5.4 incremental: pause 200, step multiplier 100, step size 13",
                    kagari_gc="default automatic threshold 1024; explicit scenarios disable automatic collection",
                    cache="fresh runtime/state per lifecycle; shared prepared Kagari program warmed; 3 warmups, 11 measured lifecycles",
                    estimator="median within-process Kagari/Lua ratios across fixed processes; auto p99 nearest rank, upper batch median",
                    memory="3 lifecycles per process, per-metric maxima, requested allocator bytes; not RSS",
                    auto_latency="128 batches x 256 objects, 8 retained batches; call latency, not isolated GC pause")
    binaries = {}
    # Finish both builds and semantic checks before starting any timed process.
    for mode, extra in [("ordinary", []), ("memory", ["--features", "diagnostics"])]:
        build = ["cargo", "build", "--release", "--locked", "-p", "kagari-lua-benchmark",
                 "--example", "gc_comparison", *extra]
        start = time.perf_counter()
        with (output / f"build-{mode}.log").open("w") as log:
            subprocess.run(build, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True)
        metadata[f"{mode}_build_seconds"] = time.perf_counter() - start
        executable = ROOT / "target/release/examples" / ("gc_comparison.exe" if os.name == "nt" else "gc_comparison")
        binary = output / (mode + executable.suffix)
        shutil.copy2(executable, binary)
        binaries[mode] = binary
        metadata[f"{mode}_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
        with (output / f"check-{mode}.csv").open("w") as log:
            subprocess.run([str(binary), "--check"], cwd=ROOT, stdout=log, check=True)
        (memory if mode == "memory" else timing)(output / f"check-{mode}.csv", True)
        print(f"{mode}: build and semantic checks passed", flush=True)
    report = {"metadata": metadata}
    if not args.check:
        for mode, binary in binaries.items():
            processes = []
            for run in range(args.runs):
                start = time.perf_counter()
                with (output / f"{mode}-{run}.csv").open("w") as log:
                    subprocess.run([str(binary), *(["--reverse"] if run % 2 else [])], cwd=ROOT, stdout=log, check=True)
                processes.append((memory if mode == "memory" else timing)(output / f"{mode}-{run}.csv"))
                print(f"{mode} {run + 1}/{args.runs}: checked, {time.perf_counter() - start:.2f}s", flush=True)
            report[mode] = summarize(processes, mode == "memory")
    for mode, binary in binaries.items():
        assert hashlib.sha256(binary.read_bytes()).hexdigest() == metadata[f"{mode}_sha256"]
    (output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    for row in report.get("ordinary", []):
        print(f"{row['workload']}/{row['phase']}/{row['metric']}: "
              f"K={row['kagari']/1000:.3f} us Lua={row['lua']/1000:.3f} us "
              f"paired K/L={row['median_pair_ratio']:.3f}")


if __name__ == "__main__":
    main()
