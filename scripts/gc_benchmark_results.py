"""Strict stdlib CSV evidence parsing for the standalone GC fixtures.

Each parse result is one estimate per metric per OS process. Timed sample IDs
0..2 are validated but excluded; IDs 3..13 contribute. Distribution observations
count pooled measured events; sample estimates/counters have 11 observations.
Absent event distributions have None estimates, not synthetic zero latencies.

Cases use slash-separated fixture dimensions (engine/case/count, kind/phase,
case/stage). Metrics carry explicit units; requested byte counters are not RSS.
Summary process_values is a process-ID-sorted list of {process, value,
observations}. Summary processes includes unobserved processes, observations is
their sum, and median/min/max ignore None. Pair estimates require all process
indices and values in both variants, with no zero baseline denominators.
"""
from __future__ import annotations

from collections import defaultdict
import csv
import io
import math
import re
from statistics import median

SUITES = {
    "access": ("field_access", ()),
    "access-borrow": ("field_access", ("--dynamic-borrow",)),
    "access-alternating": ("field_access", ("--alternating",)),
    "lifecycle": ("arena_comparison", ()),
    "lifecycle-prepared": ("arena_comparison", ("--prepared",)),
    "references": ("reference_access", ()),
    "collection": ("collection_profile", ()),
    "sustained": ("sustained_allocation", ()),
    "burst": ("sustained_allocation", ("--burst",)),
    "capacity": ("metadata_capacity", ()),
}

ACCESS_HEADER = "process,phase,sample,case,iterations,elapsed_ns,checksum"
ARENA_HEADER = "process,phase,sample,engine,case,count,elapsed_ns"
REFERENCE_HEADER = "sample,kind,phase,count,elapsed_ns"
COLLECTION_HEADER = "record,sample,case,count,stage,elapsed_ns,work,steps,maintenance_ns"
SUSTAINED_HEADER = (
    "sample,case,batch,allocations,allocation_ns,total_ns,service,service_ns,work,"
    "retired,phase,before_objects,objects,peak_managed_bytes,cycles,maintenance_ns,"
    "backing_bytes,indirect_bytes,allocation_growth_bytes"
)
CAPACITY_HEADER = (
    "case,stage,backing_bytes,pages,page_slots,page_table_bytes,roots,root_slots,"
    "root_table_bytes,descriptors,descriptor_slots,descriptor_table_bytes,"
    "mark_queue_bytes,release_queue_bytes,active_bytes,collect_ns,maintenance_ns"
)
ACCESS_CASES = {
    "access": tuple(f"{kind}_{op}" for kind in ("plain", "typed", "dynamic")
                    for op in ("read", "write", "increment"))
              + ("typed_borrow_each_increment", "typed_borrow_batch_increment"),
    "access-borrow": ("dynamic_borrow_each_increment", "dynamic_borrow_batch_increment"),
    "access-alternating": ("typed_alternating_increment", "dynamic_alternating_increment"),
}
SCALARS = ("scalar_read", "scalar_write", "scalar_increment", "borrow_each_increment",
           "borrow_batch_increment", "root_copy_idle_32")
STAGES = ("roots", "mark", "sweep", "transition")
MEASURED = range(3, 14)
METRIC_KEYS = {"case", "metric", "unit", "direction", "value", "observations"}


class EvidenceError(ValueError):
    """Malformed, incomplete or inconsistent benchmark evidence."""


def _require(condition, message):
    if not condition:
        raise EvidenceError(message)


def _rows(text, header, strings):
    """Reject noise, schema drift, ragged records and noncanonical unsigned ints."""
    fields = header.split(",")
    reader = csv.reader(io.StringIO(text, newline=""), strict=True)
    result = []
    try:
        _require(next(reader, None) == fields, f"expected exact header: {header}")
        for line, values in enumerate(reader, 2):
            _require(len(values) == len(fields), f"CSV row {line}: wrong column count")
            row = dict(zip(fields, values))
            for name in fields:
                if name not in strings:
                    _require(re.fullmatch(r"[0-9]+", row[name]) is not None,
                             f"CSV row {line}: invalid unsigned integer {name}")
                    row[name] = int(row[name])
            result.append(row)
    except (csv.Error, ValueError) as error:
        raise EvidenceError(f"invalid CSV evidence: {error}") from error
    return result


