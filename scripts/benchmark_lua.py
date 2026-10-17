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
import shutil
import statistics
import subprocess
import time

import benchmark_reference as reference
from benchmark_results import process_summary, validate_baseline, validate_extension

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
    elif platform.system() == "Darwin":
        info.update(cpu=command("sysctl", "-n", "machdep.cpu.brand_string"),
                    memory_bytes=int(command("sysctl", "-n", "hw.memsize")),
                    os_version=command("sw_vers", "-productVersion"))
    return info


def summarize(rows: list[dict]) -> list[dict]:
    processes = defaultdict(list)
    for row in rows:
        processes[(row["phase"], row["workload"], row["engine"], row["run"])].append(row["ns"] / row["batch"])
    grouped = defaultdict(list)
    for (*key, _run), values in processes.items():
        grouped[tuple(key)].append(statistics.median(values))
    return [
        {"phase": phase, "workload": workload, "engine": engine, "processes": len(values),
         "median_ns": statistics.median(values), "min_ns": min(values), "max_ns": max(values)}
        for (phase, workload, engine), values in sorted(grouped.items())
    ]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="One checked sample per route, without warmup")
    parser.add_argument("--runs", type=int, default=2, help="Sequential fresh processes; default: 2")
    parser.add_argument("--interpreter-only", action="store_true",
                        help="Skip native backend preparation and execution")
    parser.add_argument("--workload", help="Measure only one workload from the original suite")
    parser.add_argument("--numeric-matrix", action="store_true",
                        help="Measure bounded numeric/bit/cast loops in every numeric domain")
    parser.add_argument("--source-forms", action="store_true",
                        help="Measure recurrence forms, byte state, strings and host callbacks")
    parser.add_argument("--execution-controls", action="store_true",
                        help="Measure parked owned roots and ordered retirement separately")
    parser.add_argument("--window-controls", action="store_true",
                        help="Measure private runtime window banks using the release test harness")
    parser.add_argument("--baseline-executable", type=Path,
                        help="Interleave the selected unchanged workloads with a saved release binary")
    parser.add_argument("--baseline-report", type=Path,
                        help="Frozen single-variant report matching the baseline binary and matrix")
    parser.add_argument("--freeze-baseline", type=Path,
                        help="Save this ordinary binary and matrix report after successful measurements")
    parser.add_argument("--extend-report", type=Path,
                        help="Retain a prior paired report and extend it to --runs total pairs")
    parser.add_argument("--freeze-lua-reference", type=Path,
                        help="Build/freeze the independent Lua setup reference, then exit")
    parser.add_argument("--lua-setup-reference", type=Path,
                        help="Frozen independent reference required for paired original setup controls")
    args = parser.parse_args()
    if args.freeze_lua_reference:
        if any((args.check, args.workload, args.numeric_matrix, args.source_forms,
                args.execution_controls, args.window_controls, args.baseline_executable,
                args.baseline_report, args.freeze_baseline, args.extend_report,
                args.lua_setup_reference, args.interpreter_only, args.runs != 2)):
            parser.error("freeze the Lua setup reference separately from a measurement")
        reference.freeze(ROOT, args.freeze_lua_reference)
        return
    if args.numeric_matrix or args.source_forms or args.execution_controls or args.window_controls or args.baseline_executable:
        args.interpreter_only = True
    if args.workload and (args.numeric_matrix or args.source_forms or args.execution_controls or args.window_controls):
        parser.error("--workload cannot be combined with an expanded matrix")
    if sum((args.numeric_matrix, args.source_forms, args.execution_controls, args.window_controls)) > 1:
        parser.error("select one expanded matrix")
    if args.window_controls and args.check:
        parser.error("window controls have a fixed ordinary sample contract")
    if args.runs < 1:
        parser.error("--runs must be positive")
    if bool(args.baseline_executable) != bool(args.baseline_report):
        parser.error("paired execution requires both --baseline-executable and --baseline-report")
    if args.extend_report and (not args.baseline_executable or args.check):
        parser.error("extend an ordinary paired run with the same binary and configuration")
    if args.freeze_baseline and (args.baseline_executable or args.check or args.numeric_matrix):
        parser.error("freeze an ordinary unpaired original/forms run")
    if args.freeze_baseline:
        args.interpreter_only = True
    if args.baseline_executable and args.numeric_matrix:
        parser.error("numeric diagnostics are outside paired interpreter acceptance")
    original_pair = bool(args.baseline_executable) and not any((args.numeric_matrix,
                         args.source_forms, args.execution_controls, args.window_controls))
    if bool(args.lua_setup_reference) != original_pair:
        parser.error("paired original measurements require --lua-setup-reference; other suites do not use it")
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    suffix = "-controls" if args.execution_controls else "-forms" if args.source_forms else "-numeric" if args.numeric_matrix else ""
    if args.window_controls:
        suffix = "-windows"
    if args.baseline_executable:
        suffix += "-paired"
    output = ROOT / "target/lua-comparison" / (stamp + suffix + ("-check" if args.check else ""))
    output.mkdir(parents=True, exist_ok=True)
    metadata = {"timestamp_utc": stamp, "revision": command("git", "rev-parse", "HEAD"),
                "git_status": command("git", "status", "--short"),
                "rustc": command("rustc", "-Vv"), "cargo": command("cargo", "-V"),
                "machine": machine(), "profile": "release (workspace defaults, opt-level=3)",
                "features": "kagari source,native; mlua lua54,vendored",
                "parallelism": "Cargo default; benchmark single thread; processes sequential",
                "cache": "Rust build reused when available; fresh process/state per run; warmed execution measured separately from setup",
                "samples_per_process": 1 if args.check else 11,
                "warmups_per_route": 0 if args.check else 3,
                "setup_samples_per_workload": 1 if args.check or args.numeric_matrix or args.source_forms or args.execution_controls else 3,
                "workloads": "owned execution controls" if args.execution_controls else "source forms, byte state, strings and host callbacks" if args.source_forms else "numeric matrix" if args.numeric_matrix else args.workload or "original seven",
                "setup_scope": "excluded; lifecycle only" if args.execution_controls else "one matrix module per process" if args.numeric_matrix or args.source_forms else "per workload",
                "runs": args.runs, "check": args.check,
                "paired": bool(args.baseline_executable),
                "suite": "controls" if args.execution_controls else "forms" if args.source_forms else "numeric" if args.numeric_matrix else "original",
                "workload": args.workload,
                "interpreter_only": args.interpreter_only,
                "environment": {key: os.environ[key] for key in (
                    "CARGO_BUILD_JOBS", "CARGO_TARGET_DIR", "RUSTFLAGS", "CFLAGS", "CC", "DEVELOPER_DIR"
                ) if key in os.environ}}
    if args.window_controls:
        metadata.update(suite="windows", workloads="private window controls",
                        features="kagari-runtime default features; library test harness",
                        setup_samples_per_workload=1, setup_scope="excluded; storage only")
    sources = ROOT / "benchmarks/lua-comparison"
    metadata["source_sha256"] = {
        path.relative_to(ROOT).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(sources.rglob("*"))
        if path.is_file() and path.suffix in {".rs", ".kgr", ".lua", ".toml"}
        and path.relative_to(ROOT).as_posix() != reference.SOURCE
    }
    if args.window_controls:
        probe = Path("crates/kagari-runtime/src/frame/values/tests/measurements.rs")
        metadata["source_sha256"][probe.as_posix()] = hashlib.sha256((ROOT / probe).read_bytes()).hexdigest()
    metadata["driver_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    metadata["lock_sha256"] = hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest()
    if args.lua_setup_reference:
        metadata["lua_setup_reference"] = reference.load(ROOT, args.lua_setup_reference, metadata)
    print(f"Output: {output}", flush=True)
    previous = None
    if args.extend_report:
        previous = json.loads(args.extend_report.read_text(encoding="utf-8"))
        original_path = previous["metadata"].get("candidate_executable")
        if not original_path:
            parser.error("extension report lacks the original candidate image path")
        executable = Path(original_path).resolve()
        if not executable.is_file():
            parser.error("original candidate image no longer exists; cannot extend this experiment")
        metadata["build_wall_seconds"] = 0
        metadata["cache"] = "original candidate image path; no rebuild; fresh process/state; warmed execution separate from setup"
    else:
        start = time.perf_counter()
        with (output / "build.log").open("w", encoding="utf-8") as log:
            build = (["cargo", "test", "--release", "--locked", "-p", "kagari-runtime", "--lib",
                      "--no-run", "--message-format=json"] if args.window_controls else
                     ["cargo", "build", "--release", "--locked", "-p", "kagari-lua-benchmark",
                      "--bin", "kagari-lua-benchmark"])
            subprocess.run(build,
                           cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True)
        metadata["build_wall_seconds"] = time.perf_counter() - start
        executable = ROOT / "target/release" / ("kagari-lua-benchmark.exe" if os.name == "nt" else "kagari-lua-benchmark")
        if args.window_controls:
            artifacts = [json.loads(line) for line in (output / "build.log").read_text(encoding="utf-8").splitlines()
                         if line.startswith('{"reason":')]
            executable = Path(next(item["executable"] for item in artifacts
                                   if item.get("executable") and item["target"]["name"] == "kagari_runtime"))
    executable = executable.resolve()
    metadata["candidate_executable"] = str(executable)
    metadata["binary_sha256"] = hashlib.sha256(executable.read_bytes()).hexdigest()
    if args.baseline_executable:
        args.baseline_executable = args.baseline_executable.resolve()
        metadata["baseline_executable"] = str(args.baseline_executable)
        metadata["baseline_binary_sha256"] = hashlib.sha256(args.baseline_executable.read_bytes()).hexdigest()
        metadata["process_order"] = "baseline,candidate,candidate,baseline (alternating by pair)"
        baseline = json.loads(args.baseline_report.read_text(encoding="utf-8"))
        validate_baseline(metadata, baseline)
    rows = []
    reference_rows = []
    reference_processes = []
    start_pair = 0
    if previous is not None:
        start_pair = validate_extension(metadata, previous)
        if previous["baseline"] != baseline:
            parser.error("extension must retain the exact frozen baseline report")
        rows.extend(previous["raw"])
        if args.lua_setup_reference:
            reference_rows.extend(previous["reference_raw"])
            reference_processes.extend(previous["reference_processes"])
        metadata["continued_report_sha256"] = hashlib.sha256(args.extend_report.read_bytes()).hexdigest()
        metadata["continued_metadata"] = previous["metadata"]
        metadata.update({key: value for key, value in previous["metadata"].items() if key.startswith("run_")})
    routes = []
    for pair in range(start_pair, args.runs):
        variants = ["baseline", "candidate"] if pair % 2 == 0 else ["candidate", "baseline"]
        routes.extend((pair, variant) for variant in (variants if args.baseline_executable else ["candidate"]))
    first_run = start_pair * 2
    for run, (pair, variant) in enumerate(routes, start=first_run):
        argv = [str(args.baseline_executable if variant == "baseline" else executable)]
        if args.check:
            argv.append("--check")
        if args.interpreter_only:
            argv.append("--interpreter-only")
        if args.workload:
            argv.append(f"--workload={args.workload}")
        if args.numeric_matrix:
            argv.append("--numeric-matrix")
        if args.source_forms:
            argv.append("--source-forms")
        if args.execution_controls:
            argv.append("--execution-controls")
        if pair % 2:
            argv.append("--reverse")
        if args.window_controls:
            argv = [argv[0], "frame::values::tests::measurements::window_storage_timing",
                    "--exact", "--ignored", "--nocapture", "--test-threads=1"]
        start = time.perf_counter()
        with (output / f"run-{run}.csv").open("w", encoding="utf-8", newline="") as stdout, \
                (output / f"run-{run}.stderr.log").open("w", encoding="utf-8") as stderr:
            subprocess.run(argv, cwd=ROOT, stdout=stdout, stderr=stderr, check=True)
        metadata[f"run_{run}_wall_seconds"] = time.perf_counter() - start
        with (output / f"run-{run}.csv").open(encoding="utf-8", newline="") as raw:
            measured = csv.DictReader(raw) if not args.window_controls else window_rows(raw)
            subject_rows = []
            for row in measured:
                row.update({key: int(row[key]) for key in ("size", "batch", "sample", "ns", "checksum")})
                row["run"] = run
                row["pair"] = pair
                row["variant"] = variant
                rows.append(row)
                subject_rows.append(row)
        if args.lua_setup_reference:
            samples, provenance = reference.measure(ROOT, output, metadata, subject_rows, run, pair, variant)
            reference_rows.extend(samples)
            reference_processes.extend(provenance)
        print(f"Run {run + 1}/{first_run + len(routes)} ({variant}): all checksums pass", flush=True)
    summary = []
    for variant in sorted({row["variant"] for row in rows}):
        summary.extend(dict(row, variant=variant) for row in summarize([row for row in rows if row["variant"] == variant]))
    report = {"metadata": metadata, "summary": summary, "raw": rows}
    if args.lua_setup_reference:
        report.update(reference_raw=reference_rows, reference_processes=reference_processes)
    if args.interpreter_only and not args.numeric_matrix:
        report["processes"] = process_summary(report)
    if args.baseline_executable:
        report["baseline"] = baseline
    if args.lua_setup_reference:
        report["reference_summary"] = reference.summaries(report)
    (output / "results.json").write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    if args.freeze_baseline:
        frozen = args.freeze_baseline.resolve()
        if frozen == executable.resolve():
            parser.error("freeze destination must be separate from the Cargo output")
        frozen.parent.mkdir(parents=True, exist_ok=True)
        if frozen.exists() and hashlib.sha256(frozen.read_bytes()).hexdigest() != metadata["binary_sha256"]:
            parser.error("freeze destination contains a different binary; use a new checkpoint path")
        snapshot = Path(str(frozen) + f".{metadata['suite']}.json")
        if snapshot.exists():
            parser.error("frozen matrix report already exists; preserve it and select another path")
        if not frozen.exists():
            shutil.copy2(executable, frozen)
        snapshot.write_text(json.dumps(report, indent=2), encoding="utf-8")
        print(f"Frozen baseline: {frozen}; report: {snapshot}")
    if args.execution_controls or args.window_controls:
        for row in summary:
            print(f"{row['variant']} {row['workload']} {row['phase']}: {row['median_ns'] / 1000:.3f} us")
        print(f"Build wall time (excluded from execution): {metadata['build_wall_seconds']:.3f}s")
        return
    print("\nExecution median per complete workload (microseconds):")
    results = {(row["workload"], row["engine"]): row["median_ns"] / 1_000
               for row in summary if row["phase"] == "execute" and row["variant"] == "candidate"}
    if args.workload:
        names = (args.workload,)
    elif args.numeric_matrix or args.source_forms:
        names = sorted({name for name, engine in results if engine == "kagari_vm"})
    else:
        names = ("entry", "arithmetic", "branches", "calls", "fibonacci", "arrays", "maps")
    for name in names:
        vm, lua = results[name, "kagari_vm"], results[name, "lua54"]
        jit = results.get((name, "kagari_jit"))
        print(f"{name}: Kagari VM={vm:.3f}; Lua={lua:.3f}; VM/Lua={vm/lua:.2f}; "
              f"Kagari JIT={f'{jit:.3f}' if jit is not None else ('disabled' if args.interpreter_only else 'unsupported')}")
    print(f"Build wall time (excluded from execution): {metadata['build_wall_seconds']:.3f}s")


def window_rows(lines):
    """Discard libtest framing, retaining each fixed timing region as a normal row."""
    for line in lines:
        if "WINDOW," not in line:
            continue
        depth, width, order, batch, sample, admission, retirement, fragmentation = line.partition("WINDOW,")[2].strip().split(",")
        for phase, ns in (("admit", admission), ("retire", retirement), ("fragment", fragmentation)):
            yield dict(phase=phase, workload=f"windows_{depth}_{width}_{order}", engine="kagari_vm",
                       size=depth, batch=batch, sample=sample, ns=ns, checksum=0)


if __name__ == "__main__":
    main()
