"""Validate frozen benchmark evidence and aggregate at the process/pair boundary."""
from __future__ import annotations

from collections import defaultdict
import math
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
                  "environment", "source_sha256", "lock_sha256", "suite", "workload",
                  "samples_per_process", "warmups_per_route", "setup_samples_per_workload",
                  "setup_scope", "interpreter_only", "check")


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
    require(type(setup) is int and setup > 0, "invalid setup sample count")
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
        require(setup == 1, "forms setup has exactly one sample per process")
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
    require(isinstance(metadata["source_sha256"], dict) and metadata["source_sha256"]
            and isinstance(metadata["machine"], dict) and metadata["machine"], "missing fixture/machine evidence")
    for value in (metadata["binary_sha256"], metadata["lock_sha256"], *metadata["source_sha256"].values()):
        require(isinstance(value, str) and re.fullmatch("[0-9a-f]{64}", value), "invalid SHA-256 evidence")
    require(type(metadata["runs"]) is int and metadata["runs"] > 0, "invalid process count")
    require(type(metadata["paired"]) is bool, "invalid paired mode")
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


def validate_extension(current: dict, previous: dict) -> int:
    """Extend unchanged paired evidence without discarding earlier samples/order."""
    validate_report(previous)
    earlier = previous["metadata"]
    require(earlier["paired"] and not earlier["check"], "extend ordinary paired evidence only")
    require(current["runs"] > earlier["runs"], "extension must add process pairs")
    for key in (*MATCHED_CONFIG, "binary_sha256", "baseline_binary_sha256"):
        require(current[key] == earlier[key], f"extension configuration mismatch: {key}")
    validate_baseline(current, previous["baseline"])
    return earlier["runs"]


def compare(report: dict, targets: dict[str, float] | None = None,
            minimum_pairs: int = 7, margin: float = 0.01) -> dict:
    """Check timing gates only; correctness and allocation acceptance remain separate."""
    targets = targets or {}
    require(math.isfinite(margin) and 0 <= margin < 1, "invalid near-gate margin")
    for limit in targets.values():
        require(math.isfinite(limit) and 0 < limit <= 1.05, "invalid target ratio")
    metadata = report["metadata"]
    require(metadata["paired"], "comparison requires paired evidence")
    require(not metadata["check"] and metadata["runs"] >= minimum_pairs,
            "insufficient ordinary process pairs")
    require(metadata["samples_per_process"] == 11 and metadata["warmups_per_route"] == 3,
            "acceptance requires three warmups and eleven execution samples")
    require(metadata["setup_samples_per_workload"] == (3 if metadata["suite"] == "original" else 1),
            "unexpected setup sampling")
    baseline = report["baseline"]
    validate_baseline(metadata, baseline)
    summaries = process_summary(report)
    frozen = {row_key(row): (row["size"], row["batch"], row["checksum"]) for row in baseline["raw"]}
    for row in report["raw"]:
        require(frozen[row_key(row)] == (row["size"], row["batch"], row["checksum"]),
                "measurement differs from frozen workload contract")
    groups = defaultdict(dict)
    for row in summaries:
        groups[row_key(row)][row["pair"], row["variant"]] = row["median_ns"]
    target_names = {key[1] for key in groups if key[0] in ("execute", "retire") and key[2] == "kagari_vm"}
    require(targets.keys() <= target_names, "target absent from selected matrix")
    gates = []
    for (phase, workload, engine), values in sorted(groups.items()):
        before = [values[i, "baseline"] for i in range(metadata["runs"])]
        after = [values[i, "candidate"] for i in range(metadata["runs"])]
        limit = targets.get(workload, 1.05) if phase in ("execute", "retire") and engine == "kagari_vm" else 1.05
        if min(before + after) <= 0:
            aggregate = paired = None
            status = "inconclusive"
        else:
            aggregate = median(after) / median(before)
            paired = median(c / b for b, c in zip(before, after))
            worst = max(aggregate, paired)
            status = "fail" if worst > limit else "inconclusive" if worst > limit - margin else "pass"
        gates.append(dict(phase=phase, workload=workload, engine=engine, limit=limit,
                          aggregate_ratio=aggregate, paired_ratio=paired, status=status,
                          baseline_process_medians_ns=before, candidate_process_medians_ns=after))
    return dict(scope="timing only", passed=all(row["status"] == "pass" for row in gates),
                near_gate_margin=margin, gates=gates, processes=summaries)
