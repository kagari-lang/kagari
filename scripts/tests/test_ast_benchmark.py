"""Evidence validation tests for AST characterization and neutral comparisons."""
from copy import deepcopy
import json
import math
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from benchmark_ast import (
    NAMES, ROOT, VALID, aggregate, build_executable, summarize_memory, validate,
    validate_comparison_metadata, validate_memory,
)


def report(scale=1.0):
    rows = []
    for name in NAMES:
        stages = ["lex_drop", "parse_drop", "parse_and_drop_split"]
        if name in VALID:
            stages += ["parse_hir_drop", "field_read"]
        rows.append({"name": name, "bytes": 100, "files": 1, "blake3": "a" * 64,
                     "evidence": {"diagnostics": int(name in ("recovery", "limits")),
                                  "functions": 1, "field_checksum": 107, "lexical_tokens": 60},
                     "stages": [{"stage": stage, "iterations": 5,
                                 "ns_per_workload": [100.0 * scale] * 11,
                                 "drop_ns_per_workload": [10.0 * scale] * 11
                                 if stage == "parse_and_drop_split" else None}
                                for stage in stages]})
    return {"schema": 1, "warmups": 3, "samples": 11, "check_only": False, "workloads": rows}


class EvidenceTests(unittest.TestCase):
    def test_missing_rows_stages_and_samples_are_not_results(self):
        for level in ("row", "stage", "sample"):
            data = report()
            if level == "row":
                data["workloads"].pop()
            elif level == "stage":
                data["workloads"][0]["stages"].pop()
            else:
                data["workloads"][0]["stages"][0]["ns_per_workload"].pop()
            with self.subTest(level=level), self.assertRaises(ValueError):
                validate(data)

    def test_nonfinite_and_nonpositive_timings_are_rejected(self):
        for value in (math.nan, math.inf, 0.0, -1.0):
            data = report()
            data["workloads"][0]["stages"][0]["ns_per_workload"][0] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                validate(data)

    def test_changed_input_or_correctness_evidence_rejects_pair(self):
        baseline = report()
        for field, value in (("blake3", "b" * 64), ("bytes", 101), ("evidence", {"diagnostics": 0})):
            candidate = deepcopy(baseline)
            candidate["workloads"][0][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                aggregate([{"candidate": candidate, "baseline": baseline}])

    def test_ratios_are_normalized_before_process_aggregation(self):
        runs = [{"candidate": report(c), "baseline": report(b)}
                for c, b in ((2, 1), (300, 100), (400, 1000))]
        for row in aggregate(runs):
            self.assertEqual(row["paired_ratio"], 2.0)

    def test_unpaired_characterization_does_not_claim_speedup(self):
        for row in aggregate([{"candidate": report()}]):
            self.assertIsNone(row["paired_ratio"])

    def test_empty_or_partially_paired_runs_are_rejected(self):
        paired = {"candidate": report(), "baseline": report()}
        single = {"candidate": report()}
        for runs in ([], [paired, single], [single, paired]):
            with self.subTest(groups=[list(run) for run in runs]), self.assertRaises(ValueError):
                aggregate(runs)

    def test_memory_summary_reports_levels_differences_and_allocation_deltas(self):
        memory = {image: memory_report() for image in ("candidate", "baseline")}
        for stage in ("parsed", "with_view", "parse_released", "released"):
            memory["candidate"]["workloads"][0][stage] = counters(
                10000, 10000 if stage in ("parsed", "with_view") else 0)
        summary = summarize_memory([{"candidate": report(2), "baseline": report()}], memory)
        self.assertEqual(set(summary), {"memory", "allocations"})
        self.assertEqual(summary["memory"][0], {
            "workload": "small", "stage": "released", "metric": "peak_bytes",
            "baseline": 100, "candidate": 10000, "difference": 9900, "ratio": 100.0})
        self.assertEqual(summary["memory"][2]["difference"], 0)
        self.assertIsNone(summary["memory"][2]["ratio"])
        self.assertEqual(summary["allocations"][2], {
            "workload": "small", "image": "candidate", "stage": "parse_released",
            "requests": 0, "requested_bytes": 0, "frees": 1, "freed_bytes": 10000})
        standalone = summarize_memory([{"candidate": report()}], {"candidate": memory_report()})
        for row in standalone["memory"]:
            self.assertIsNone(row["baseline"])
            self.assertIsNone(row["difference"])
            self.assertIsNone(row["ratio"])
        self.assertEqual({row["image"] for row in standalone["allocations"]}, {"candidate"})

    def test_memory_images_and_workloads_must_match_timing(self):
        runs = [{"candidate": report(), "baseline": report()}]
        for mutation in ("missing_image", "paired_input", "retention_input", "timing_input"):
            memory = {image: memory_report() for image in ("candidate", "baseline")}
            if mutation == "missing_image":
                del memory["baseline"]
            elif mutation == "paired_input":
                memory["baseline"]["workloads"][0]["bytes"] += 1
            elif mutation == "retention_input":
                memory["baseline"]["workloads"][-1]["blake3"] = "b" * 64
            else:
                for image in memory.values():
                    image["workloads"][0]["blake3"] = "b" * 64
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                summarize_memory(runs, memory)

    def test_invalid_or_unmeasured_memory_is_rejected(self):
        for mutation in ("missing", "zero", "leak", "nonfinite", "inconsistent", "nonmonotonic", "no_view"):
            data = memory_report()
            if mutation == "missing":
                data["workloads"].pop()
            elif mutation == "zero":
                data["workloads"][0]["parsed"] = counters(0, 0)
            elif mutation == "leak":
                data["workloads"][0]["released"] = counters(100, 10)
            elif mutation == "nonfinite":
                data["workloads"][0]["parsed"]["peak_bytes"] = math.inf
            elif mutation == "inconsistent":
                data["workloads"][0]["parsed"]["live_bytes"] += 1
            elif mutation == "nonmonotonic":
                data["workloads"][0]["with_view"] = counters(90, 70)
            else:
                data["workloads"][-1]["parse_released"] = counters(100, 0)
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                validate_memory(data)


def counters(requested, live):
    return {"requests": int(requested > 0), "requested_bytes": requested,
            "frees": int(requested > live), "freed_bytes": requested - live,
            "live_bytes": live, "peak_bytes": requested}


def memory_report():
    return {"schema": 1, "diagnostic_only": True, "lifecycles": 4, "workloads": [
        {"name": name, "bytes": 100, "blake3": "a" * 64,
         "parsed": counters(100, 80), "with_view": counters(100, 80),
         "parse_released": counters(100, 70 if name == "inline_retention" else 0),
         "released": counters(100, 0)}
        for name in (*NAMES, "inline_retention")]}


class BuildDiscoveryTests(unittest.TestCase):
    def test_build_requires_one_matching_bench_executable(self):
        executable = ROOT / "target/release/deps/ast_baseline-0123456789abcdef.exe"
        artifact = {"reason": "compiler-artifact", "fresh": True,
                    "target": {"name": "ast_baseline", "kind": ["bench"]},
                    "executable": str(executable)}
        ignored = [
            {"reason": "build-finished", "success": True},
            {**artifact, "target": {"name": "ast_baseline", "kind": ["example"]}},
            {**artifact, "target": {"name": "hir_baseline", "kind": ["bench"]}},
            {**artifact, "executable": None},
        ]
        with patch("benchmark_ast.command") as cargo:
            cargo.return_value = "\n".join(json.dumps(row) for row in [*ignored, artifact])
            self.assertEqual(build_executable(), executable.resolve())
            cargo.assert_called_once_with(
                "cargo", "build", "--release", "--locked", "-p", "kagari-hir",
                "--bench", "ast_baseline", "--message-format=json-render-diagnostics")
            for matches in ([], [artifact, artifact]):
                cargo.return_value = "\n".join(json.dumps(row) for row in [*ignored, *matches])
                with self.subTest(count=len(matches)), self.assertRaisesRegex(
                        ValueError, f"expected one ast_baseline bench executable artifact, got {len(matches)}"):
                    build_executable()


class ComparisonMetadataTests(unittest.TestCase):
    def test_frozen_image_dependencies_and_environment_must_match(self):
        baseline = {
            "executable_sha256": {"candidate": "a" * 64}, "lock_sha256": "b" * 64,
            "rustc": "rustc version", "cargo": "cargo version", "platform": "platform",
            "processor": "processor", "logical_cpus": 8, "profile": "workspace release",
            "features": "default", "parallelism": "default Cargo; single-thread measurement",
            "cache": "resident source", "environment": {"RUSTFLAGS": None},
        }
        candidate = {**baseline, "executable_sha256": {"candidate": "c" * 64, "baseline": "a" * 64}}
        validate_comparison_metadata(baseline, candidate)
        for key in baseline:
            changed = deepcopy(candidate)
            if key == "executable_sha256":
                changed[key]["baseline"] = "d" * 64
            else:
                changed[key] = "different"
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "baseline"):
                validate_comparison_metadata(baseline, changed)


if __name__ == "__main__":
    unittest.main()
