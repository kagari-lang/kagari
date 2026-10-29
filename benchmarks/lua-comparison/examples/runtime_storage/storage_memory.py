"""Validate three-lifecycle no-argument diagnostics and seven paired memory gates."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import statistics

NAMES = {"points", "pairs", "interfaces", "wide_interfaces", "generic_interfaces",
         "copied_interface", "nodes", "numbers"}
PHASES = {"setup", "constructed", "live", "released", "empty", "reused"}
SCENARIOS = {(name, count) for name in NAMES for count in (1, 512)}
COUNT_KEYS = {"requests", "requested_bytes", "peak_net_bytes", "interval_peak_net_bytes", "net_bytes"}
PEAK_KEYS = {"setup_peak_bytes", "post_setup_peak_bytes", "post_setup_peak_delta_bytes", "lifecycle_peak_bytes"}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def counters(raw: str) -> dict:
    values = {key: int(value) for key, value in re.findall(r"(\w+): (-?\d+)", raw.split("}", 1)[0])}
    require(values.keys() == COUNT_KEYS, "incomplete allocation counters")
    require(values["peak_net_bytes"] >= max(0, values["interval_peak_net_bytes"])
            and values["interval_peak_net_bytes"] >= values["net_bytes"], "inconsistent allocation peak")
    return values


def read(path: Path) -> dict:
    rows, peaks, totals, first_use, loops = {}, {}, {}, {}, {}
    for line in path.read_text().splitlines():
        kind = line.split(",", 1)[0]
        if kind not in {"memory", "memory_peak", "teardown", "shared_first_use", "field_loop"}:
            continue
        if kind == "memory_peak":
            head, values = line, None
        else:
            head, raw = line.split(",allocations=", 1)
            values = counters(raw)
        _, name, *columns = head.split(",")
        fields = dict(column.split("=", 1) for column in columns)
        if kind == "field_loop":
            require(name not in loops and int(fields["iterations"]) == 50000, "duplicate/changed field loop")
            loops[name] = values
            continue
        count, repetition = int(fields.pop("count")), int(fields.pop("repetition"))
        require((name, count) in SCENARIOS and repetition in range(3), "unknown scenario/repetition")
        key = name, count, repetition
        if kind == "memory":
            phase = fields.pop("phase")
            require(phase in PHASES and not fields, "unknown memory phase/fields")
            key += (phase,)
            target = rows
        elif kind == "memory_peak":
            require(fields.keys() == PEAK_KEYS, "incomplete peak record")
            values = {key: int(value) for key, value in fields.items()}
            target = peaks
        else:
            require(not fields, "unknown counter fields")
            target = totals if kind == "teardown" else first_use
        require(key not in target, "duplicate lifecycle record")
        target[key] = values
    expected = {(name, count, repetition) for name, count in SCENARIOS for repetition in range(3)}
    require(peaks.keys() == totals.keys() == first_use.keys() == expected, "incomplete lifecycle coverage")
    require(rows.keys() == {key + (phase,) for key in expected for phase in PHASES}, "incomplete phases")
    require(loops.keys() == {"read_fields", "write_fields", "fields"}, "incomplete field controls")
    metrics = {}
    for name, count in sorted(SCENARIOS):
        collected = {}
        for repetition in range(3):
            key = name, count, repetition
            setup, total, peak = rows[key + ("setup",)], totals[key], peaks[key]
            require(total["net_bytes"] == 0, "nonzero Runtime teardown")
            require(peak["setup_peak_bytes"] == setup["peak_net_bytes"], "setup peak mismatch")
            require(peak["lifecycle_peak_bytes"] == total["peak_net_bytes"]
                    == max(peak["setup_peak_bytes"], peak["post_setup_peak_bytes"]), "lifecycle peak mismatch")
            require(peak["post_setup_peak_delta_bytes"] == peak["post_setup_peak_bytes"] - setup["net_bytes"],
                    "post-setup delta mismatch")
            # The accumulator also includes intermediate reuse snapshots not printed
            # as phase rows. Printed intervals and final teardown are lower bounds.
            observed = [rows[key + (phase,)]["interval_peak_net_bytes"] for phase in PHASES - {"setup"}]
            require(peak["post_setup_peak_bytes"] >= max(*observed, total["interval_peak_net_bytes"], setup["net_bytes"]),
                    "omitted post-setup interval")
            for phase in sorted(PHASES):
                value = rows[key + (phase,)]["net_bytes"]
                metric = f"{name}/{count}/{phase}/backing"
                collected.setdefault(metric, []).append(value if phase == "setup" else value - setup["net_bytes"])
            for metric, field in [("setup_peak", "setup_peak_bytes"),
                                  ("post_setup_peak_delta", "post_setup_peak_delta_bytes"),
                                  ("lifecycle_peak", "lifecycle_peak_bytes")]:
                collected.setdefault(f"{name}/{count}/{metric}", []).append(peak[field])
        for metric, samples in collected.items():
            if metric.endswith("/backing"):
                require(len(set(samples)) == 1, f"{metric}: backing not repeatable")
            metrics[metric] = {"samples": samples, "maximum": max(samples)}
    return {"metrics": metrics, "teardowns": len(totals), "field_loops": loops,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def paired(manifest: Path) -> dict:
    pairs = json.loads(manifest.read_text())["pairs"]
    require(len(pairs) == 7 and {pair["pair"] for pair in pairs} == set(range(7)), "seven distinct pairs required")
    pairs.sort(key=lambda pair: pair["pair"])
    paths = [Path(pair[side]).resolve() for pair in pairs for side in ("baseline", "candidate")]
    require(len(set(paths)) == 14, "each pair requires distinct process logs")
    processes = {str(path): read(path) for path in paths}
    rows = []
    for metric in processes[str(paths[0])]["metrics"]:
        baselines = [processes[str(paths[index])]["metrics"][metric]["maximum"] for index in range(0, 14, 2)]
        candidates = [processes[str(paths[index])]["metrics"][metric]["maximum"] for index in range(1, 14, 2)]
        setup = metric.endswith(("/setup/backing", "/setup_peak"))
        bounds = [value * 1.1 if setup else value + max(value * .1, 4096) for value in baselines]
        ratios = [value / bound for value, bound in zip(candidates, bounds)]
        rows.append({"metric": metric, "baseline_bytes": statistics.median(baselines),
                     "candidate_bytes": statistics.median(candidates), "baseline_samples": baselines,
                     "candidate_samples": candidates, "bounds": bounds, "ratios": ratios,
                     "normalized": statistics.median(ratios), "passed": statistics.median(ratios) <= 1})
    return {"method": "seven paired process maxima of three independently warmed lifecycles; requested bytes, not RSS",
            "manifest_sha256": hashlib.sha256(manifest.read_bytes()).hexdigest(), "processes": processes, "gates": rows}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pairs", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    report = paired(args.pairs)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    failed = [row for row in report["gates"] if not row["passed"]]
    print(f"storage memory: {len(report['gates'])} gates, {len(failed)} failed; "
          f"{sum(process['teardowns'] for process in report['processes'].values())} zero-net teardowns")
    for row in failed:
        print(row["metric"], row["baseline_bytes"], row["candidate_bytes"], row["normalized"])
    raise SystemExit(bool(failed))


if __name__ == "__main__":
    main()
