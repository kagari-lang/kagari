"""Build, archive and sequentially measure the standalone GC benchmark suite."""
from __future__ import annotations

import argparse
import csv
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import time

from gc_benchmark_results import EvidenceError, SUITES, parse_run, summarize

ROOT = Path(__file__).resolve().parents[1]
SCHEMA = 1
MATCHED = ("machine", "rustc", "cargo", "environment", "measurement",
           "fixture_sha256", "analysis_sha256", "lock_sha256", "build_config_sha256",
           "cargo_config_sha256", "suites")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def tree_digest(paths: list[Path]) -> str:
    result = hashlib.sha256()
    for path in sorted(paths):
        result.update(path.relative_to(ROOT).as_posix().encode() + b"\0")
        result.update(path.read_bytes() + b"\0")
    return result.hexdigest()


def command(argv: list[str], timeout: float = 30) -> str:
    return subprocess.check_output(argv, cwd=ROOT, text=True, encoding="utf-8",
                                   errors="strict", timeout=timeout).strip()


def machine() -> dict:
    info = dict(host=platform.node(), platform=platform.platform(),
                architecture=platform.machine(), processor=platform.processor(),
                logical_cpus=os.cpu_count())
    if os.name == "nt":
        info["processor_identifier"] = os.environ.get("PROCESSOR_IDENTIFIER", "")
        details = command(["powershell", "-NoProfile", "-Command",
                           "Get-CimInstance Win32_ComputerSystem | "
                           "Select-Object Model,TotalPhysicalMemory | ConvertTo-Json -Compress"])
        info["system"] = json.loads(details)
    return info


def cargo_config_digest() -> str:
    """Fingerprint Cargo's cwd/ancestor and Cargo-home config files, without exposing secrets."""
    home = Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo")))
    if not home.is_absolute():
        home = ROOT / home
    directories = {directory / ".cargo" for directory in (ROOT, *ROOT.parents)} | {home}
    result = hashlib.sha256()
    for directory in sorted(directories):
        for name in ("config", "config.toml"):
            path = directory / name
            if path.is_file():
                result.update(str(path.resolve()).encode() + b"\0" + path.read_bytes() + b"\0")
    return result.hexdigest()


def metadata(suites: list[str], runs: int) -> dict:
    examples = ROOT / "crates/kagari-gc/examples"
    environment = {key: value for key, value in sorted(os.environ.items())
                   if key.startswith(("CARGO_PROFILE_", "CARGO_TARGET_", "CARGO_BUILD_"))
                   or key in ("CARGO_BUILD_JOBS", "CARGO_BUILD_TARGET", "CARGO_TARGET_DIR",
                              "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC", "RUSTC_WRAPPER",
                              "RUSTC_WORKSPACE_WRAPPER", "RUSTUP_TOOLCHAIN", "RUSTC_BOOTSTRAP",
                              "CARGO_HOME", "CARGO_INCREMENTAL", "CC", "CXX",
                              "CFLAGS", "CXXFLAGS", "LDFLAGS")}
    forbidden = {key for key in environment if key in ("CARGO_TARGET_DIR", "CARGO_BUILD_TARGET", "CARGO_BUILD_TARGET_DIR")
                 or key.startswith("CARGO_PROFILE_")}
    if forbidden:
        raise EvidenceError(f"use workspace profiles/default target; unset: {sorted(forbidden)}")
    status = command(["git", "--no-pager", "--no-optional-locks", "status", "--short"])
    patch = subprocess.check_output(["git", "--no-pager", "diff", "HEAD", "--binary"], cwd=ROOT,
                                    timeout=30)
    return dict(timestamp_utc=datetime.now(timezone.utc).isoformat(),
                revision=command(["git", "--no-pager", "rev-parse", "HEAD"]),
                git_status=status, tracked_diff_sha256=hashlib.sha256(patch).hexdigest(),
                gc_source_sha256=tree_digest(list((ROOT / "crates/kagari-gc/src").rglob("*.rs"))
                                            + [ROOT / "crates/kagari-gc/Cargo.toml"]),
                fixture_sha256=tree_digest(list(examples.rglob("*.rs"))),
                analysis_sha256=tree_digest([Path(__file__).resolve(),
                                            ROOT / "scripts/gc_benchmark_results.py"]),
                lock_sha256=digest(ROOT / "Cargo.lock"),
                build_config_sha256=tree_digest([ROOT / "Cargo.toml", ROOT / "crates/kagari-gc/Cargo.toml"]),
                cargo_config_sha256=cargo_config_digest(),
                rustc=command(["rustc", "-Vv"]), cargo=command(["cargo", "-V"]),
                machine=machine(), environment=environment, suites=suites, runs=runs,
                measurement=dict(profile="workspace release; default target/features/parallelism",
                                 processes="sequential; single-thread workloads; no affinity pinning",
                                 cache="build once; fresh processes; 3 warmups + 11 samples",
                                 capacity="one high-low observation per process; no internal warmup",
                                 estimator="median of process estimates; paired median of index-matched ratios",
                                 tails="nearest-rank per process; no SLO or significance claim"))


