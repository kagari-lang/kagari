"""Validate frozen benchmark evidence and aggregate at the process/pair boundary."""
from __future__ import annotations

from collections import defaultdict
import math
from pathlib import PurePosixPath, PureWindowsPath
import re
from statistics import median

ORIGINAL = ("entry", "arithmetic", "branches", "calls", "fibonacci", "arrays", "maps")
FORMS = ("direct", "helper", "concrete_generic", "interface", "shared_generic",
         "capture_cell", "field", "native", "byte_state", "host_callback",
         "string_constants", "string_calls")
CONTROLS = tuple(f"roots_{population}_{order}_{mode}"
                 for population, order in ((1, "tail"), (32, "tail"), (256, "forward"), (256, "random"))
                 for mode in ("complete", "cancel"))
WINDOWS = ("windows_1_0_tail", "windows_32_8_tail", "windows_256_128_tail",
           "windows_256_0_random", "windows_256_8_forward", "windows_256_128_random")
MATCHED_CONFIG = ("rustc", "cargo", "machine", "profile", "features", "parallelism",
                  "environment", "workload_sha256", "lock_sha256", "suite", "workload",
                  "samples_per_process", "warmups_per_route", "setup_samples_per_workload",
                  "setup_scope", "interpreter_only", "check")
IMAGE_PATHS = ("candidate_executable", "baseline_executable")


