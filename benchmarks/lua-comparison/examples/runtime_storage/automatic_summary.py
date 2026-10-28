"""Validate automatic-family observations without treating check runs as timing gates."""
from __future__ import annotations

import ast
import hashlib
from pathlib import Path
import statistics

import automatic_baselines

BASIC = {"construct", "live_gc", "access", "release_boundary", "release_gc"}
PHASES = {
    "sparse_types_32x4": BASIC,
    "mixed_native_128x6": BASIC,
    "scalar_65536": BASIC,
    "references_8192": BASIC,
    "script_keys_256": BASIC,
    "sparse_cycles_4096_keep64": {
        "construct", "dense_gc", "sparsify", "sparsify_gc", "sparse_live_gc",
        "release_boundary", "release_gc",
        *(f"churn_{phase}_{i}" for phase in ("construct", "gc", "access") for i in range(3)),
    },
    "retained_reload_4x256": {
        "construct_reload", "retained_gc", "access", "release_old_boundary",
        "release_old_gc", "release_all_boundary", "release_all_gc",
    },
    automatic_baselines.CONTROL: {
        "admit_roots", "start_cycle", "pressure_burst", "assist", "verify_roots",
        "release_burst_gc", "release_roots", "release_gc",
    },
}
KINDS = {"normal", "pressure_assist", "explicit", "cycles", "carried_cycles"}
INDIVISIBLE = {
    "ProviderScan", "PayloadDrop", "BackingRelease", "MetadataDrop", "RootDrop",
    "DescriptorDrop", "WeakTokenDrop", "DirectoryCompaction", "EmptyMapRelease",
    "QueueCompaction", "LifecycleDrop",
}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def fields(columns: list[str]) -> dict[str, str]:
    pairs = [column.split("=", 1) for column in columns]
    require(all(len(pair) == 2 for pair in pairs), "malformed observation field")
    result = dict(pairs)
    require(len(result) == len(pairs), "duplicate observation field")
    return result


def work(row: dict) -> None:
    require(int(row["failures"]) == 0, "collector service failed")
    for unit in ("work", "ns"):
        require(0 <= int(row[f"last_{unit}"]) <= int(row[f"max_{unit}"])
                <= int(row[f"total_{unit}"]), "inconsistent work/time extrema")


