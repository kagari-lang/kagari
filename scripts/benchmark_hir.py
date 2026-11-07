"""Fixed HIR process runner. Examples (from the repository root):

uv run python scripts/benchmark_hir.py --check --workload plain --size 96
uv run python scripts/benchmark_hir.py --output target/hir-before
uv run python scripts/benchmark_hir.py --baseline target/hir-before --output target/hir-after
uv run python scripts/benchmark_hir.py --compare target/hir-before target/hir-after

Normal runs build both release benchmarks, freeze their images, then run only those
copies. Baseline mode re-executes a frozen image in fresh alternating AB/BA pairs;
saved-only comparison reports unpaired medians. No performance acceptance budget.
"""
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
import subprocess
import sys
import time

from hir_benchmark_results import (
    WORKLOADS, aggregate, case_key, compare_saved, require, validate_manifests,
    validate_metadata, validate_report, validate_run,
)

ROOT = Path(__file__).resolve().parents[1]
BENCHES = {"timing": "hir_baseline", "memory": "hir_memory"}
PROTOCOL = {
    "execution": "serial independent fresh processes; one suite per process",
    "timing_warmups": 1,
    "memory_warmups": 0,
    "check_warmups": 0,
    "pair_order": "AB on even sample indices, BA on odd; A=baseline, B=candidate",
    "cache": "driver-defined fresh databases; OS caches uncontrolled; no cache flushing",
    "memory": "process-cumulative counts; peak window starts at baseline live bytes; never speed evidence",
}


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def object_sha(value) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()).hexdigest()


def decode(text: str):
    def invalid(value):
        raise ValueError(f"nonfinite JSON number: {value}")

    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, f"duplicate JSON key: {key}")
            result[key] = value
        return result

    return json.loads(text, parse_constant=invalid, object_pairs_hook=unique)


def save(path: Path, value) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    # All destinations are new campaign files; never overwrite a frozen run.
    with path.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, indent=2, sort_keys=True, allow_nan=False)
        stream.write("\n")


def command(args: list[str], timeout: float) -> subprocess.CompletedProcess:
    result = subprocess.run(args, cwd=ROOT, capture_output=True, text=True,
                            encoding="utf-8", errors="strict", timeout=timeout, check=False)
    if result.returncode:
        raise ValueError(f"command failed ({result.returncode}): {args!r}\n{result.stderr}\n{result.stdout}")
    return result


def source_state(timeout: float) -> dict:
    names = command(["git", "--no-pager", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], timeout).stdout.split("\0")
    relevant = sorted({name for name in names if name and (
        Path(name).suffix in (".rs", ".kgr") or Path(name).name in
        ("Cargo.toml", "Cargo.lock", "rust-toolchain", "rust-toolchain.toml")
        or name.startswith(".cargo/")
        or name in ("scripts/benchmark_hir.py", "scripts/hir_benchmark_results.py"))})
    files = {name: sha(ROOT / name) if (ROOT / name).is_file() else "deleted" for name in relevant}
    harness = {name: value for name, value in files.items() if name.startswith("crates/kagari-hir/benches/hir_baseline/")}
    require(len(harness) >= 7, "HIR harness sources are missing")
    tooling = {name: value for name, value in files.items() if name.startswith("scripts/")}
    diff = command(["git", "--no-pager", "diff", "--no-ext-diff", "--no-textconv", "--binary", "HEAD", "--",
                    "*.rs", "*.kgr", "**/Cargo.toml", "Cargo.toml", "Cargo.lock", ".cargo",
                    "rust-toolchain*", "scripts/benchmark_hir.py", "scripts/hir_benchmark_results.py"], timeout).stdout
    return {"files": files, "source_sha256": object_sha(files), "harness_sha256": object_sha(harness),
            "tooling_sha256": object_sha(tooling), "lock_sha256": sha(ROOT / "Cargo.lock"),
            "dirty_diff_sha256": hashlib.sha256(diff.encode()).hexdigest(),
            "git_revision": command(["git", "--no-pager", "rev-parse", "HEAD"], timeout).stdout.strip()}