def _grid(rows, expected, identity):
    indexed = {}
    for row in rows:
        key = identity(row)
        _require(key in expected, f"unexpected record identity: {key}")
        _require(key not in indexed, f"duplicate record identity: {key}")
        indexed[key] = row
    missing = expected - indexed.keys()
    _require(not missing, f"missing records ({len(missing)}): {sorted(missing)[:3]}")
    return indexed


def _metric(case, name, unit, value, observations=11, direction="lower"):
    return dict(case=case, metric=name, unit=unit, direction=direction,
                value=value, observations=observations)


def _estimate(case, name, unit, values, direction="lower"):
    return _metric(case, name, unit, median(values) if values else None,
                   len(values), direction)


def _distribution(case, prefix, values, percentiles=(50, 95, 99)):
    values = sorted(values)
    result = []
    for percentile in percentiles:
        value = values[(len(values) * percentile + 99) // 100 - 1] if values else None
        result.append(_metric(case, f"{prefix}_p{percentile}_ns", "ns", value, len(values)))
    result.append(_metric(case, f"{prefix}_max_ns", "ns", values[-1] if values else None,
                          len(values)))
    return result


def _timings(rows, expected, identity, process=None, measured_phase=None):
    indexed = _grid(rows, expected, lambda row: (row["sample"], *identity(row)))
    groups = defaultdict(list)
    for (sample, *dimensions), row in sorted(indexed.items()):
        if process is not None:
            _require(row["process"] == process, "wrong process identity")
            phase = "warmup" if sample < 3 else measured_phase
            _require(row["phase"] == phase, "wrong warmup/measured phase")
        if sample in MEASURED:
            groups["/".join(map(str, dimensions))].append(row["elapsed_ns"])
    return groups


def _access(suite, text, process):
    rows = _rows(text, ACCESS_HEADER, {"phase", "case"})
    for row in rows:
        case = row["case"]
        expected = 350000 if case.endswith("_read") else 49999 if case.endswith("_write") else 50000
        _require(row["iterations"] == 50000, "field access iterations must be 50000")
        _require(row["checksum"] == expected, "field access checksum mismatch")
    expected = {(sample, case) for sample in range(14) for case in ACCESS_CASES[suite]}
    groups = _timings(rows, expected, lambda r: (r["case"],), process, "measure")
    return [_estimate(case, "ns_per_op", "ns/op", [ns / 50000 for ns in values])
            for case, values in groups.items()]


def _arena(suite, text, process):
    rows = _rows(text, ARENA_HEADER, {"phase", "engine", "case"})
    cases = set()
    for engine in ("kagari", "gc_arena"):
        cases.update((engine, case, 50000) for case in SCALARS)
        allocate = "allocate_prepared" if suite == "lifecycle-prepared" and engine == "kagari" else "allocate_rooted"
        cases.update((engine, case, count) for count in (512, 8192)
                     for case in (allocate, "collect_live", "clear_roots", "collect_dead"))
    expected = {(sample, *case) for sample in range(14) for case in cases}
    groups = _timings(rows, expected, lambda r: (r["engine"], r["case"], r["count"]),
                      process, "sample")
    return [_estimate(case, "ns_per_op", "ns/op", [ns / int(case.rsplit("/", 1)[1]) for ns in values])
            for case, values in groups.items()]


def _references(text):
    rows = _rows(text, REFERENCE_HEADER, {"kind", "phase"})
    _require(all(row["count"] == 50000 for row in rows), "reference count must be 50000")
    expected = {(sample, kind, phase) for sample in range(14)
                for kind in ("typed", "dynamic") for phase in ("Idle", "Mark", "Sweep")}
    groups = _timings(rows, expected, lambda r: (r["kind"], r["phase"]))
    return [_estimate(case, "ns_per_op", "ns/op", [ns / 50000 for ns in values])
            for case, values in groups.items()]


def _collection(text):
    rows = _rows(text, COLLECTION_HEADER, {"record", "case", "stage"})
    steps = defaultdict(list)
    aggregates = []
    expected = {(sample, case, record, stage) for sample in range(14)
                for case in ("leaves", "duplicate", "chain")
                for record, stages in (("stage", STAGES), ("full", ("full",)))
                for stage in stages}
    for row in rows:
        _require(row["count"] == 8192, "collection count must be 8192")
        _require(0 <= row["sample"] < 14 and row["case"] in ("leaves", "duplicate", "chain"),
                 "unknown collection sample/case")
        _require(row["maintenance_ns"] <= row["elapsed_ns"], "maintenance exceeds collection time")
        if row["record"] == "step":
            _require(row["stage"] in STAGES and row["steps"] == 1 and row["work"] <= 1024
                     and row["maintenance_ns"] == 0, "invalid normal collection step")
            steps[row["sample"], row["case"], row["stage"]].append(row)
        else:
            aggregates.append(row)
    indexed = _grid(aggregates, expected,
                    lambda r: (r["sample"], r["case"], r["record"], r["stage"]))
    for (sample, case, record, stage), row in indexed.items():
        if record == "stage":
            for field in ("elapsed_ns", "work", "steps", "maintenance_ns"):
                _require(row[field] == sum(step[field] for step in steps[sample, case, stage]),
                         f"collection stage {field} does not match step evidence")
        else:
            _require(row["steps"] == 0 and row["work"] > 0, "invalid separate full collection")
            _require(sum(step["work"] for s in STAGES for step in steps[sample, case, s]) > 0,
                     "missing collection step evidence")
    result = []
    for case in ("leaves", "duplicate", "chain"):
        for stage in (*STAGES, "full"):
            name = f"{case}/{stage}"
            samples = [indexed[sample, case, "full" if stage == "full" else "stage", stage]
                       for sample in MEASURED]
            for field, unit in (("elapsed_ns", "ns"), ("work", "work"),
                                ("steps", "count"), ("maintenance_ns", "ns")):
                result.append(_estimate(name, field, unit, [row[field] for row in samples],
                                        "neutral" if field in ("work", "steps") else "lower"))
            if stage != "full":
                events = [row["elapsed_ns"] for sample in MEASURED for row in steps[sample, case, stage]]
                result.extend(_distribution(name, "step", events))
                result.append(_metric(name, "step_observations", "count", len(events), 11, "neutral"))
    return result


def _validate_window(window, burst):
    previous_cycles = previous_objects = 0
    for batch, row in enumerate(window):
        cleanup = batch == 128
        allocations = 0 if cleanup else 4096 if burst and batch % 16 == 0 else 256
        _require(row["allocations"] == allocations, "incorrect sustained workload allocations")
        _require(row["phase"] in ("Idle", "Roots", "Mark", "Sweep"), "unknown collector phase")
        _require(row["maintenance_ns"] <= row["service_ns"] <= row["total_ns"]
                 and row["allocation_ns"] <= row["total_ns"], "inconsistent sustained timings")
        _require(row["cycles"] >= previous_cycles, "nonmonotonic cycle counter")
        _require(row["before_objects"] == previous_objects, "discontinuous object snapshots")
        _require(row["objects"] <= row["before_objects"] + allocations, "impossible object count")
        if cleanup:
            _require(row["service"] == "collect" and row["allocation_ns"] == 0
                     and row["phase"] == "Idle" and row["objects"] == 0
                     and row["backing_bytes"] == 0 and row["indirect_bytes"] == 0,
                     "incomplete final cleanup")
            _require(row["cycles"] - previous_cycles == (1 if window[127]["phase"] == "Idle" else 2)
                     and row["work"] > 0 and row["retired"] > 0, "invalid cleanup collection")
        else:
            _require(row["service"] in ("idle", "step", "assist"), "unknown sustained service")
            if row["service"] == "step":
                _require(row["work"] <= 1024 and row["retired"] <= 32, "normal step exceeds budget")
            if row["service"] == "idle":
                _require(all(row[field] == 0 for field in ("service_ns", "work", "retired", "maintenance_ns"))
                         and row["cycles"] == previous_cycles, "idle batch reports GC service")
        previous_cycles, previous_objects = row["cycles"], row["objects"]
    _require(window[127]["cycles"] > 0, "no automatic collection cycles in window")


def _ratio(numerator, denominator):
    return numerator / denominator if denominator else None


def _sustained(text, burst):
    rows = _rows(text, SUSTAINED_HEADER, {"case", "service", "phase"})
    expected = {(sample, case, batch) for sample in range(14)
                for case in ("leaf", "cycle") for batch in range(129)}
    indexed = _grid(rows, expected, lambda r: (r["sample"], r["case"], r["batch"]))
    for sample in range(14):
        for case in ("leaf", "cycle"):
            _validate_window([indexed[sample, case, batch] for batch in range(129)], burst)
    result = []
    for case in ("leaf", "cycle"):
        windows = [[indexed[sample, case, batch] for batch in range(129)] for sample in MEASURED]
        batches = [row for window in windows for row in window[:128]]
        allocations = sum(row["allocations"] for row in batches)
        total = sum(row["total_ns"] for row in batches)
        # Weighted operation costs and service fraction use the same measured window.
        for name, unit, value, direction in (
            ("ns_per_object", "ns/object", total / allocations, "lower"),
            ("allocation_ns_per_object", "ns/object", sum(r["allocation_ns"] for r in batches) / allocations, "lower"),
            ("ops_per_second", "objects/s", _ratio(allocations * 1e9, total), "higher"),
            ("gc_service_fraction", "fraction", _ratio(sum(r["service_ns"] for r in batches), total), "lower"),
            ("lifecycle_ns_per_object", "ns/object", (total + sum(w[128]["total_ns"] for w in windows)) / allocations, "lower"),
        ):
            result.append(_metric(case, name, unit, value, 11 if value is not None else 0, direction))
        result.extend(_distribution(case, "batch", [row["total_ns"] for row in batches]))
        for service in ("step", "assist"):
            events = [row["service_ns"] for row in batches if row["service"] == service]
            result.extend(_distribution(case, service, events, (99,)))
            for name, unit, values in (
                (f"{service}_count", "count", [sum(r["service"] == service for r in w[:128]) for w in windows]),
                (f"{service}_total_ns", "ns", [sum(r["service_ns"] for r in w[:128] if r["service"] == service) for w in windows]),
            ):
                result.append(_estimate(case, name, unit, values, "neutral" if unit == "count" else "lower"))
        for name, unit, values, direction in (
            ("maintenance_ns", "ns", [sum(r["maintenance_ns"] for r in w[:128]) for w in windows], "lower"),
            ("cycles", "count", [w[127]["cycles"] for w in windows], "neutral"),
            ("end_objects", "objects", [w[127]["objects"] for w in windows], "lower"),
            ("end_backing_bytes", "bytes", [w[127]["backing_bytes"] for w in windows], "lower"),
            ("end_indirect_bytes", "bytes", [w[127]["indirect_bytes"] for w in windows], "lower"),
            ("allocation_growth_bytes", "bytes", [w[127]["allocation_growth_bytes"] for w in windows], "neutral"),
            ("cleanup_ns", "ns", [w[128]["total_ns"] for w in windows], "lower"),
            ("cleanup_work", "work", [w[128]["work"] for w in windows], "neutral"),
            ("cleanup_maintenance_ns", "ns", [w[128]["maintenance_ns"] for w in windows], "lower"),
        ):
            result.append(_estimate(case, name, unit, values, direction))
        result.append(_metric(case, "peak_managed_bytes", "bytes",
                              max(r["peak_managed_bytes"] for w in windows for r in w), 11))
        result.append(_metric(case, "observed_peak_objects", "objects",
                              max(max(r["before_objects"] + r["allocations"], r["objects"]) for r in batches), 11))
        for quarter in range(4):
            values = [median(r["backing_bytes"] + r["indirect_bytes"]
                             for r in w[quarter * 32:(quarter + 1) * 32]) for w in windows]
            result.append(_estimate(case, f"managed_bytes_q{quarter + 1}", "bytes", values))
    return result


def _capacity(text):
    rows = _rows(text, CAPACITY_HEADER, {"case", "stage"})
    expected = {(case, stage) for case in ("pages", "roots", "descriptors")
                for stage in ("peak", "one_high_id", "empty")}
    indexed = _grid(rows, expected, lambda r: (r["case"], r["stage"]))
    result = []
    for (case, stage), row in sorted(indexed.items()):
        _require(row["maintenance_ns"] <= row["collect_ns"], "maintenance exceeds collect time")
        for entries, slots in (("pages", "page_slots"), ("roots", "root_slots"),
                               ("descriptors", "descriptor_slots")):
            _require(row[entries] <= row[slots], "metadata entries exceed slots")
        if stage == "peak":
            _require(row["collect_ns"] == row["maintenance_ns"] == 0, "peak is not a timed collection")
        for name in CAPACITY_HEADER.split(",")[2:]:
            unit = "bytes" if name.endswith("bytes") else "ns" if name.endswith("ns") else "count"
            result.append(_metric(f"{case}/{stage}", name, unit, row[name], 1,
                                  "neutral" if unit == "count" else "lower"))
    return result


def parse_run(suite: str, text: str, process: int) -> list[dict]:
    """Validate a complete fixture stdout and return measured process estimates.

    Only field_access and arena_comparison embed the runner's process index in
    CSV. Other fixtures start with their flags, never a synthetic process arg.
    Cleanup metrics cover the explicit final collect, not complete heap teardown.
    """
    _require(isinstance(suite, str) and suite in SUITES, f"unknown suite: {suite}")
    _require(type(process) is int and process >= 0, "invalid process index")
    _require(isinstance(text, str), "CSV evidence must be text")
    if suite in ACCESS_CASES:
        return _access(suite, text, process)
    if suite.startswith("lifecycle"):
        return _arena(suite, text, process)
    if suite == "references":
        return _references(text)
    if suite == "collection":
        return _collection(text)
    if suite in ("sustained", "burst"):
        return _sustained(text, suite == "burst")
    return _capacity(text)


def _metric_key(metric):
    _require(isinstance(metric, dict) and metric.keys() == METRIC_KEYS, "invalid metric fields")
    for field in ("case", "metric", "unit"):
        _require(isinstance(metric[field], str) and metric[field], f"invalid metric {field}")
    _require(metric["direction"] in ("lower", "higher", "neutral"), "invalid metric direction")
    value, count = metric["value"], metric["observations"]
    _require(type(count) is int and count >= 0, "invalid observation count")
    _require((value is None and count == 0) or
             (type(value) in (int, float) and math.isfinite(value) and value >= 0 and count > 0),
             "invalid metric value/observations")
    return tuple(metric[field] for field in ("case", "metric", "unit", "direction"))


def summarize(processes: list[dict]) -> list[dict]:
    """Summarize process estimates, never pool inner samples across processes.

    Candidate paired_ratio is median(candidate[i] / baseline[i]); change is
    (paired_ratio - 1) * 100 irrespective of metric direction. Missing/zero
    denominators or incomplete pairing yield None, never infinity/partial pairs.
    """
    _require(isinstance(processes, list), "process evidence must be a list")
    schemas, identities, groups = {}, set(), defaultdict(dict)
    for process in processes:
        _require(isinstance(process, dict) and
                 {"suite", "variant", "process", "metrics"} <= process.keys(), "invalid process record")
        suite, variant, index = process["suite"], process["variant"], process["process"]
        _require(isinstance(suite, str) and suite in SUITES, "unknown process suite")
        _require(variant in ("candidate", "baseline"), "unknown variant")
        _require(type(index) is int and index >= 0, "invalid process identity")
        identity = suite, variant, index
        _require(identity not in identities, f"duplicate process: {identity}")
        identities.add(identity)
        metrics = process["metrics"]
        _require(isinstance(metrics, list) and metrics, "missing process metrics")
        keys, names = set(), set()
        for metric in metrics:
            key = _metric_key(metric)
            _require(key[:2] not in names, f"duplicate metric: {key[:2]}")
            names.add(key[:2])
            keys.add(key)
            groups[suite, *key, variant][index] = metric
        _require(schemas.setdefault(suite, keys) == keys, f"mismatched metric keys: {suite}")
    result = []
    for (suite, case, name, unit, direction, variant), metrics in sorted(groups.items()):
        values = [metric["value"] for metric in metrics.values() if metric["value"] is not None]
        row = dict(suite=suite, case=case, metric=name, unit=unit, direction=direction,
                   variant=variant, median=median(values) if values else None,
                   min=min(values) if values else None, max=max(values) if values else None,
                   processes=len(metrics), observations=sum(m["observations"] for m in metrics.values()),
                   process_values=[dict(process=i, value=m["value"], observations=m["observations"])
                                   for i, m in sorted(metrics.items())])
        if variant == "candidate":
            baseline = groups.get((suite, case, name, unit, direction, "baseline"), {})
            ratios = []
            if baseline.keys() == metrics.keys():
                ratios = [metrics[i]["value"] / baseline[i]["value"] for i in metrics
                          if metrics[i]["value"] is not None and baseline[i]["value"] not in (None, 0)]
            paired = median(ratios) if len(ratios) == len(metrics) else None
            row.update(paired_ratio=paired,
                       paired_change_percent=(paired - 1) * 100 if paired is not None else None)
        result.append(row)
    return result
