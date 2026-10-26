"""Validate selected complete storage families and summarize per-process medians.

Use uv run python. --memory consumes the separately instrumented allocation log.
Distinct timing files may select different families, but never partial lifecycles.
This is a baseline summary, not the paired before/after acceptance checker.
"""
from __future__ import annotations

import argparse
import ast
import hashlib
import json
from pathlib import Path
import statistics

ORDINARY = {"setup", "construct", "live_gc", "release_gc"}
PHASES = {
    "sparse_types_32x4": ORDINARY,
    "mixed_native_128x6": ORDINARY,
    "scalar_65536": ORDINARY,
    "references_8192": ORDINARY,
    "script_keys_256": ORDINARY,
    "sparse_cycles_4096_keep64": {"setup", "construct", "dense_gc", "sparsify_gc",
        "churn_gc_0", "churn_gc_1", "churn_gc_2", "sparse_live_gc", "release_gc"},
    "retained_reload_4x256": {"setup", "construct_reload", "retained_gc",
        "release_old_gc", "release_all_gc"},
}


def complete(rows: dict, path: Path) -> set[str]:
    names = {name for name, _ in rows}
    if not names or not names <= PHASES.keys():
        raise ValueError(f"{path}: empty or unknown workload selection")
    for name in names:
        if {phase for workload, phase in rows if workload == name} != PHASES[name]:
            raise ValueError(f"{path}: incomplete {name} lifecycle")
    return names


def fields(line: str) -> tuple[str, dict]:
    _, name, *columns = line.split(",")
    return name, dict(column.split("=", 1) for column in columns)


def timing(path: Path) -> dict:
    rows = {}
    for line in path.read_text().splitlines():
        if not line.startswith("family,"):
            continue
        prefix, raw = line.split(",samples_ns=", 1)
        name, values = fields(prefix)
        samples = ast.literal_eval(raw)
        if len(samples) != 11 or values["samples"] != "11" or values["warmups"] != "3":
            raise ValueError(f"{path}: unexpected sample contract")
        for key, expected in [("median_ns", statistics.median(samples)),
                              ("min_ns", min(samples)), ("max_ns", max(samples))]:
            if int(values[key]) != expected:
                raise ValueError(f"{path}: {name}/{key} disagrees with raw samples")
        key = (name, values["phase"])
        if key in rows:
            raise ValueError(f"{path}: duplicate row {key}")
        rows[key] = {"median_ns": int(values["median_ns"]), "samples_ns": samples,
                     "objects": int(values["objects"])}
    complete(rows, path)
    return rows


def repeated(repetitions: list, label: str) -> list[dict]:
    if [repeat for repeat, _ in repetitions] != [0, 1, 2]:
        raise ValueError(f"{label}: missing/duplicate memory repetition")
    return [value for _, value in repetitions]


def memory(path: Path) -> tuple[list[dict], list[dict]]:
    rows, peaks, teardowns = {}, {}, {}
    for line in path.read_text().splitlines():
        if line.startswith("family_teardown,"):
            if not line.endswith("net_bytes: 0 }"):
                raise ValueError(f"{path}: nonzero Runtime teardown: {line}")
            _, name, repetition, _ = line.split(",", 3)
            teardowns.setdefault(name, []).append(int(repetition.split("=")[1]))
        if line.startswith(("family_memory,", "family_peak,")):
            name, values = fields(line)
            repetition = int(values.pop("repeat"))
            if line.startswith("family_peak,"):
                peaks.setdefault(name, []).append((repetition, {k: int(v) for k, v in values.items()}))
            else:
                phase = values.pop("phase")
                rows.setdefault((name, phase), []).append((repetition, {k: int(v) for k, v in values.items()}))
    names = complete(rows, path)
    if set(teardowns) != names or any(value != [0, 1, 2] for value in teardowns.values()):
        raise ValueError(f"{path}: incomplete teardown coverage")
    output = []
    for (name, phase), repetitions in rows.items():
        values = repeated(repetitions, f"{path}/{name}/{phase}")
        stable = [{k: v for k, v in value.items() if k != "interval_peak_bytes"} for value in values]
        if any(value != stable[0] for value in stable):
            raise ValueError(f"{path}: {name}/{phase} retained backing/counts are not repeatable")
        row = dict(workload=name, phase=phase, repetitions=3, **stable[0])
        if "interval_peak_bytes" in values[0]:
            row["interval_peak_samples_bytes"] = [v["interval_peak_bytes"] for v in values]
        output.append(row)
    high_water = []
    if peaks and set(peaks) != names:
        raise ValueError(f"{path}: incomplete high-water coverage")
    for name, repetitions in peaks.items():
        values = repeated(repetitions, f"{path}/{name}/peak")
        for value in values:
            if value["lifecycle_peak_bytes"] < max(value["setup_peak_bytes"], value["post_setup_peak_bytes"]):
                raise ValueError(f"{path}: inconsistent lifecycle peak")
        high_water.append(dict(workload=name, samples=values,
            maxima={key: max(v[key] for v in values) for key in values[0]},
            minima={key: min(v[key] for v in values) for key in values[0]}))
    return output, high_water


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("logs", nargs="+", type=Path)
    parser.add_argument("--memory", type=Path)
    parser.add_argument("--json", type=Path)
    args = parser.parse_args()
    if len(set(args.logs)) != len(args.logs):
        raise ValueError("duplicate timing input")
    processes = [timing(path) for path in args.logs]
    keys = dict.fromkeys(key for process in processes for key in process)
    summary = []
    for name, phase in keys:
        values = [process[name, phase] for process in processes if (name, phase) in process]
        if len({value["objects"] for value in values}) != 1:
            raise ValueError(f"{name}/{phase}: live counts differ across processes")
        medians = [value["median_ns"] for value in values]
        row = dict(workload=name, phase=phase, median_ns=statistics.median(medians),
                   process_medians_ns=medians, sample_min_ns=min(min(v["samples_ns"]) for v in values),
                   sample_max_ns=max(max(v["samples_ns"]) for v in values), objects=values[0]["objects"])
        summary.append(row)
        print(f"{name:28} {phase:18} median_us={row['median_ns']/1000:.3f} "
              f"process_range_us={min(medians)/1000:.3f}..{max(medians)/1000:.3f} "
              f"sample_max_us={row['sample_max_ns']/1000:.3f}")
    measured_memory, peaks = memory(args.memory) if args.memory else ([], [])
    for row in measured_memory:
        print(f"memory {row['workload']:28} {row['phase']:18} "
              f"net_delta_bytes={row['net_delta_bytes']} requests_since_setup={row['requests_since_setup']}")
    for row in peaks:
        print(f"peak {row['workload']}: maxima={row['maxima']} minima={row['minima']}")
    paths = args.logs + ([args.memory] if args.memory else [])
    report = {"method": "median of per-process medians; range is not a confidence interval",
              "inputs_sha256": {str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in paths},
              "timing": summary, "memory": measured_memory, "peaks": peaks}
    if args.json:
        args.json.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
