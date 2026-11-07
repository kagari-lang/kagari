"""Validation and summaries for the fixed HIR driver, not a benchmark framework."""
from __future__ import annotations

import math
import re
from statistics import median

WORKLOADS = ("plain", "generic", "dependency_edit")
CHECKPOINTS = ("baseline", "old_snapshot", "both_snapshots", "old_released",
               "new_released", "database_released")
COUNTS = ("requests", "requested_bytes", "frees", "freed_bytes", "live_bytes", "peak_bytes")
LIFECYCLE = ("install", "old_snapshot", "old_check_program", "source_edit", "new_snapshot",
             "new_check_program", "drop_old", "drop_new", "drop_database", "measured_sum_ns")
METRICS = {
    "staged": ("install", "declarations", "signatures_after_declarations",
               "snapshot_after_signatures", "check_program", "drop_results", "drop_database"),
    "snapshot": ("install", "snapshot", "drop_results", "drop_database"),
    "checked": ("install", "snapshot", "check_program", "drop_results", "drop_database"),
    "warm": ("install", "preparation_snapshot", "unchanged_snapshot", "drop_results", "drop_database"),
    "body_edit": ("install", "preparation_snapshot", "source_edit", "edited_snapshot",
                  "check_program", "drop_results", "drop_database"),
    "selected": ("install", "declarations", "owner_lookup", "selected_body"),
    "selected_warm": ("install", "declarations", "owner_lookup", "preparation_body", "unchanged_selected_body"),
    "selected_signature_edit": ("install", "declarations", "owner_lookup", "preparation_body",
                                "source_edit", "edited_selected_body"),
    "lifecycle": LIFECYCLE,
}
METRICS["signature_edit"] = METRICS["body_edit"]
MANIFEST_KEYS = {"schema", "workload", "size", "input_hash", "source_bytes", "source_files",
                 "scenario_names", "generation", "logical_user_modules", "source_bytes_scope",
                 "input_hash_algorithm"}
MATCH_METADATA = ("machine", "platform", "processor", "logical_cpus", "rustc", "cargo",
                  "profile", "features", "parallelism", "environment", "lock_sha256",
                  "harness_sha256", "tooling_sha256", "cargo_config_sha256", "sample_protocol", "cases")
OBSERVATIONS = {"construction_stats", "user_file_results_reused", "whole_result_reused"}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def number(value, label: str, *, integer: bool = False) -> None:
    valid_type = type(value) is int if integer else type(value) in (int, float)
    require(valid_type and math.isfinite(value) and value >= 0, f"invalid {label}: {value!r}")


def digest(value, label: str) -> None:
    require(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) is not None,
            f"invalid {label}")


def scenario_names(workload: str, size: int, mode: str) -> list[str]:
    if mode == "memory":
        return ["lifecycle"]
    names = ["staged", "snapshot", "checked", "warm"]
    if workload == "dependency_edit":
        names += ["body_edit", "signature_edit", "selected", "selected_warm",
                  "selected_signature_edit", "lifecycle"]
    elif size == 96:
        names += (["body_edit"] if workload == "plain" else []) + ["lifecycle"]
    return names


def validate_manifest(manifest: dict, case: dict) -> None:
    require(set(manifest) == MANIFEST_KEYS, "incomplete input manifest")
    require(case["mode"] in ("timing", "memory") and case["workload"] in WORKLOADS
            and case["size"] in (24, 96, 384), "invalid selected case")
    require(case["workload"] != "dependency_edit" or case["size"] == 96, "dependency requires size 96")
    require(manifest["schema"] == 1 and type(manifest["schema"]) is int, "unexpected driver schema")
    require(all(manifest[k] == case[k] for k in ("workload", "size")), "manifest case mismatch")
    require(manifest["scenario_names"] == scenario_names(**case), "missing/reordered manifest scenarios")
    digest(manifest["input_hash"], "input hash")
    for key in ("source_bytes", "source_files", "logical_user_modules", "size"):
        number(manifest[key], key, integer=True)
        require(manifest[key] > 0, f"empty {key}")
    files = 5 if case["workload"] == "dependency_edit" else 1
    require(manifest["source_files"] == manifest["logical_user_modules"] == files,
            "incorrect source/module count")
    for key in ("generation", "source_bytes_scope", "input_hash_algorithm"):
        require(isinstance(manifest[key], str) and bool(manifest[key]), f"missing {key}")