def environment() -> dict:
    explicit = {"PATH", "CC", "CXX", "AR", "CFLAGS", "CXXFLAGS", "LDFLAGS", "LD_LIBRARY_PATH",
                "DYLD_LIBRARY_PATH", "LD_PRELOAD", "DYLD_INSERT_LIBRARIES", "MACOSX_DEPLOYMENT_TARGET",
                "SDKROOT", "LIB", "LIBPATH", "INCLUDE", "CL", "_CL_", "NUMBER_OF_PROCESSORS",
                "PROCESSOR_IDENTIFIER", "PROCESSOR_ARCHITECTURE", "RAYON_NUM_THREADS"}
    return {key: value for key, value in sorted(os.environ.items()) if key in explicit or key.startswith(("CARGO_", "RUST", "SCCACHE_"))}


def cargo_configs() -> dict:
    # Cargo also reads ancestor and user configs. Record their contents, not just names.
    directories = [ROOT, *ROOT.parents]
    home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    paths = [directory / ".cargo" / name for directory in directories for name in ("config", "config.toml")]
    paths += [home / name for name in ("config", "config.toml")]
    return {str(path.resolve()): sha(path) for path in paths if path.is_file()}


def machine_metadata(timeout: float) -> dict:
    processor = os.environ.get("PROCESSOR_IDENTIFIER") or platform.processor()
    if not processor and Path("/proc/cpuinfo").is_file():
        processor = next((line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines()
                          if line.startswith("model name")), "")
    if not processor and sys.platform == "darwin":
        processor = command(["sysctl", "-n", "machdep.cpu.brand_string"], timeout).stdout.strip()
    require(bool(processor), "cannot identify CPU; refusing an unidentifiable benchmark machine")
    return {"machine": platform.node(), "platform": platform.platform(), "processor": processor,
            "logical_cpus": os.cpu_count(), "rustc": command(["rustc", "-Vv"], timeout).stdout.strip(),
            "cargo": command(["cargo", "-Vv"], timeout).stdout.strip(), "environment": environment(),
            "cargo_config_sha256": cargo_configs()}


def build(timeout: float, output: Path) -> tuple[dict, dict]:
    args = ["cargo", "build", "--release", "--locked", "-p", "kagari-hir",
            "--bench", "hir_baseline", "--bench", "hir_memory", "--message-format=json-render-diagnostics"]
    start = time.perf_counter()
    result = command(args, timeout)
    elapsed = time.perf_counter() - start
    (output / "build.stdout.jsonl").write_text(result.stdout, encoding="utf-8")
    (output / "build.stderr.txt").write_text(result.stderr, encoding="utf-8")
    artifacts, configurations = {}, {}
    for line in result.stdout.splitlines():
        message = decode(line)
        if message.get("reason") != "compiler-artifact":
            continue
        configuration = {"package": message["package_id"], "target": message["target"]["name"],
                         "kind": message["target"]["kind"], "features": sorted(message["features"]),
                         "profile": message["profile"]}
        configurations[object_sha(configuration)] = configuration
        if message.get("executable"):
            for mode, bench in BENCHES.items():
                if message["target"]["name"] == bench and "bench" in message["target"]["kind"]:
                    require(mode not in artifacts, f"duplicate Cargo executable: {bench}")
                    artifacts[mode] = message
    require(set(artifacts) == set(BENCHES), "Cargo did not report both benchmark executables")
    resolved = [configurations[key] for key in sorted(configurations)]
    return artifacts, {"command": args, "duration_seconds": elapsed,
                       "resolved_configuration": resolved,
                       "cache": "Cargo default incremental/reuse state; not forced clean"}


def archive(source: Path, output: Path, image: str, mode: str, expected: str | None = None) -> dict:
    before = sha(source)
    require(expected is None or before == expected, "frozen executable hash mismatch")
    destination = output / "images" / image / source.name
    destination.parent.mkdir(parents=True, exist_ok=True)
    require(not destination.exists(), "archive destination already exists")
    shutil.copy2(source, destination)
    require(sha(source) == sha(destination) == before, "executable changed during archival")
    return {"path": destination.relative_to(output).as_posix(), "sha256": before, "bench": BENCHES[mode]}