def process(path: Path, check_only: bool) -> dict:
    config = None
    phases, services, slices, indivisible, summaries, checks = {}, {}, {}, {}, {}, {}
    count, warmups = (1, 0) if check_only else (11, 3)
    for line in path.read_text().splitlines():
        if not line.startswith("auto_"):
            continue
        prefix, _, slowest = line.partition(",slowest=")
        prefix, _, maxima = prefix.partition(",sample_maxima_ns=")
        prefix, _, counts = prefix.partition(",sample_counts=")
        columns = prefix.split(",")
        tag, policy = columns[:2]
        if tag == "auto_config":
            require(config is None, "duplicate automatic config")
            config = {"policy": policy, **fields(columns[2:])}
            require(config["mode"] == ("check" if check_only else "measurement"),
                    "check/measurement input mismatch; check runs are not acceptance")
            require(config["counting_allocator"] == "false", "allocation-instrumented timing")
            require(config["raw_pauses_include_indivisible"] == "true", "subtracted pause")
            expected = {"default": (1024, 65536), "pressure_64_4096": (64, 4096)}
            require(policy in expected, "unknown pressure policy")
            require((int(config["unit_threshold"]), int(config["byte_threshold"]))
                    == expected[policy], "changed pressure policy")
            continue
        require(config is not None and policy == config["policy"], "missing/mixed config")
        family = columns[2]
        require(family in PHASES, "unknown automatic family")
        row = fields(columns[3:])
        if tag == "auto_check":
            key, destination = family, checks
        elif tag == "auto_summary":
            row["sample_maxima_ns"] = ast.literal_eval(maxima)
            row["sample_counts"] = ast.literal_eval(counts)
            key, destination = (family, row["phase"]), summaries
        else:
            key = (family, row["phase"], int(row["sample"]))
            require(key[1] in PHASES[family] and 0 <= key[2] < count, "unknown phase/sample")
            if tag == "auto_phase":
                destination = phases
            elif tag == "auto_slices":
                destination = slices
            elif tag == "auto_work":
                key = (*key, row["kind"])
                require(row["kind"] in KINDS, "unknown work category")
                destination = services
                work(row)
            elif tag == "auto_indivisible":
                key = (*key, row["kind"])
                require(row["kind"] in INDIVISIBLE, "unknown indivisible category")
                row["slowest"] = slowest
                require(0 <= int(row["unknown_size_count"]) <= int(row["count"]), "invalid unknown-size count")
                require(0 <= int(row["max_ns"]) <= int(row["total_ns"]), "invalid indivisible maximum")
                require((int(row["count"]) == 0) == (slowest == "None"), "missing indivisible sample")
                destination = indivisible
            else:
                raise ValueError(f"unknown observation row {tag}")
        require(key not in destination, f"duplicate {tag} {key}")
        destination[key] = row
    expected = {(f, p, i) for f, ps in PHASES.items() for p in ps for i in range(count)}
    require(phases.keys() == slices.keys() == expected, "incomplete phases or slices")
    require(services.keys() == {(*key, k) for key in expected for k in KINDS}, "incomplete services")
    require(indivisible.keys() == {(*key, k) for key in expected for k in INDIVISIBLE}, "incomplete indivisible observations")
    require(summaries.keys() == {(f, p) for f, ps in PHASES.items() for p in ps}, "incomplete summaries")
    require(checks.keys() == PHASES.keys(), "incomplete semantic checks")
    for key, phase in phases.items():
        normal, assist, explicit, cycles, carried = [services[*key, k] for k in
            ("normal", "pressure_assist", "explicit", "cycles", "carried_cycles")]
        require(sum(int(r["total_ns"]) for r in (normal, assist, explicit))
                <= int(phase["elapsed_ns"]), "service time exceeds action wall time")
        require(sum(int(r["total_work"]) for r in (normal, assist, explicit))
                == int(slices[key]["total_work"]), "slice/service work mismatch")
        require(int(carried["count"]) <= 1, "multiple carried cycles")
        require(phase["active_cycle_at_start"] == "true" or int(carried["count"]) == 0,
                "carried cycle without active entry")
        explicit_phase = phase["scope"] == "Explicit"
        require(phase["scope"] in ("Automatic", "Explicit"), "unknown phase scope")
        if not explicit_phase and key[0] != automatic_baselines.CONTROL:
            require(key[1] in automatic_baselines.PHASE_MAP[key[0]], "unmapped automatic scenario")
        require(int(explicit["count"]) == int(explicit_phase), "explicit/automatic scopes mixed")
        if explicit_phase:
            require(int(normal["count"]) == int(assist["count"]) == 0, "automatic service in explicit-only phase")
    for i in range(count):
        control = automatic_baselines.CONTROL
        first = services[control, "start_cycle", i, "normal"]
        assist = services[control, "assist", i, "pressure_assist"]
        carried = services[control, "assist", i, "carried_cycles"]
        require(int(first["count"]) == 1 and 0 < int(first["last_work"]) <= 1024,
                "host control did not start one bounded normal step")
        require(int(slices[control, "start_cycle", i]["count"]) == 1, "multiple start slices")
        require(int(services[control, "start_cycle", i, "cycles"]["count"]) == 0,
                "host control cycle completed before burst")
        require(phases[control, "pressure_burst", i]["active_cycle_at_start"] == "true",
                "burst did not enter an active cycle")
        require(phases[control, "assist", i]["active_cycle_at_start"] == "true",
                "assist observation did not carry a cycle")
        burst_phase = phases[control, "pressure_burst", i]
        require(int(burst_phase["objects_after"]) - int(burst_phase["objects_before"]) == 2048,
                "host burst population changed")
        require(all(int(services[control, "pressure_burst", i, kind]["count"]) == 0
                    for kind in ("normal", "pressure_assist", "explicit")), "collection inside allocation burst")
        require(int(assist["count"]) == 1 and int(assist["last_work"]) > 1024,
                "host pressure burst did not force an assist beyond normal budget")
        require(int(services[control, "assist", i, "normal"]["count"]) == 0,
                "assist mislabeled as normal work")
        require(int(services[control, "assist", i, "cycles"]["count"]) == 0
                and int(carried["count"]) == 1, "incorrect carried cycle completion")
        require(int(carried["total_work"]) == int(first["total_work"]) + int(assist["total_work"]),
                "carried work omitted or double counted")
    for (family, phase), summary in summaries.items():
        require(int(summary["samples"]) == count and int(summary["warmups"]) == warmups, "sample contract changed")
        values = [services[family, phase, i, "normal"] for i in range(count)]
        maxima = [int(v["max_ns"]) for v in values]
        counts = [int(v["count"]) for v in values]
        require(summary["sample_maxima_ns"] == maxima and summary["sample_counts"] == counts, "raw/summary disagreement")
        require(int(summary["normal_max_ns"]) == max(maxima)
                and int(summary["normal_count"]) == sum(counts), "incorrect process maximum/count")
    for family, check in checks.items():
        require(check["mode"] == ("check" if check_only else "measurement"), "check mode mismatch")
        require(int(check["samples"]) == count and int(check["warmups"]) == warmups, "check sample contract mismatch")
        total = sum(int(summaries[family, p]["normal_count"]) for p in PHASES[family])
        require(total > 0 and int(check["normal_count"]) == total and check["active"] == "true",
                f"{family}: no actual automatic normal activity")
        require(check["semantic_checks"] == check["observation_checks"] == "passed", "failed checks")
    return {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "config": config, "summaries": summaries, "services": services,
            "indivisible": indivisible, "checks": checks}