def validate_manifests(manifests: dict, cases: list[dict]) -> None:
    require(bool(cases) and len({case_key(c) for c in cases}) == len(cases), "missing/duplicate cases")
    require(set(manifests) == {case_key(c) for c in cases}, "manifest case inventory mismatch")
    identities = {}
    for case in cases:
        manifest = manifests[case_key(case)]
        validate_manifest(manifest, case)
        identity = {k: v for k, v in manifest.items() if k != "scenario_names"}
        key = (case["workload"], case["size"])
        require(identities.setdefault(key, identity) == identity, "timing/memory manifest mismatch")


def case_key(case: dict) -> str:
    return f'{case["mode"]}/{case["workload"]}/{case["size"]}'


def summary_evidence(value: dict, manifest: dict, signature: bool) -> None:
    require(set(value) == {"diagnostics", "user_functions", "provider_functions", "logical_user_modules",
                           "provider_modules", "representative_type"}, "incomplete summary evidence")
    for key in set(value) - {"representative_type"}:
        number(value[key], key, integer=True)
    require(value["diagnostics"] == 0, "unexpected diagnostics")
    require(value["user_functions"] == manifest["size"] + (4 if manifest["workload"] == "generic" else 1),
            "user function evidence mismatch")
    require(value["logical_user_modules"] == manifest["source_files"]
            and value["provider_functions"] > 0 and value["provider_modules"] > 0,
            "module/provider evidence mismatch")
    require(value["representative_type"] == ("i64" if signature else "i32"), "type evidence mismatch")


def validate_evidence(evidence: dict, name: str, manifest: dict, mode: str) -> dict:
    require(type(evidence.get("diagnostics")) is int and evidence["diagnostics"] == 0,
            "missing/invalid diagnostics evidence")
    signature = name in ("signature_edit", "selected_signature_edit")
    selected = name.startswith("selected")
    edited = name in ("body_edit", "signature_edit", "selected_signature_edit", "lifecycle")
    required = {"diagnostics"}
    if selected:
        required |= {"representative_type", "selected_function", "construction_stats",
                     "construction_stats_scope", "owner_preparation", "full_validation_after_measurement",
                     "checked_modules"}
        require(evidence.get("selected_function") == "consumer_a::f0", "selected identity mismatch")
        require(evidence.get("representative_type") == ("i64" if signature else "i32"), "selected type mismatch")
        summary_evidence(evidence["full_validation_after_measurement"], manifest, signature)
        stats = evidence["construction_stats"]
        require(set(stats) == ({"first"} if name == "selected" else {"first", "second"}),
                "missing construction observations")
        for counts in stats.values():
            require(set(counts) == {"checked_bodies", "reused_bodies"}, "invalid construction observations")
            for value in counts.values():
                number(value, "construction count", integer=True)
        if name != "selected":
            required.add("whole_result_reused")
    elif edited:
        required |= {"old", "new", "checked_modules"}
        summary_evidence(evidence["old"], manifest, False)
        summary_evidence(evidence["new"], manifest, signature)
    else:
        required.add("summary")
        summary_evidence(evidence["summary"], manifest, False)
    if name in ("staged", "checked"):
        required.add("checked_modules")
    if name == "staged":
        required.add("route")
    if name in ("warm", "selected_warm"):
        required.add("unchanged_source")
        require(evidence.get("unchanged_source") is True, "unchanged source evidence missing")
    if name == "warm":
        required.add("user_file_results_reused")
    if edited:
        required.add("edit")
        dependency = manifest["workload"] == "dependency_edit"
        expected = {"changed_source": "shared.kgr" if dependency else "profile.kgr",
                    "effective_edit": True, "edit_kind": "signature" if signature else "body",
                    "edit": "value return i32 -> i64" if signature else
                            ("value literal 1 -> 9" if dependency else "first x + 0 -> x + 9"),
                    "old_type": "i32", "new_type": "i64" if signature else "i32", "old_snapshot_valid": True}
        require(evidence.get("edit") == expected, "changed source/signature/edit evidence mismatch")
    if name == "lifecycle":
        required |= {"lifetime", "source_setup", "measurement", "residual"}
    if mode == "memory":
        required |= {"allocation_accounting", "timing_is_speed_evidence"}
        require(evidence.get("timing_is_speed_evidence") is False, "instrumented timing is not speed evidence")
    require(set(evidence) == required, f"incomplete/unexpected {name} evidence")
    for key in ("whole_result_reused", "user_file_results_reused"):
        if key in evidence:
            require(type(evidence[key]) is bool, f"invalid {key} observation")
    if "checked_modules" in evidence:
        number(evidence["checked_modules"], "checked_modules", integer=True)
        summary = evidence.get("summary", evidence.get("new", evidence.get("full_validation_after_measurement")))
        require(evidence["checked_modules"] == summary["logical_user_modules"] + summary["provider_modules"],
                "checked module evidence mismatch")
    for key in ("construction_stats_scope", "owner_preparation", "route", "lifetime", "source_setup",
                "measurement", "residual", "allocation_accounting"):
        if key in evidence:
            require(isinstance(evidence[key], str) and bool(evidence[key]), f"empty {key}")
    # Allocation/reuse observations are retained raw but are not semantic equality oracles.
    return {k: v for k, v in evidence.items() if k not in OBSERVATIONS}