def image_path(directory: Path, image: dict) -> Path:
    path = (directory / image["path"]).resolve()
    require(path.is_relative_to(directory.resolve()), "archived executable escapes run directory")
    require(sha(path) == image["sha256"], "archived executable hash mismatch")
    return path


def invoke(directory: Path, image: dict, case: dict, timeout: float, *, inputs=False, check=False) -> dict:
    path = image_path(directory, image)
    args = [str(path), "--workload", case["workload"], "--size", str(case["size"])]
    if inputs:
        args.append("--inputs")
    if check:
        args.append("--check")
    result = command(args, timeout)
    image_path(directory, image)
    require(not result.stderr.strip(), f"unexpected driver stderr: {result.stderr}")
    return decode(result.stdout)


def load_run(directory: Path) -> dict:
    run = decode((directory / "run.json").read_text(encoding="utf-8"))
    require(run["complete"] is True, "incomplete frozen campaign")
    validate_run(run)
    require(run["source_before"] == run["source_after"], "source changed during frozen campaign")
    state = run["source_before"]
    require(object_sha(state["files"]) == state["source_sha256"], "invalid source fingerprint")
    for key in ("source_sha256", "harness_sha256", "tooling_sha256", "lock_sha256"):
        require(run["metadata"][key] == state[key], f"source metadata mismatch: {key}")
    harness = {name: value for name, value in state["files"].items()
               if name.startswith("crates/kagari-hir/benches/hir_baseline/")}
    tooling = {name: value for name, value in state["files"].items() if name.startswith("scripts/")}
    require(object_sha(harness) == state["harness_sha256"] and object_sha(tooling) == state["tooling_sha256"]
            and state["files"]["Cargo.lock"] == state["lock_sha256"], "inconsistent source/harness fingerprint")
    for filename, value in (("metadata.json", run["metadata"]), ("images.json", run["images"]),
                            ("manifests.json", run["manifests"]), ("source-before.json", state),
                            ("source-after.json", run["source_after"])):
        require(decode((directory / filename).read_text(encoding="utf-8")) == value,
                f"archive metadata mismatch: {filename}")
    for images in run["images"].values():
        require(set(images) == set(BENCHES), "missing frozen executable")
        for mode, image in images.items():
            require(image["bench"] == BENCHES[mode], "incorrect frozen target")
            image_path(directory, image)
    for position, sample in enumerate(run["samples"]):
        saved = decode((directory / "samples" / f"{position:04d}.json").read_text(encoding="utf-8"))
        require(saved == sample, "raw sample/archive mismatch")
        for image, report in sample["reports"].items():
            raw = decode((directory / "raw" / f"{position:04d}-{image}.json").read_text(encoding="utf-8"))
            require(raw == report, "raw report/archive mismatch")
    return run


def selection(args) -> list[dict]:
    workloads = list(dict.fromkeys(args.workload or WORKLOADS))
    if args.size is not None and args.size != 96 and "dependency_edit" in workloads:
        raise ValueError("dependency_edit supports only --size 96; select --workload plain and/or generic for other sizes")
    modes = ("timing", "memory") if args.mode == "both" else (args.mode,)
    cases = []
    for mode in modes:
        for workload in workloads:
            sizes = [args.size] if args.size else ([96] if args.check or mode == "memory" or workload == "dependency_edit" else [24, 96, 384])
            cases.extend({"mode": mode, "workload": workload, "size": size} for size in sizes)
    return cases


def output_directory(value: str | None, *, prefix: str = "run") -> Path:
    target = (ROOT / "target").resolve()
    path = Path(value) if value else target / "hir-benchmark" / f'{prefix}-{datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")}'
    if not path.is_absolute():
        path = ROOT / path
    path = path.resolve()
    require(path.is_relative_to(target) and path != target, "--output must be a new directory under target/")
    require(not path.exists(), "--output already exists; frozen runs are never overwritten")
    path.mkdir(parents=True)
    return path


