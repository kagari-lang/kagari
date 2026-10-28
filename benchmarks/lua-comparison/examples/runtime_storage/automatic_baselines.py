"""Premeasurement associations to PG00 full-cycle scenarios, never a fastest/largest search."""
from __future__ import annotations

import ast
import hashlib
import json
from pathlib import Path
import statistics

CONTROL = "host_pressure_burst_2048x2048"
BASIC = {"construct": "live_gc", "access": "live_gc", "release_boundary": "release_gc"}
PHASE_MAP = {
    "sparse_types_32x4": dict(BASIC),
    "mixed_native_128x6": dict(BASIC),
    "scalar_65536": dict(BASIC),
    "references_8192": dict(BASIC),
    "script_keys_256": dict(BASIC),
    "sparse_cycles_4096_keep64": {
        "construct": "dense_gc", "sparsify": "sparsify_gc", "release_boundary": "release_gc",
        **{f"churn_construct_{i}": f"churn_gc_{i}" for i in range(3)},
        **{f"churn_access_{i}": "sparse_live_gc" for i in range(3)},
    },
    "retained_reload_4x256": {
        "construct_reload": "retained_gc", "access": "retained_gc",
        "release_old_boundary": "release_old_gc", "release_all_boundary": "release_all_gc",
    },
}
SHAPES = {
    "sparse_types_32x4": {"nominal_types": 32, "instances_per_type": 4, "entries": 128},
    "mixed_native_128x6": {"tuples": 128, "native_payloads_per_tuple": 6},
    "scalar_65536": {"i32_elements": 65536, "vectors": 1},
    "references_8192": {"distinct_points": 8192, "vectors": 1},
    "script_keys_256": {"keys": 256, "hash_buckets": 16},
    "sparse_cycles_4096_keep64": {"dense_nodes": 4096, "retained_nodes": 64, "churn_nodes_per_round": 4096, "churn_rounds": 3},
    "retained_reload_4x256": {"generations": 4, "items_per_generation": 256, "old_generations_released": 3},
}
ASSOCIATION = {
    "baseline_revision": "50ab194b30b0218e9befe3176d6b5711ecf17a28",
    "phase_map": PHASE_MAP,
    "shapes": SHAPES,
    "absolute_only_control": CONTROL,
    "normal_bound_ns": "max(50000, paired_full_cycle_process_median_ns / 4)",
    "estimator": "median of seven paired process-maximum / bound ratios",
}


def fingerprint() -> str:
    return hashlib.sha256(json.dumps(ASSOCIATION, sort_keys=True).encode()).hexdigest()


def full_cycles(path: Path) -> dict:
    rows = {}
    for line in path.read_text().splitlines():
        if not line.startswith("family,"):
            continue
        prefix, raw = line.split(",samples_ns=", 1)
        _, family, *columns = prefix.split(",")
        fields = dict(column.split("=", 1) for column in columns)
        samples = ast.literal_eval(raw)
        if fields["samples"] != "11" or fields["warmups"] != "3" or len(samples) != 11:
            raise ValueError(f"{path}: changed baseline sampling")
        median = statistics.median(samples)
        if median != int(fields["median_ns"]) or median <= 0:
            raise ValueError(f"{path}: invalid baseline median")
        key = family, fields["phase"]
        if key in rows:
            raise ValueError(f"{path}: duplicate baseline phase")
        rows[key] = median
    return rows


def paired(report: dict, manifest_path: Path) -> dict:
    if report["mode"] != "measurement":
        raise ValueError("check runs cannot satisfy normal-step gates")
    manifest = json.loads(manifest_path.read_text())
    if manifest["association_sha256"] != fingerprint():
        raise ValueError("phase association changed after freezing")
    pairs = manifest["pairs"]
    processes = {str(Path(p["path"]).resolve()): p for p in report["processes"]}
    if len(pairs) != len(processes) or {str(Path(p["candidate"]).resolve()) for p in pairs} != processes.keys():
        raise ValueError("pair manifest does not exactly cover candidate logs")
    grouped, baselines, input_hashes = {}, {}, {}
    frozen_baseline_pair = {}
    for pair in pairs:
        policy, index = pair["policy"], pair["pair"]
        if policy not in ("default", "pressure_64_4096") or not 0 <= index < 7:
            raise ValueError("unknown policy or pair index")
        key = policy, index
        if key in grouped:
            raise ValueError("duplicate pair")
        candidate = processes[str(Path(pair["candidate"]).resolve())]
        if candidate["config"]["policy"] != policy:
            raise ValueError("candidate policy mismatch")
        baseline_paths = tuple(str(Path(pair[k]).resolve()) for k in ("baseline_families", "baseline_script_keys"))
        if index in frozen_baseline_pair and frozen_baseline_pair[index] != baseline_paths:
            raise ValueError("policies must use the same predeclared baseline pair")
        frozen_baseline_pair[index] = baseline_paths
        combined = {}
        for path in baseline_paths:
            if path not in baselines:
                baselines[path] = full_cycles(Path(path))
                input_hashes[path] = hashlib.sha256(Path(path).read_bytes()).hexdigest()
            if combined.keys() & baselines[path].keys():
                raise ValueError("overlapping baseline scenarios")
            combined.update(baselines[path])
        grouped[key] = candidate, combined
    policies = {p for p, _ in grouped}
    if any({i for p, i in grouped if p == policy} != set(range(7)) for policy in policies):
        raise ValueError("seven fixed pairs are required per selected policy")
    if len(set(frozen_baseline_pair.values())) != 7:
        raise ValueError("seven distinct paired baseline processes are required")
    rows = []
    for policy in sorted(policies):
        for family, phase_map in PHASE_MAP.items():
            for phase, baseline_phase in phase_map.items():
                maxima, counts, full = [], [], []
                for index in range(7):
                    candidate, baseline = grouped[policy, index]
                    values = [r for r in candidate["services"] if r["family"] == family
                              and r["phase"] == phase and r["kind"] == "normal"]
                    maxima.append(max(int(v["max_ns"]) for v in values))
                    counts.append(sum(int(v["count"]) for v in values))
                    full.append(baseline[family, baseline_phase])
                limits = [max(50000, f / 4) for f in full]
                observed = all(counts)
                ratios = [m / limit for m, limit in zip(maxima, limits)] if observed else None
                rows.append({"policy": policy, "family": family, "phase": phase,
                             "baseline_phase": baseline_phase, "shape": SHAPES[family],
                             "baseline_process_medians_ns": full, "normal_process_maxima_ns": maxima,
                             "normal_counts": counts, "limits_ns": limits, "normalized_ratios": ratios,
                             "median_process_max_ns": statistics.median(maxima), "absolute_max_ns": max(maxima),
                             "median_normalized_ratio": statistics.median(ratios) if observed else None,
                             "passed": statistics.median(ratios) <= 1 if observed else None,
                             "status": "evaluated" if observed else "unobserved in one or more processes; not a zero-cost pass"})
    return {"association": ASSOCIATION, "association_sha256": fingerprint(),
            "manifest_sha256": hashlib.sha256(manifest_path.read_bytes()).hexdigest(),
            "baseline_log_sha256": input_hashes, "gates": rows,
            "escalation": "absolute invocation/indivisible statistics only; host control has no PG00 ratio"}