def validate_report(report: dict, manifest: dict, case: dict, check: bool = False) -> tuple[dict, dict]:
    validate_manifest(manifest, case)
    base = {k: v for k, v in manifest.items() if k != "scenario_names"}
    require(set(report) == set(base) | {"mode", "check_only", "warmups", "provider_setup_ns", "scenarios"},
            "incomplete/unexpected report fields")
    require(all(type(report[k]) is type(v) and report[k] == v for k, v in base.items()),
            "report/manifest identity mismatch")
    require(report["mode"] == case["mode"] and report["check_only"] is check,
            "report mode/check protocol mismatch")
    require(type(report["warmups"]) is int and report["warmups"] == int(case["mode"] == "timing" and not check),
            "warmup protocol mismatch")
    number(report["provider_setup_ns"], "provider setup duration")
    rows = report["scenarios"]
    require([r["name"] for r in rows] == manifest["scenario_names"], "missing/duplicate/reordered scenarios")
    values, evidence = {}, {}
    semantic_summaries = []
    checked_modules = []
    if case["mode"] == "timing":
        values["provider_setup_ns"] = report["provider_setup_ns"]
    for row in rows:
        name = row["name"]
        evidence[name] = validate_evidence(row["evidence"], name, manifest, case["mode"])
        for field in ("summary", "old", "new", "full_validation_after_measurement"):
            if field in evidence[name]:
                semantic_summaries.append({k: v for k, v in evidence[name][field].items() if k != "representative_type"})
        if "checked_modules" in evidence[name]:
            checked_modules.append(evidence[name]["checked_modules"])
        if case["mode"] == "timing":
            require(set(row) == {"name", "metrics_ns", "evidence"}, "invalid timing row")
            metrics = row["metrics_ns"]
            require(set(metrics) == set(METRICS[name]), f"missing/unexpected {name} timing metrics")
            for key, value in metrics.items():
                number(value, f"{name}/{key}")
                values[f"{name}/{key}_ns"] = value
            if name == "lifecycle":
                require(math.isclose(metrics["measured_sum_ns"], sum(metrics[k] for k in LIFECYCLE[:-1]),
                                     rel_tol=1e-12, abs_tol=1e-6), "invalid lifecycle sum")
        else:
            require(set(row) == {"name", "checkpoints", "evidence"}, "invalid memory row")
            checkpoints = row["checkpoints"]
            require([p["name"] for p in checkpoints] == list(CHECKPOINTS), "missing/reordered retained checkpoints")
            baseline = checkpoints[0]
            previous = None
            for point in checkpoints:
                require(set(point) == {"name", *COUNTS}, "missing allocation counts")
                for key in COUNTS:
                    number(point[key], key, integer=True)
                require(point["live_bytes"] == point["requested_bytes"] - point["freed_bytes"]
                        and point["peak_bytes"] >= point["live_bytes"], "inconsistent deallocation/live/peak accounting")
                if previous:
                    require(all(point[k] >= previous[k] for k in COUNTS if k != "live_bytes"),
                            "nonmonotonic cumulative allocation counts")
                for key in COUNTS:
                    values[f'{point["name"]}/raw/{key}'] = point[key]
                    values[f'{point["name"]}/baseline_delta/{key}'] = point[key] - baseline[key]
                previous = point
            require(baseline["peak_bytes"] == baseline["live_bytes"], "peak window must start at baseline live bytes")
    require(all(value == semantic_summaries[0] for value in semantic_summaries),
            "inconsistent semantic evidence across direct/staged/edit routes")
    require(all(value == checked_modules[0] for value in checked_modules), "inconsistent checked closure evidence")
    return values, evidence


def validate_metadata(left: dict, right: dict) -> None:
    for key in MATCH_METADATA:
        require(key in left and key in right and left[key] == right[key], f"incompatible metadata: {key}")


def stats(values: list) -> dict:
    require(bool(values), "empty sample vector")
    middle = median(values)
    return {"samples": values, "median": middle, "mad": median([abs(v - middle) for v in values]),
            "min": min(values), "max": max(values)}