def summarize(paths: list[Path], check_only: bool) -> dict:
    processes = [process(path, check_only) for path in paths]
    rows, assists = [], []
    for policy in dict.fromkeys(p["config"]["policy"] for p in processes):
        selected = [p for p in processes if p["config"]["policy"] == policy]
        for family, phases in PHASES.items():
            for phase in sorted(phases):
                maxima = [int(p["summaries"][family, phase]["normal_max_ns"]) for p in selected]
                counts = [int(p["summaries"][family, phase]["normal_count"]) for p in selected]
                rows.append({"policy": policy, "family": family, "phase": phase,
                             "process_maxima_ns": maxima, "process_normal_counts": counts,
                             "median_process_max_ns": statistics.median(maxima),
                             "absolute_max_ns": max(maxima), "gate": None,
                             "baseline_phase": automatic_baselines.PHASE_MAP.get(family, {}).get(phase),
                             "absolute_only_control": family == automatic_baselines.CONTROL})
                per_process = [[v for k, v in p["services"].items()
                                if k[0] == family and k[1] == phase and k[3] == "pressure_assist"]
                               for p in selected]
                assist_maxima = [max(int(v["max_ns"]) for v in values) for values in per_process]
                assists.append({"policy": policy, "family": family, "phase": phase,
                                "process_maxima_ns": assist_maxima,
                                "median_process_max_ns": statistics.median(assist_maxima),
                                "absolute_max_ns": max(assist_maxima),
                                "process_counts": [sum(int(v["count"]) for v in values) for values in per_process],
                                "process_total_work": [sum(int(v["total_work"]) for v in values) for values in per_process],
                                "process_total_ns": [sum(int(v["total_ns"]) for v in values) for values in per_process],
                                "gate": None, "reason": "escalation is absolute-only, never a normal-step sample"})
    report = {"mode": "check" if check_only else "measurement", "acceptance": "not evaluated; pair with scenario-matched frozen full-cycle baselines",
              "normal_pauses": rows, "assist_pauses": assists, "inputs_sha256": {p["path"]: p["sha256"] for p in processes},
              "processes": [{"path": p["path"], "config": p["config"], "activity": p["checks"],
                             "services": [{"family": k[0], "phase": k[1], "sample": k[2], **v}
                                          for k, v in p["services"].items()],
                             "indivisible": [{"family": k[0], "phase": k[1], "sample": k[2], **v}
                                             for k, v in p["indivisible"].items()]}
                            for p in processes]}
    print(f"Validated {len(processes)} complete automatic-family processes ({report['mode']}); no timing gate evaluated.")
    for p in processes:
        print(p["path"], {f: int(v["normal_count"]) for f, v in p["checks"].items()})
    return report
