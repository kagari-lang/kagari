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
import tomllib

ROOT = Path(__file__).resolve().parents[1]
NAMES = ("small", "medium", "large", "trivia", "syntax_corpus", "recovery", "limits", "declarations")
VALID = NAMES[:5]


def command(*args: str) -> str:
    return subprocess.check_output(args, cwd=ROOT, text=True, encoding="utf-8").strip()


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def compare_locks(before: bytes, after: bytes) -> list[str]:
    """Allow only Rowan removal from its two consumers and newly orphaned packages."""
    old = tomllib.loads(before.decode("utf-8"))
    new = tomllib.loads(after.decode("utf-8"))
    if {k: v for k, v in old.items() if k != "package"} != {k: v for k, v in new.items() if k != "package"}:
        raise ValueError("lockfile format/metadata changed")

    def packages(lock):
        result = {(p["name"], p["version"]): p for p in lock["package"]}
        if len(result) != len(lock["package"]):
            raise ValueError("ambiguous package identity")
        return result

    def dependency(table, spelling):
        parts = spelling.split()
        matches = [key for key in table if key[0] == parts[0] and (len(parts) == 1 or key[1] == parts[1])]
        if len(matches) != 1:
            raise ValueError(f"ambiguous or missing dependency: {spelling}")
        return matches[0]

    previous, current = packages(old), packages(new)
    removed = previous.keys() - current.keys()
    rowan = {key for key in previous if key[0] == "rowan"}
    if not rowan or not rowan <= removed or current.keys() - previous.keys():
        raise ValueError("lock change must remove Rowan without adding/upgrading packages")
    reachable, pending = set(), list(rowan)
    while pending:
        key = pending.pop()
        if key in reachable:
            continue
        reachable.add(key)
        pending.extend(dependency(previous, dep) for dep in previous[key].get("dependencies", []))
    if not removed <= reachable:
        raise ValueError("unrelated package removed")
    for key, package in current.items():
        expected = {k: v for k, v in previous[key].items() if k != "dependencies"}
        actual = {k: v for k, v in package.items() if k != "dependencies"}
        expected_dependencies = [dependency(previous, dep) for dep in previous[key].get("dependencies", [])]
        if key[0] in ("kagari-syntax", "kagari-format"):
            expected_dependencies = [dep for dep in expected_dependencies if dep not in rowan]
        actual_dependencies = [dependency(current, dep) for dep in package.get("dependencies", [])]
        if actual != expected or sorted(actual_dependencies) != sorted(expected_dependencies):
            raise ValueError(f"unrelated dependency/package change: {key}")
    return [f"{name} {version}" for name, version in sorted(removed)]


def baseline_lock(image: Path, metadata: dict) -> bytes:
    archived = image.parent / "Cargo.lock"
    if archived.exists():
        data = archived.read_bytes()
    else:
        # Early TA0 archives predate lockfile copies. Recover only the exact bytes.
        data = subprocess.check_output(["git", "show", f"{metadata['revision']}:Cargo.lock"], cwd=ROOT)
        if hashlib.sha256(data).hexdigest() != metadata["lock_sha256"]:
            data = data.replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
    if hashlib.sha256(data).hexdigest() != metadata["lock_sha256"]:
        raise ValueError("cannot recover the baseline's exact lockfile")
    return data


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