def validate_run(run: dict, *, performance: bool = True) -> dict:
    metadata = run["metadata"]
    validate_metadata(metadata, metadata)
    check = metadata["check_only"]
    require(type(check) is bool and (not performance or not check), "correctness checks are not performance samples")
    count = metadata["runs"]
    require(type(count) is int and count > 0 and (not check or count == 1), "invalid process sample count")
    cases = metadata["cases"]
    validate_manifests(run["manifests"], cases)
    images = set(run["images"])
    require(images in ({"candidate"}, {"candidate", "baseline"}), "invalid image inventory")
    if "baseline" in images:
        require(not check, "check cannot contain performance pairs")
        validate_metadata(run["baseline_metadata"], metadata)
    samples = run["samples"]
    require(len(samples) == len(cases) * count, "partial process samples/pairs")
    references, series, lifecycles = {}, {}, {}
    for position, sample in enumerate(samples):
        index, case_index = divmod(position, len(cases))
        case = cases[case_index]
        key = case_key(case)
        require(sample["case"] == key and sample["index"] == index, "sample ordering/inventory mismatch")
        order = (["baseline", "candidate"] if index % 2 == 0 else ["candidate", "baseline"]) if len(images) == 2 else ["candidate"]
        require(sample["order"] == order and set(sample["reports"]) == images, "partial/nonalternating process pair")
        require(sample["manifest"] == run["manifests"][key], "sample manifest changed")
        parsed = {}
        for image in order:
            values, evidence = validate_report(sample["reports"][image], sample["manifest"], case, check)
            require(references.setdefault(key, evidence) == evidence, "source/signature/semantic evidence changed")
            if "lifecycle" in evidence:
                common = {k: v for k, v in evidence["lifecycle"].items()
                          if k not in ("allocation_accounting", "timing_is_speed_evidence")}
                identity = (case["workload"], case["size"])
                require(lifecycles.setdefault(identity, common) == common, "timing/memory lifecycle evidence mismatch")
            parsed[image] = values
        for metric in parsed["candidate"]:
            row = series.setdefault((key, metric), {image: [] for image in images})
            for image in images:
                row[image].append(parsed[image][metric])
    return series


def aggregate(run: dict) -> dict:
    series = validate_run(run)
    rows, warnings = [], []
    for (case, metric), images in series.items():
        row = {"case": case, "metric": metric, **{k: stats(v) for k, v in images.items()}}
        if case.startswith("timing/") and "baseline" in images:
            ratios = [c / b for c, b in zip(images["candidate"], images["baseline"]) if b != 0]
            row["paired_candidate_over_baseline"] = stats(ratios) if len(ratios) == len(images["baseline"]) else None
            if row["paired_candidate_over_baseline"] is None:
                warnings.append(f"{case}/{metric}: zero baseline duration; paired ratio undefined")
        for image in images:
            sample = row[image]
            if case.startswith("timing/") and sample["mad"] > abs(sample["median"]) * 0.1:
                warnings.append(f"{case}/{metric}/{image}: noisy samples (MAD > 10% of median)")
        rows.append(row)
    if run["metadata"]["runs"] < 5:
        warnings.insert(0, "Fewer than five independent process samples; descriptive results only")
    return {"comparison": "fresh alternating paired" if "baseline" in run["images"] else "candidate only",
            "ratio_direction": "candidate / baseline (timing only; lower is faster)",
            "memory_units": "requested allocator bytes/counts, not RSS; baseline_delta is within each process",
            "warnings": warnings, "rows": rows}


def compare_saved(baseline: dict, candidate: dict) -> dict:
    left, right = validate_run(baseline), validate_run(candidate)
    validate_metadata(baseline["metadata"], candidate["metadata"])
    require(baseline["manifests"] == candidate["manifests"], "incompatible saved manifests")
    # Validate substantive evidence across campaigns, not only within each campaign.
    for a, b in zip(baseline["samples"], candidate["samples"]):
        case = next(c for c in candidate["metadata"]["cases"] if case_key(c) == a["case"])
        _, ae = validate_report(a["reports"]["candidate"], a["manifest"], case)
        _, be = validate_report(b["reports"]["candidate"], b["manifest"], case)
        require(ae == be, "saved semantic evidence mismatch")
    rows = []
    for key, values in right.items():
        before, after = stats(left[key]["candidate"]), stats(values["candidate"])
        row = {"case": key[0], "metric": key[1], "baseline": before, "candidate": after}
        if key[0].startswith("timing/"):
            row["unpaired_median_candidate_over_baseline"] = after["median"] / before["median"] if before["median"] else None
        rows.append(row)
    return {"comparison": "saved-only unpaired medians; not fresh paired evidence",
            "warnings": ["Historical process samples are unpaired"] + aggregate(baseline)["warnings"] + aggregate(candidate)["warnings"],
            "rows": rows}