def write_json(path: Path, value: dict) -> None:
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False, allow_nan=False) + "\n",
                    encoding="utf-8")


def load_baseline(directory: Path, current: dict) -> dict:
    """Reject changed recipes/environment and incomplete or modified saved evidence."""
    report = json.loads((directory / "report.json").read_text(encoding="utf-8"))
    if report.get("schema") != SCHEMA or report.get("status") != "complete":
        raise EvidenceError("baseline is not a complete report of this schema")
    previous = report["metadata"]
    for key in MATCHED:
        if previous.get(key) != current[key]:
            raise EvidenceError(f"baseline mismatch: {key}; establish a new baseline for changed recipes/environment")
    runs = previous.get("runs")
    if type(runs) is not int or runs < 1:
        raise EvidenceError("invalid baseline process count")
    expected = {(suite, process) for suite in current["suites"] for process in range(runs)}
    observed = set()
    for record in report["processes"]:
        if record["variant"] != "candidate":
            continue
        identity = record["suite"], record["process"]
        if identity not in expected or identity in observed:
            raise EvidenceError("missing, duplicate or unexpected baseline process")
        observed.add(identity)
        path = directory / "raw" / f"{identity[0]}-candidate-{identity[1]}.csv"
        if digest(path) != record["raw_sha256"]:
            raise EvidenceError(f"baseline CSV hash mismatch: {path}")
        parse_run(identity[0], path.read_text(encoding="utf-8"), identity[1])
    if observed != expected:
        raise EvidenceError("baseline has missing candidate processes")
    for example in {SUITES[suite][0] for suite in current["suites"]}:
        filename = example + (".exe" if os.name == "nt" else "")
        binary = directory / "bin" / filename
        if digest(binary) != previous["binaries"][example]["sha256"]:
            raise EvidenceError(f"baseline executable hash mismatch: {example}")
    return report


def build(output: Path, suites: list[str], timeout: float) -> dict:
    examples = sorted({SUITES[suite][0] for suite in suites})
    argv = ["cargo", "build", "--release", "--locked", "-p", "kagari-gc", "--message-format=json"]
    for name in examples:
        argv.extend(["--example", name])
    start = time.perf_counter()
    with (output / "build.jsonl").open("w", encoding="utf-8") as stdout, \
            (output / "build.stderr.log").open("w", encoding="utf-8") as stderr:
        subprocess.run(argv, cwd=ROOT, stdout=stdout, stderr=stderr, timeout=timeout, check=True)
    binaries = {}
    for line in (output / "build.jsonl").read_text(encoding="utf-8").splitlines():
        item = json.loads(line)
        if item.get("reason") != "compiler-artifact" or not item.get("executable"):
            continue
        name = item["target"]["name"]
        if name in examples and "example" in item["target"]["kind"]:
            source = Path(item["executable"])
            saved = output / "bin" / source.name
            shutil.copy2(source, saved)
            binaries[name] = dict(file=saved.relative_to(output).as_posix(), sha256=digest(saved))
    if set(binaries) != set(examples):
        raise EvidenceError("Cargo did not produce every selected example")
    return dict(binaries=binaries, build_wall_seconds=time.perf_counter() - start,
                build_command=argv)