class EvidenceError(ValueError):
    """Missing or inconsistent evidence must not produce a passing comparison."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise EvidenceError(message)


def row_key(row: dict) -> tuple[str, str, str]:
    return row["phase"], row["workload"], row["engine"]


def expected_rows(metadata: dict) -> dict[tuple[str, str, str], int]:
    """Keep the finite timing schema explicit so an entirely absent row is detected."""
    require(metadata["interpreter_only"], "acceptance requires interpreter-only evidence")
    samples = metadata["samples_per_process"]
    setup = metadata["setup_samples_per_workload"]
    require(type(samples) is int and samples > 0, "invalid execution sample count")
    require(type(setup) is int and setup == 1, "one preparation per workload is required")
    result = {}
    if metadata["suite"] == "original":
        names = (metadata["workload"],) if metadata["workload"] else ORIGINAL
        require(all(name in ORIGINAL for name in names), "unknown original workload")
        for name in names:
            for engine, phases in (
                ("kagari_vm", ("engine_init", "source_to_artifact", "artifact_prepare",
                               "runtime_init", "program_link")),
                ("lua54", ("engine_init", "source_to_chunk", "module_init")),
            ):
                result["execute", name, engine] = samples
                for phase in phases:
                    result[phase, name, engine] = setup
    elif metadata["suite"] == "forms":
        require(not metadata["workload"], "forms cannot select an original workload")
        for name in FORMS:
            for engine in ("kagari_vm", "lua54"):
                result["execute", f"forms_{name}", engine] = samples
        for phase in ("source_to_artifact", "artifact_prepare", "runtime_init", "program_link"):
            result[phase, "forms_module", "kagari_vm"] = 1
    elif metadata["suite"] in ("controls", "windows"):
        require(not metadata["workload"], "controls cannot select an original workload")
        names, phases = (WINDOWS, ("admit", "retire", "fragment")) if metadata["suite"] == "windows" else (CONTROLS, ("admit", "retire", "collect"))
        for name in names:
            for phase in phases:
                result[phase, name, "kagari_vm"] = samples
    else:
        raise EvidenceError("numeric diagnostics cannot establish interpreter acceptance")
    return result


def validate_report(report: dict) -> dict[tuple[int, str, str, str], list[dict]]:
    """Validate every process before comparing times, including setup and controls."""
    metadata = report["metadata"]
    for key in (*MATCHED_CONFIG, "binary_sha256", "revision", "runs", "paired"):
        require(key in metadata, f"missing metadata: {key}")
    require(isinstance(metadata["machine"], dict) and metadata["machine"], "missing machine evidence")
    for value in (metadata["binary_sha256"], metadata["lock_sha256"], metadata["workload_sha256"]):
        require(isinstance(value, str) and re.fullmatch("[0-9a-f]{64}", value), "invalid SHA-256 evidence")
    require(type(metadata["runs"]) is int and metadata["runs"] > 0, "invalid process count")
    require(type(metadata["paired"]) is bool, "invalid paired mode")
    if metadata["paired"]:
        for key in IMAGE_PATHS:
            path = metadata.get(key)
            require(isinstance(path, str) and bool(path)
                    and (PurePosixPath(path).is_absolute() or PureWindowsPath(path).is_absolute()),
                    f"missing absolute image path: {key}")
    schema = expected_rows(metadata)
    variants = 2 if metadata["paired"] else 1
    grouped = defaultdict(list)
    contracts = {}
    process_ids = set()
    for row in report["raw"]:
        for field in ("size", "batch", "sample", "ns", "checksum", "run", "pair"):
            require(type(row[field]) is int, f"non-integer {field}")
        run = row["run"]
        require(0 <= run < metadata["runs"] * variants, "unexpected process identity")
        pair = run // variants
        order = ("baseline", "candidate") if pair % 2 == 0 else ("candidate", "baseline")
        expected_variant = order[run % 2] if metadata["paired"] else "candidate"
        require(row["pair"] == pair and row["variant"] == expected_variant,
                "process pair/order mismatch")
        key = row_key(row)
        require(key in schema, f"unexpected row {key}")
        require(row["batch"] > 0 and row["ns"] >= 0 and row["size"] >= 0,
                "invalid timing/batch/size")
        contract = row["size"], row["batch"], row["checksum"]
        require(contracts.setdefault(key, contract) == contract,
                f"changed size/batch/checksum for {key}")
        if metadata["suite"] == "windows":
            require(row["batch"] == 32 and row["checksum"] == 0, "invalid window batch contract")
        elif row["phase"] not in ("execute", "retire"):
            require(row["batch"] == 1 and row["checksum"] == 0, "invalid setup contract")
        grouped[run, *key].append(row)
        process_ids.add(run)
    require(process_ids == set(range(metadata["runs"] * variants)), "missing process")
    for run in sorted(process_ids):
        for key, count in schema.items():
            rows = grouped.get((run, *key), [])
            require(len(rows) == count, f"missing/duplicate samples: process {run} {key}")
            require(sorted(row["sample"] for row in rows) == list(range(count)),
                    f"invalid sample identities: process {run} {key}")
    for key, contract in contracts.items():
        if key[0] == "execute":
            peer = key[0], key[1], "lua54" if key[2] == "kagari_vm" else "kagari_vm"
            require(contracts.get(peer) == contract, f"engine checksum mismatch: {key}")
    return dict(grouped)


def process_summary(report: dict) -> list[dict]:
    grouped = validate_report(report)
    result = []
    for (run, phase, workload, engine), rows in sorted(grouped.items()):
        values = [row["ns"] / row["batch"] for row in rows]
        result.append(dict(run=run, pair=rows[0]["pair"], variant=rows[0]["variant"],
                           phase=phase, workload=workload, engine=engine, samples=len(rows),
                           median_ns=median(values), min_ns=min(values), max_ns=max(values)))
    return result


def validate_baseline(current: dict, baseline: dict) -> None:
    validate_report(baseline)
    require(not baseline["metadata"]["paired"], "baseline report must be a frozen single variant")
    for key in MATCHED_CONFIG:
        if (key == "workload" and current["suite"] == "original"
                and current["workload"] in ORIGINAL and baseline["metadata"]["workload"] is None):
            continue  # A focused run can select a row from the validated full frozen matrix.
        require(current[key] == baseline["metadata"][key], f"baseline configuration mismatch: {key}")
    require(current["baseline_binary_sha256"] == baseline["metadata"]["binary_sha256"],
            "baseline binary does not match its frozen report")


def compare(report: dict, targets: dict[str, float] | None = None,
            minimum_pairs: int = 7) -> dict:
    """Check timing gates only; correctness and allocation acceptance remain separate."""
    targets = targets or {}
    for limit in targets.values():
        require(math.isfinite(limit) and 0 < limit <= 1.05, "invalid target ratio")
    metadata = report["metadata"]
    require(metadata["paired"], "comparison requires paired evidence")
    require(not metadata["check"] and metadata["runs"] >= minimum_pairs,
            "insufficient ordinary process pairs")
    require(metadata["samples_per_process"] == 11 and metadata["warmups_per_route"] == 3,
            "acceptance requires three warmups and eleven execution samples")
    baseline = report["baseline"]
    validate_baseline(metadata, baseline)
    summaries = process_summary(report)
    frozen = {row_key(row): (row["size"], row["batch"], row["checksum"]) for row in baseline["raw"]}
    for row in report["raw"]:
        require(frozen[row_key(row)] == (row["size"], row["batch"], row["checksum"]),
                "measurement differs from frozen workload contract")
    measured, diagnostics = [], []
    for row in summaries:
        selected = (row["engine"] == "kagari_vm"
                    and row["phase"] in ("execute", "admit", "retire", "collect", "fragment"))
        (measured if selected else diagnostics).append(row)
    target_names = {row["workload"] for row in measured
                    if row["phase"] in ("execute", "retire") and row["engine"] == "kagari_vm"}
    require(targets.keys() <= target_names, "target absent from selected matrix")
    gates = timing_comparisons(measured, metadata["runs"], targets)
    return dict(scope="Kagari execution/lifecycle timing only",
                passed=all(row["status"] == "pass" for row in gates), gates=gates,
                diagnostics=timing_comparisons(diagnostics, metadata["runs"]), processes=summaries)


def timing_comparisons(summaries: list[dict], pairs: int, targets: dict[str, float] | None = None) -> list[dict]:
    groups = defaultdict(dict)
    for row in summaries:
        key = row_key(row)
        groups[key][row["pair"], row["variant"]] = row["median_ns"]
    gates = []
    for (phase, workload, engine), values in sorted(groups.items()):
        before = [values[i, "baseline"] for i in range(pairs)]
        after = [values[i, "candidate"] for i in range(pairs)]
        comparison = dict(phase=phase, workload=workload, engine=engine,
                          baseline_ns=median(before), candidate_ns=median(after),
                          baseline_process_medians_ns=before, candidate_process_medians_ns=after)
        ratios = [c / b for b, c in zip(before, after)] if min(before + after) > 0 else []
        ratio = median(ratios) if ratios else None
        comparison.update(ratio=ratio, pair_ratios=ratios,
                          min_ratio=min(ratios) if ratios else None,
                          max_ratio=max(ratios) if ratios else None)
        if targets is not None:
            limit = targets.get(workload, 1.05) if phase in ("execute", "retire") else 1.05
            comparison.update(limit=limit, status="inconclusive" if ratio is None else
                              "pass" if ratio <= limit else "fail")
        gates.append(comparison)
    return gates
