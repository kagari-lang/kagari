"""Evidence rejection tests for the AST migration measurement protocol."""
from copy import deepcopy
import math
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from benchmark_ast import NAMES, VALID, acceptance, aggregate, compare_locks, validate, validate_memory


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

    def test_acceptance_requires_pairs_memory_budgets_and_noise(self):
        baseline, candidate = report(), report(0.7)
        memory = {name: memory_report() for name in ("candidate", "baseline")}
        runs = [{"candidate": candidate, "baseline": baseline} for _ in range(7)]
        self.assertTrue(acceptance(runs, memory)["passed"])
        self.assertFalse(acceptance(runs[:1], memory)["passed"])
        slow = [{"candidate": report(0.9), "baseline": baseline} for _ in range(7)]
        self.assertFalse(acceptance(slow, memory)["passed"])
        noisy = deepcopy(runs)
        noisy[0]["baseline"] = report(1.3)
        self.assertFalse(acceptance(noisy, memory)["passed"])
        inflated = deepcopy(memory)
        for stage in ("parsed", "with_view", "parse_released", "released"):
            inflated["candidate"]["workloads"][0][stage] = counters(10000, 10000 if stage in ("parsed", "with_view") else 0)
        self.assertFalse(acceptance(runs, inflated)["passed"])

    def test_near_threshold_requires_independent_passing_repeat(self):
        memory = {name: memory_report() for name in ("candidate", "baseline")}
        runs = [{"candidate": report(0.79), "baseline": report()} for _ in range(7)]
        first = acceptance(runs, memory)
        self.assertFalse(first["passed"])
        first["failures"] = []
        self.assertTrue(acceptance(runs, memory, first)["passed"])
        first["failures"] = ["unresolved noise"]
        self.assertFalse(acceptance(runs, memory, first)["passed"])

    def test_unimplemented_leaked_or_missing_memory_cannot_pass(self):
        for mutation in ("missing", "zero", "leak", "nonfinite", "no_view"):
            data = memory_report()
            if mutation == "missing":
                data["workloads"].pop()
            elif mutation == "zero":
                data["workloads"][0]["parsed"] = counters(0, 0)
            elif mutation == "leak":
                data["workloads"][0]["released"] = counters(100, 10)
            elif mutation == "nonfinite":
                data["workloads"][0]["parsed"]["peak_bytes"] = math.inf
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


class DependencyTests(unittest.TestCase):
    BEFORE = b'''version = 4
[[package]]
name = "kagari-syntax"
version = "0.1.0"
dependencies = ["rowan", "shared"]
[[package]]
name = "kagari-format"
version = "0.1.0"
dependencies = ["rowan", "shared"]
[[package]]
name = "rowan"
version = "0.17.0"
dependencies = ["orphan", "shared"]
[[package]]
name = "orphan"
version = "1.0.0"
[[package]]
name = "shared"
version = "1.0.0"
checksum = "unchanged"
'''
    AFTER = b'''version = 4
[[package]]
name = "kagari-syntax"
version = "0.1.0"
dependencies = ["shared"]
[[package]]
name = "kagari-format"
version = "0.1.0"
dependencies = ["shared"]
[[package]]
name = "shared"
version = "1.0.0"
checksum = "unchanged"
'''

    def test_rowan_and_orphans_can_be_removed(self):
        self.assertEqual(compare_locks(self.BEFORE, self.AFTER), ["orphan 1.0.0", "rowan 0.17.0"])

    def test_equivalent_dependency_spelling_is_not_an_upgrade(self):
        before = self.BEFORE.replace(b'"shared"]', b'"shared 1.0.0"]')
        self.assertEqual(compare_locks(before, self.AFTER), ["orphan 1.0.0", "rowan 0.17.0"])

    def test_retained_dependency_changes_are_rejected(self):
        for changed in (self.AFTER.replace(b"unchanged", b"changed"),
                        self.AFTER.replace(b'"1.0.0"', b'"2.0.0"'),
                        self.AFTER.replace(b'["shared"]', b'[]', 1)):
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                compare_locks(self.BEFORE, changed)

    def test_still_referenced_package_cannot_be_removed(self):
        changed = self.AFTER[:self.AFTER.rfind(b"[[package]]")]
        with self.assertRaises(ValueError):
            compare_locks(self.BEFORE, changed)

    def test_unrelated_orphan_removal_is_rejected(self):
        before = self.BEFORE + b'\n[[package]]\nname = "unrelated"\nversion = "1.0.0"\n'
        with self.assertRaises(ValueError):
            compare_locks(before, self.AFTER)


if __name__ == "__main__":
    unittest.main()