def print_summary(summary: dict, output: Path) -> None:
    print(summary["comparison"])
    print(f"Raw vectors, median/MAD/min/max: {output / 'summary.json'}")
    for warning in summary["warnings"]:
        print(f"WARNING: {warning}")
    for row in summary["rows"]:
        value = row["candidate"]
        suffix = ""
        if "paired_candidate_over_baseline" in row:
            ratios = row["paired_candidate_over_baseline"]
            suffix = f" paired C/B={ratios['median']:.4g}" if ratios else " paired C/B=undefined"
        if "unpaired_median_candidate_over_baseline" in row:
            ratio = row["unpaired_median_candidate_over_baseline"]
            suffix = f" unpaired median C/B={ratio:.4g}" if ratio is not None else " unpaired median C/B=undefined"
        print(f"{row['case']} {row['metric']}: median={value['median']:.6g} MAD={value['mad']:.6g} "
              f"range=[{value['min']:.6g}, {value['max']:.6g}]{suffix}")


def campaign(args) -> None:
    cases = selection(args)
    baseline_directory = Path(args.baseline).resolve() if args.baseline else None
    baseline = load_run(baseline_directory) if baseline_directory else None
    before = source_state(args.timeout)
    machine = machine_metadata(args.timeout)
    output = output_directory(args.output)
    artifacts, build_info = build(args.timeout, output)
    print(f"Build: {build_info['duration_seconds']:.3f} seconds (excluded from measurements)", flush=True)
    require(source_state(args.timeout) == before, "source changed during build; discard this campaign")
    require(machine == machine_metadata(args.timeout), "toolchain/environment changed during build")
    images = {"candidate": {mode: archive(Path(artifact["executable"]), output, "candidate", mode)
                             for mode, artifact in artifacts.items()}}
    metadata = {**machine, "source_sha256": before["source_sha256"], "harness_sha256": before["harness_sha256"],
                "tooling_sha256": before["tooling_sha256"], "lock_sha256": before["lock_sha256"],
                "profile": {"name": "release", "resolved": [{k: v for k, v in config.items() if k != "features"}
                                                           for config in build_info["resolved_configuration"]]},
                "features": [{k: v for k, v in config.items() if k != "profile"}
                             for config in build_info["resolved_configuration"]],
                "parallelism": "Cargo default; child processes serial", "sample_protocol": PROTOCOL,
                "cases": cases, "runs": 1 if args.check else args.runs, "check_only": args.check,
                "memory_classification": "representative96" if all(c["size"] == 96 for c in cases if c["mode"] == "memory") else "explicit nonrepresentative size",
                "created_utc": datetime.now(timezone.utc).isoformat(), "build": build_info,
                "cargo_artifacts": artifacts,
                "git_status": command(["git", "--no-pager", "--no-optional-locks", "status", "--porcelain=v1"], args.timeout).stdout}
    if baseline:
        validate_metadata(baseline["metadata"], metadata)
        images["baseline"] = {mode: archive(image_path(baseline_directory, image), output, "baseline", mode, image["sha256"])
                              for mode, image in baseline["images"]["candidate"].items()}
    save(output / "metadata.json", metadata)
    save(output / "images.json", images)
    save(output / "source-before.json", before)
    manifests = {}
    for case in cases:
        key = case_key(case)
        manifests[key] = invoke(output, images["candidate"][case["mode"]], case, args.timeout, inputs=True)
        if baseline:
            require(manifests[key] == baseline["manifests"][key], "baseline/candidate input manifest mismatch")
            require(invoke(output, images["baseline"][case["mode"]], case, args.timeout, inputs=True) == manifests[key],
                    "frozen baseline input manifest changed")
    validate_manifests(manifests, cases)
    save(output / "manifests.json", manifests)
    run = {"metadata": metadata, "images": images, "source_before": before, "manifests": manifests, "samples": []}
    if baseline:
        run["baseline_metadata"] = baseline["metadata"]
    evidence_reference = {}
    for index in range(metadata["runs"]):
        for case in cases:
            key = case_key(case)
            order = (["baseline", "candidate"] if index % 2 == 0 else ["candidate", "baseline"]) if baseline else ["candidate"]
            sample = {"index": index, "case": key, "order": order, "manifest": manifests[key], "reports": {}}
            for image in order:
                report = invoke(output, images[image][case["mode"]], case, args.timeout, check=args.check)
                # Preserve the actual child output even if validation rejects it.
                save(output / "raw" / f'{len(run["samples"]):04d}-{image}.json', report)
                _, evidence = validate_report(report, manifests[key], case, args.check)
                require(evidence_reference.setdefault(key, evidence) == evidence, "semantic evidence changed during campaign")
                if baseline:
                    old = next(s for s in baseline["samples"] if s["case"] == key)
                    _, frozen_evidence = validate_report(old["reports"]["candidate"], old["manifest"], case)
                    require(evidence == frozen_evidence, "frozen/candidate semantic evidence mismatch")
                sample["reports"][image] = report
            save(output / "samples" / f'{len(run["samples"]):04d}.json', sample)
            run["samples"].append(sample)
            print(f"Validated {key}, process sample {index + 1}/{metadata['runs']} ({'/'.join(order)})", flush=True)
    for case in cases:
        for image in images.values():
            require(invoke(output, image[case["mode"]], case, args.timeout, inputs=True) == manifests[case_key(case)],
                    "input manifest changed during campaign")
    for image in images.values():
        for executable in image.values():
            image_path(output, executable)
    after = source_state(args.timeout)
    save(output / "source-after.json", after)
    require(before == after, "source changed during campaign; results are not accepted")
    require(machine == machine_metadata(args.timeout), "toolchain/environment changed during campaign")
    run["source_after"] = after
    run["complete"] = True
    validate_run(run, performance=not args.check)
    save(output / "run.json", run)
    if args.check:
        print(f"Correctness check passed for {len(cases)} cases; no performance summary/comparison. Archive: {output}")
    else:
        summary = aggregate(run)
        save(output / "summary.json", summary)
        print_summary(summary, output)