def measure(output: Path, binary: Path, suite: str, variant: str,
            process: int, timeout: float) -> dict:
    example, flags = SUITES[suite]
    argv = [str(binary)]
    if example in ("field_access", "arena_comparison"):
        argv.append(str(process))
    argv.extend(flags)
    path = output / "raw" / f"{suite}-{variant}-{process}.csv"
    start = time.perf_counter()
    with path.open("x", encoding="utf-8", newline="") as stdout, \
            path.with_suffix(".stderr.log").open("x", encoding="utf-8") as stderr:
        subprocess.run(argv, cwd=ROOT, stdout=stdout, stderr=stderr, timeout=timeout, check=True)
    return dict(suite=suite, variant=variant, process=process, command=argv,
                wall_seconds=time.perf_counter() - start, raw_sha256=digest(path),
                metrics=parse_run(suite, path.read_text(encoding="utf-8"), process))


def display(value) -> str:
    if value is None:
        return "unobserved"
    return f"{value:,.3f}" if isinstance(value, float) else f"{value:,}"


def render(report: dict) -> str:
    meta = report["metadata"]
    lines = ["# Standalone GC benchmark", "",
             f"Revision: `{meta['revision']}`; process repetitions: {meta['runs']}.",
             "Compilation, setup and validation are excluded from operation timers.",
             "Values are medians of process estimates; ranges are process estimate min/max, not confidence intervals.",
             "Paired change is median(new/old - 1) across matched processes. Negative favors lower-is-better metrics; positive favors throughput.",
             "Counts/work/cycles are diagnostic, not scores. Unobserved or zero-denominator comparisons are n/a.",
             "Maintenance is already included in service time. Cleanup is post-release collect, not full teardown.",
             "Memory is requested managed/metadata storage, not RSS or a complete allocation census.",
             "Small-fixture p99/max are observations, not deadlines; this suite does not establish Runtime performance.", ""]
    if meta["runs"] < 7:
        lines.extend(["**Exploratory run:** fewer than seven process repetitions; do not treat small changes as evidence.", ""])
    baselines = {(r["suite"], r["case"], r["metric"]): r for r in report["summary"]
                 if r["variant"] == "baseline"}
    for suite in meta["suites"]:
        lines.extend([f"## {suite}", "",
                      "| Case | Metric | Unit | Direction | New median [min, max] | Old median | Paired change | Observations |",
                      "| --- | --- | --- | --- | ---: | ---: | ---: | ---: |"])
        for row in report["summary"]:
            if row["suite"] != suite or row["variant"] != "candidate":
                continue
            old = baselines.get((suite, row["case"], row["metric"]))
            change = row["paired_change_percent"]
            change_text = "n/a" if change is None else f"{change:+.2f}%"
            lines.append(f"| {row['case']} | {row['metric']} | {row['unit']} | {row['direction']} | "
                         f"{display(row['median'])} [{display(row['min'])}, {display(row['max'])}] | "
                         f"{display(old['median']) if old else 'n/a'} | {change_text} | {row['observations']} |")
        lines.append("")
    return "\n".join(lines)


def save_report(output: Path, report: dict) -> None:
    (output / "report.md").write_text(render(report), encoding="utf-8")
    fields = ["suite", "case", "metric", "unit", "direction", "variant", "median", "min", "max",
              "processes", "observations", "paired_ratio", "paired_change_percent"]
    with (output / "summary.csv").open("w", encoding="utf-8", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=fields, extrasaction="ignore")
        writer.writeheader()
        writer.writerows(report["summary"])
    write_json(output / "report.json", report)