def acceptance(runs: list[dict], memory: dict, confirmed: dict | None = None) -> dict:
    """Fail closed: required noise, budgets, memory and independent repeats all matter."""
    summary = aggregate(runs)
    failures, repeats, timing = [], [], []
    if len(runs) != 7 or any("baseline" not in run for run in runs):
        failures.append("seven paired processes required")
    for row in summary:
        stage, name, ratio = row["stage"], row["workload"], row["paired_ratio"]
        budget = None
        if stage == "parse_drop":
            budget = 0.80 if name in ("medium", "large", "trivia", "syntax_corpus") else 1.05
        elif stage == "parse_hir_drop":
            budget = 1.05
        elif stage == "field_read":
            budget = 1.00
        if budget is None:
            continue
        key = f"{name}/{stage}"
        noisy = row["candidate_spread"] > 0.10 or row["baseline_spread"] is None or row["baseline_spread"] > 0.10
        if noisy:
            failures.append(f"{key}: unresolved process noise")
        if ratio is None or ratio > budget:
            failures.append(f"{key}: timing budget {budget}")
        elif abs(ratio - budget) <= 0.02:
            repeats.append(key)
        timing.append({**row, "budget": budget, "noisy": noisy})
    if set(memory) != {"candidate", "baseline"}:
        raise ValueError("both memory images required")
    candidate, baseline = (validate_memory(memory[name]) for name in ("candidate", "baseline"))
    identities = {row["name"]: row for row in identity(runs[0]["candidate"])}
    memory_rows, allocations = [], []
    for name, row in candidate.items():
        previous = baseline[name]
        if any(row[key] != previous[key] for key in ("bytes", "blake3")):
            raise ValueError("memory workload mismatch")
        if name in identities and any(row[key] != identities[name][key] for key in ("bytes", "blake3")):
            raise ValueError("memory/timing workload mismatch")
        for stage, metric in (("released", "peak_bytes"), ("parsed", "live_bytes"),
                              ("parse_released", "live_bytes")):
            value, before = row[stage][metric], previous[stage][metric]
            budget = before + max(0.10 * before, 4096)
            if value > budget:
                failures.append(f"{name}/{stage}/{metric}: memory budget {budget}")
            memory_rows.append({"workload": name, "stage": stage, "metric": metric,
                                "baseline": before, "candidate": value, "budget": budget})
        for image, counts in (("candidate", row), ("baseline", previous)):
            prior = {key: 0 for key in ("requests", "requested_bytes", "frees", "freed_bytes")}
            for stage in ("parsed", "with_view", "parse_released", "released"):
                current = counts[stage]
                allocations.append({"workload": name, "image": image, "stage": stage,
                                    **{key: current[key] - prior[key] for key in prior}})
                prior = {key: current[key] for key in prior}
    if confirmed is not None:
        # The first report must independently meet budgets and noise requirements.
        if confirmed.get("failures") or not confirmed.get("timing") or not confirmed.get("memory"):
            failures.append("confirmation report did not independently pass")
    elif repeats:
        failures.append("independent complete paired repeat required")
    return {"passed": not failures, "failures": failures, "near_threshold": repeats,
            "timing": timing, "memory": memory_rows, "allocations": allocations}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    suffix = ".exe" if os.name == "nt" else ""
    parser.add_argument("--executable", type=Path, default=ROOT / f"target/release/examples/ast_baseline{suffix}")
    parser.add_argument("--baseline-executable", type=Path)
    parser.add_argument("--runs", type=int, default=7)
    parser.add_argument("--confirm-report", type=Path, help="first paired report directory for required independent repeat")
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs must be positive; acceptance requires seven pairs")
    images = {"candidate": args.executable.resolve()}
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
            Path(__file__).resolve(), *sorted((ROOT / "crates/kagari-hir/examples/ast_baseline").glob("*.rs"))]},
        "runs": args.runs,
        "scope": "input loading, source construction, correctness walks and compilation excluded",
    }
    if "baseline" in images:
        baseline_metadata = json.loads((images["baseline"].parent / "metadata.json").read_text(encoding="utf-8"))
        if baseline_metadata["executable_sha256"]["candidate"] != hashes["baseline"]:
            raise ValueError("baseline image differs from its frozen metadata")
        for key in ("rustc", "cargo", "platform", "processor", "logical_cpus", "profile", "features",
                    "parallelism", "cache", "environment"):
            if baseline_metadata[key] != metadata[key]:
                raise ValueError(f"baseline configuration mismatch: {key}")
        if baseline_metadata["lock_sha256"] != metadata["lock_sha256"]:
            removed = compare_locks(baseline_lock(images["baseline"], baseline_metadata), (ROOT / "Cargo.lock").read_bytes())
            metadata["dependency_change"] = {"policy": "Rowan removal only", "removed": removed}
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
    if "baseline" in images:
        confirmed = None
        if args.confirm_report:
            previous = args.confirm_report
            old_metadata = json.loads((previous / "metadata.json").read_text(encoding="utf-8"))
            if old_metadata["executable_sha256"] != hashes:
                raise ValueError("confirmation uses different images")
            prior_runs = [{name: json.loads((previous / f"{i:02}-{name}.json").read_text(encoding="utf-8"))
                           for name in images} for i in range(7)]
            prior_memory = {name: json.loads((previous / f"memory-{name}.json").read_text(encoding="utf-8"))
                            for name in images}
            confirmed = acceptance(prior_runs, prior_memory)
            # A missing repeat alone is not a failed first run.
            confirmed["failures"] = [f for f in confirmed["failures"] if f != "independent complete paired repeat required"]
        verdict = acceptance(runs, memory, confirmed)
        (output / "acceptance.json").write_text(json.dumps(verdict, indent=2), encoding="utf-8")
        print(json.dumps({key: verdict[key] for key in ("passed", "failures", "near_threshold")}, indent=2))
        if not verdict["passed"]:
            raise SystemExit(1)
    else:
        print("Baseline characterization and memory evidence; no candidate acceptance claim.")


if __name__ == "__main__":
    main()
