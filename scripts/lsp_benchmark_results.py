"""Validation and paired process-level summaries for LSP benchmark archives."""
from __future__ import annotations

import math
import random
import statistics
from pathlib import Path
import lsp_benchmark_queries as queries
import lsp_benchmark_diagnostics as diagnostics

from lsp_benchmark_inputs import object_digest
from lsp_benchmark_diagnostics import APP, API, UNAFFECTED, METRICS, frozen_scenario, matches

MATCH_METADATA = ("rustc", "cargo", "platform", "machine", "cpu", "logical_cpus", "power_mode",
                  "profile", "features", "transport", "suite", "lock_sha256", "workload_sha256", "protocol_schema", "cache_state")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def unavailable(suite):
    if suite == "protocol":
        return ["B01.diagnostics", "B02", "B03", "B04", "B05", "B06", "B07", "B08"]
    if suite == "queries":
        return ["B01.diagnostics", "B03.diagnostics", "B06", "B07", "B08"]
    require(suite in ("diagnostics", "diagnostic-tails"), "unknown benchmark suite")
    return ["B02", "B03.queries", "B04", "B05", "B06", "B07", "B08"]


def validate_diagnostics(sample, steps=None, metric_consistency=True, history_limit=512):
    steps = frozen_scenario()["steps"] if steps is None else steps
    raw = sample.get("operations", [])
    require(len(raw) == len(steps), "missing diagnostic operations")
    uris = sample.get("document_uris", {})
    require(set(uris) == {APP, API, UNAFFECTED} and len(set(uris.values())) == 3, "invalid diagnostic document identities")
    for path, uri in uris.items():
        require(isinstance(uri, str) and uri.startswith("file:") and uri.endswith("/" + path), "invalid diagnostic URI")
    history = sample.get("notification_history", [])
    require(isinstance(history, list) and 0 < len(history) <= history_limit, "missing or excessive notification history")
    for notification in history:
        require(type(notification.get("received_ns")) is int and notification["received_ns"] > 0, "invalid notification timestamp")
        frame = notification.get("frame", {})
        require(isinstance(frame, dict) and isinstance(frame.get("method"), str) and "id" not in frame, "invalid notification history frame")
        if (notification["received_ns"] >= raw[0].get("started_ns", 0) and frame["method"] == "textDocument/publishDiagnostics"
                and frame.get("params", {}).get("uri") == uris[UNAFFECTED]):
            require(matches(frame["params"], uris[UNAFFECTED], {"version": 1, "diagnostics": []}), "unaffected module diagnostics changed")
    previous = 0
    for expected, actual in zip(steps, raw):
        require(actual.get("name") == expected["name"] and actual.get("changes") == expected["changes"], "diagnostic edit schedule changed")
        started = actual.get("started_ns")
        require(type(started) is int and started > previous, "diagnostic operation clock inversion")
        observations = actual.get("observations", {})
        require(set(observations) == set(expected["expected"]), "missing diagnostic observation")
        for path, witness in expected["expected"].items():
            observation = observations[path]
            received = observation.get("received_ns")
            require(type(received) is int and received > started, "diagnostic receive clock inversion")
            require(matches(observation.get("params"), uris[path], witness), "wrong diagnostic version/code/message/range")
            require(any(entry["received_ns"] == received and entry["frame"].get("method") == "textDocument/publishDiagnostics"
                        and entry["frame"].get("params") == observation["params"] for entry in history), "observed diagnostic is absent from wire history")
        ended = observations[APP]["received_ns"] if expected["name"] == "open" else max(value["received_ns"] for value in observations.values())
        require(actual.get("completed_ns") == ended and actual.get("duration_ns") == ended - started, "invalid diagnostic latency")
        if expected["metric"] and metric_consistency:
            require(sample.get(expected["metric"]) == actual["duration_ns"], "diagnostic metric disagrees with raw operation")
        previous = max(value["received_ns"] for value in observations.values())