def run(args) -> Path:
    suites = [suite for suite in SUITES if not args.suite or suite in args.suite]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    (output / "bin").mkdir()
    (output / "raw").mkdir()
    print(f"Output: {output}", flush=True)
    state = dict(schema=SCHEMA, status="running", processes=[])
    try:
        meta = metadata(suites, args.runs)
        state["metadata"] = meta
        write_json(output / "state.json", state)
        baseline = args.baseline.resolve() if args.baseline else None
        previous = load_baseline(baseline, meta) if baseline else None
        meta.update(build(output, suites, args.build_timeout))
        if previous:
            meta["baseline"] = dict(directory=str(baseline), revision=previous["metadata"]["revision"],
                                    report_sha256=digest(baseline / "report.json"),
                                    binaries=previous["metadata"]["binaries"])
        write_json(output / "state.json", state)
        for process in range(args.runs):
            # Rotate suite order; each old/new pair uses the same process index.
            order = suites[process % len(suites):] + suites[:process % len(suites)]
            for suite in order:
                variants = (["baseline", "candidate"] if process % 2 == 0 else ["candidate", "baseline"]) if baseline else ["candidate"]
                for variant in variants:
                    example = SUITES[suite][0]
                    directory = baseline if variant == "baseline" else output
                    binary = directory / "bin" / (example + (".exe" if os.name == "nt" else ""))
                    print(f"  {process + 1}/{args.runs} {suite}: {variant}", flush=True)
                    state["processes"].append(measure(output, binary, suite, variant, process, args.timeout))
                    write_json(output / "state.json", state)
        final = metadata(suites, args.runs)
        for key in (*MATCHED, "gc_source_sha256", "revision", "tracked_diff_sha256", "git_status"):
            if meta[key] != final[key]:
                raise EvidenceError(f"inputs/environment changed during measurement: {key}")
        for example, saved in meta["binaries"].items():
            if digest(output / saved["file"]) != saved["sha256"]:
                raise EvidenceError(f"candidate executable changed during measurement: {example}")
        if baseline:
            load_baseline(baseline, meta)
        state.update(status="complete", summary=summarize(state["processes"]))
        save_report(output, state)
        write_json(output / "state.json", state)
    except (Exception, KeyboardInterrupt) as error:
        state.update(status="failed", error=f"{type(error).__name__}: {error}")
        write_json(output / "state.json", state)
        raise
    print(f"Report: {output / 'report.md'}", flush=True)
    highlights = {"ns_per_object", "batch_p99_ns", "step_max_ns", "assist_max_ns",
                  "peak_managed_bytes", "cleanup_ns"}
    for row in state["summary"]:
        if row["variant"] == "candidate" and row["suite"] in ("sustained", "burst") and row["metric"] in highlights:
            change = row["paired_change_percent"]
            suffix = "" if change is None else f" ({change:+.2f}%)"
            print(f"{row['suite']}/{row['case']} {row['metric']}: {display(row['median'])} {row['unit']}{suffix}")
    return output


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "target/gc-benchmark" /
                        datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ"),
                        help="New output directory; existing directories are never overwritten")
    parser.add_argument("--baseline", type=Path, help="Completed output directory; rerun its archived binaries in pairs")
    parser.add_argument("--suite", action="append", choices=tuple(SUITES), help="Select a suite; repeat to select several (default: all)")
    parser.add_argument("--runs", type=int, default=7, help="Process repetitions per suite/variant (default: 7)")
    parser.add_argument("--timeout", type=float, default=180, help="Timeout seconds per fixture process")
    parser.add_argument("--build-timeout", type=float, default=600, help="Timeout seconds for the focused release build")
    args = parser.parse_args()
    if args.runs < 1 or not 0 < args.timeout < float("inf") or not 0 < args.build_timeout < float("inf"):
        parser.error("runs and finite timeouts must be positive")
    try:
        run(args)
    except (EvidenceError, OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"GC benchmark failed: {error}; inspect state.json and logs in {args.output}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