def positive_int(value: str) -> int:
    result = int(value)
    if result <= 0:
        raise argparse.ArgumentTypeError("must be positive")
    return result


def positive_seconds(value: str) -> float:
    result = float(value)
    if not math.isfinite(result) or result <= 0:
        raise argparse.ArgumentTypeError("must be finite and positive")
    return result


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    result.add_argument("--check", action="store_true", help="one correctness-only process per case; default size 96, no warmup or speed summary")
    result.add_argument("--workload", choices=WORKLOADS, action="append", help="repeat to select families (default: all)")
    result.add_argument("--size", choices=(24, 96, 384), type=int, help="override sizes; dependency_edit requires 96; default timing all scales, memory 96")
    result.add_argument("--runs", type=positive_int, default=7, help="fresh process samples per case (default: 7; --check uses 1)")
    result.add_argument("--mode", choices=("timing", "memory", "both"), default="both")
    result.add_argument("--timeout", type=positive_seconds, default=600, help="seconds per subprocess, including build (default: 600)")
    result.add_argument("--output", help="new archive directory under target/ (default: timestamped target/hir-benchmark/ directory)")
    comparison = result.add_mutually_exclusive_group()
    comparison.add_argument("--baseline", help="frozen run directory; execute its archived image in fresh alternating AB/BA pairs")
    comparison.add_argument("--compare", nargs=2, metavar=("BASELINE", "CANDIDATE"), help="compare two saved directories, unpaired; never build or execute images")
    return result


def main(argv: list[str] | None = None) -> int:
    cli = parser()
    args = cli.parse_args(argv)
    if args.check and (args.baseline or args.compare):
        cli.error("--check cannot be used for a performance comparison")
    supplied = sys.argv[1:] if argv is None else argv
    if args.compare and any(arg.split("=", 1)[0] in {"--workload", "--size", "--runs", "--mode"} for arg in supplied):
        cli.error("--compare uses saved cases/modes; selection and sample options do not apply")
    try:
        if args.compare:
            summary = compare_saved(*(load_run(Path(path).resolve()) for path in args.compare))
            output = output_directory(args.output, prefix="comparison")
            save(output / "summary.json", summary)
            print_summary(summary, output)
        else:
            campaign(args)
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError) as error:
        print(f"HIR benchmark rejected: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