def validate_sample(sample, suite):
    diagnostic = suite in ("diagnostics", "diagnostic-tails")
    workload = {"protocol":"B01.protocol", "diagnostics":"B01+B03.diagnostics", "diagnostic-tails":"B01+B03.diagnostic-tails", "queries":"B02+B03+B04+B05.queries"}[suite]
    completed = 2 + queries.COMPLETED_REQUESTS if suite == "queries" else (8 if diagnostic else 2)
    if suite == "diagnostic-tails":
        completed = 2 + len(diagnostics.frozen_tail_scenario()["steps"])
    require(sample.get("workload") == workload, "sample does not match benchmark suite")
    require(sample.get("errors") == 0 and sample.get("completed") == completed, "unsuccessful process sample")
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
    if suite == "diagnostic-tails":
        validate_diagnostics(sample, diagnostics.frozen_tail_scenario()["steps"], False, 8192)
        for metric, expected in diagnostics.tail_metrics(sample["operations"]).items():
            require(sample.get(metric) == expected, "diagnostic percentile disagrees with raw edit samples")
    elif diagnostic:
        validate_diagnostics(sample)
    elif suite == "queries":
        root = sample.get("input_root")
        require(isinstance(root, str) and Path(root).is_absolute(), "missing query input root")
        queries.validate(sample, Path(root))


def validate_report(report):
    require(report.get("schema") == 1, "unsupported LSP benchmark report schema")
    metadata = report.get("metadata", {})
    for key in MATCH_METADATA:
        require(key in metadata and metadata[key] is not None, f"missing metadata: {key}")
    suite = metadata["suite"]
    expected_unavailable = unavailable(suite)
    if suite == "diagnostics":
        require(report.get("inputs", {}).get("diagnostics") == frozen_scenario(), "diagnostic scenario hash/evidence mismatch")
    if suite == "diagnostic-tails":
        require(report.get("inputs", {}).get("diagnostic-tails") == diagnostics.frozen_tail_scenario(), "diagnostic tail schedule mismatch")
    if suite == "queries":
        require(report.get("inputs", {}).get("queries") == queries.frozen_scenario(), "query scenario hash/evidence mismatch")
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
        validate_sample(sample, suite)
        key = (sample.get("group"), sample.get("image"))
        require(key in expected and key not in actual, "unknown or repeated process sample")
        actual.add(key)
    require(actual == expected, "missing required process samples")
    require(report.get("unavailable_rows") == expected_unavailable, "unmeasured benchmark rows must remain explicitly unavailable")


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
    suite = report["metadata"]["suite"]
    metrics = ("spawn_to_initialize_ns", "initialize_round_trip_ns") + (METRICS if suite == "diagnostics" else diagnostics.TAIL_METRICS if suite == "diagnostic-tails" else queries.METRICS if suite == "queries" else ())
    for metric in metrics:
        groups = {image: {sample["group"]: sample[metric] / 1_000_000
                          for sample in report["samples"] if sample["image"] == image}
                  for image in report["images"]}
        candidate = list(groups["candidate"].values())
        row = {"unit": "ms", "candidate_median": statistics.median(candidate),
               "candidate_range": [min(candidate), max(candidate)],
               "tails": "per-process nearest-rank percentile; 200 warm queries, 40 load requests or 20 diagnostic edits per route; group summary is median/range" if suite in ("queries", "diagnostic-tails") and "_p" in metric else "insufficient samples; no p95 or p99 inferred from process groups"}
        target = 650 if suite == "diagnostic-tails" and metric.endswith("_p95_ns") else {
            "hover_p95_ns": 100, "definition_p95_ns": 100, "completion_p95_ns": 100,
            "implementation_p95_ns": 150,
        }.get(metric) if suite == "queries" else None
        if target is not None:
            row["product_target"] = {"limit_ms": target, "max_observed_group_p95_ms": max(candidate),
                                     "within_all_observed_groups": all(value <= target for value in candidate),
                                     "scope": "observed per-process nearest-rank p95; no universal-hardware claim"}
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
            "scope": ("B01 protocol and first exact diagnostics; B03 body/signature diagnostic invalidation only; no query, cancellation, RSS or worker-retirement acceptance"
                      if suite in ("diagnostics", "diagnostic-tails") else "B02 warm queries, B03 selected/dependent/unaffected edit queries, B04 cold/warm complete implementation index and B05 fixed offered load with bounded slow reader; no server queue, cancellation or retention acceptance" if suite == "queries" else "B01 protocol readiness only; no diagnostic, semantic, RSS or worker-retirement acceptance")}
