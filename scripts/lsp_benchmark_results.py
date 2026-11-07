"""Validation and paired process-level summaries for LSP benchmark archives."""
from __future__ import annotations

import math
import random
import statistics

from lsp_benchmark_inputs import object_digest

MATCH_METADATA = ("rustc", "cargo", "platform", "machine", "cpu", "logical_cpus", "power_mode",
                  "profile", "features", "transport", "lock_sha256", "workload_sha256", "protocol_schema")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def validate_sample(sample):
    require(sample.get("workload") == "B01.protocol", "missing B01 protocol sample")
    require(sample.get("errors") == 0 and sample.get("completed") == 2, "unsuccessful protocol sample")
    evidence = sample.get("evidence", {})
    require(evidence.get("positionEncoding") == "utf-16", "wrong negotiated position encoding")
    require(evidence.get("openClose") is True and evidence.get("change") == 1, "wrong sync capability")
    require(evidence.get("serverName") == "kagari-analyzer", "wrong server identity")
    require(isinstance(evidence.get("serverVersion"), str) and evidence["serverVersion"], "missing build version")
    require(evidence.get("shutdown_is_null") is True and evidence.get("exit_code") == 0, "shutdown failed")
    for metric in ("spawn_to_initialize_ns", "initialize_round_trip_ns"):
        value = sample.get(metric)
        require(isinstance(value, int) and not isinstance(value, bool) and value > 0, f"invalid {metric}")
    require(sample["spawn_to_initialize_ns"] >= sample["initialize_round_trip_ns"], "startup clock inversion")


def validate_report(report):
    require(report.get("schema") == 1, "unsupported LSP benchmark report schema")
    metadata = report.get("metadata", {})
    for key in MATCH_METADATA:
        require(key in metadata and metadata[key] is not None, f"missing metadata: {key}")
    require(object_digest(report.get("inputs")) == metadata["workload_sha256"], "input manifest hash mismatch")
    require(report.get("source_before") == report.get("source_after") == metadata.get("source"),
            "source changed during frozen campaign")
    source = metadata.get("source", {})
    require(source and object_digest(source.get("files")) == source.get("source_sha256"), "invalid source fingerprint")
    groups = report.get("groups")
    require(isinstance(groups, int) and not isinstance(groups, bool) and groups > 0, "invalid process group count")
    require(report.get("check_only") is True or groups >= 7, "timing needs seven process groups")
    images = report.get("images", {})
    require(set(images) in ({"candidate"}, {"candidate", "baseline"}), "invalid image set")
    for image in images.values():
        require(isinstance(image.get("sha256"), str) and len(image["sha256"]) == 64, "missing image hash")
        require(isinstance(image.get("path"), str) and image["path"], "missing archived image path")
    samples = report.get("samples", [])
    expected = {(group, image) for group in range(groups) for image in images}
    actual = set()
    for sample in samples:
        validate_sample(sample)
        key = (sample.get("group"), sample.get("image"))
        require(key in expected and key not in actual, "unknown or repeated process sample")
        actual.add(key)
    require(actual == expected, "missing required process samples")
    require(report.get("unavailable_rows") == ["B01.diagnostics", "B02", "B03", "B04", "B05", "B06", "B07", "B08"],
            "LSP00 must explicitly distinguish protocol startup from unmeasured semantic rows")


def validate_compatible(baseline, candidate):
    for key in MATCH_METADATA:
        require(baseline.get(key) == candidate.get(key), f"incompatible baseline {key}")


def interval(changes):
    randomizer = random.Random(178921)
    medians = sorted(statistics.median(randomizer.choices(changes, k=len(changes))) for _ in range(20000))
    return [medians[499], medians[19499]]


def summarize(report):
    validate_report(report)
    rows = {}
    for metric in ("spawn_to_initialize_ns", "initialize_round_trip_ns"):
        groups = {image: {sample["group"]: sample[metric] / 1_000_000
                          for sample in report["samples"] if sample["image"] == image}
                  for image in report["images"]}
        candidate = list(groups["candidate"].values())
        row = {"unit": "ms", "candidate_median": statistics.median(candidate),
               "candidate_range": [min(candidate), max(candidate)],
               "tails": "insufficient samples; no p95 or p99 inferred from startup groups"}
        if report["check_only"]:
            row["status"] = "correctness-only"
        elif "baseline" not in groups:
            row["status"] = "baseline-established"
        else:
            baseline = list(groups["baseline"].values())
            absolute = [groups["candidate"][index] - groups["baseline"][index] for index in range(report["groups"])]
            percentages = [100 * absolute[index] / groups["baseline"][index] for index in range(report["groups"])]
            change = statistics.median(absolute)
            percentage = statistics.median(percentages)
            confidence = interval(absolute)
            established = confidence[0] > 0 or confidence[1] < 0
            status = "inconclusive" if not established else "pass"
            if established and change > 2 and percentage > 10:
                status = "regression"
            row.update(baseline_median=statistics.median(baseline), baseline_range=[min(baseline), max(baseline)],
                       median_paired_change=change, median_paired_change_percent=percentage,
                       confidence_interval_95=confidence, status=status)
        require(all(math.isfinite(value) for value in candidate), "nonfinite sample")
        rows[metric.removesuffix("_ns")] = row
    return {"schema": 1, "rows": rows, "unavailable_rows": report["unavailable_rows"],
            "sampling": "seven independent fresh process groups; 20000 deterministic process-group bootstrap medians when paired",
            "scope": "B01 protocol readiness only; no diagnostic, semantic, RSS or worker-retirement acceptance"}
