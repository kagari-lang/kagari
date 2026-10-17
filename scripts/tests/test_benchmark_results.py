"""Process evidence must fail closed and preserve the declared paired estimator."""
import copy
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from benchmark_lua import summarize, window_rows, workload_digest
from benchmark_results import ORIGINAL, EvidenceError, compare, expected_rows, validate_report


def metadata(paired=False, runs=1):
    return dict(rustc="test compiler", cargo="test cargo", machine={"cpu": "test"},
                profile="release", features="source,native", parallelism="default",
                environment={}, workload_sha256="a" * 64, lock_sha256="b" * 64,
                suite="original", workload="calls", samples_per_process=11,
                warmups_per_route=3, setup_samples_per_workload=1, setup_scope="per workload",
                interpreter_only=True, check=False, binary_sha256="c" * 64,
                revision="baseline", runs=runs, paired=paired,
                candidate_executable="C:/bench/candidate.exe", baseline_executable="C:/bench/baseline.exe")


def fixture(before=None, after=None):
    before = before or [100] * 7
    after = after or [90] * 7
    baseline = dict(metadata=metadata(), raw=[])
    report = dict(metadata=metadata(True, len(before)), raw=[], baseline=baseline)
    report["metadata"]["baseline_binary_sha256"] = baseline["metadata"]["binary_sha256"]
    for destination in (baseline, report):
        config = destination["metadata"]
        for run in range(config["runs"] * (2 if config["paired"] else 1)):
            pair = run // 2 if config["paired"] else run
            order = ("baseline", "candidate") if pair % 2 == 0 else ("candidate", "baseline")
            variant = order[run % 2] if config["paired"] else "candidate"
            value = (before if variant == "baseline" else after)[pair] if config["paired"] else 100
            for (phase, workload, engine), samples in expected_rows(config).items():
                for sample in range(samples):
                    destination["raw"].append(dict(phase=phase, workload=workload, engine=engine,
                        size=10, batch=1, sample=sample, ns=value,
                        checksum=42 if phase == "execute" else 0,
                        run=run, pair=pair, variant=variant))
    return report


class BenchmarkResultsTests(unittest.TestCase):
    def test_workload_identity_tracks_inputs_without_tracking_runtime_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ("src/main.rs", "src/workloads.rs", "src/profile.rs",
                         "workloads/calls.kgr", "workloads/calls.lua"):
                path = root / "benchmarks/lua-comparison" / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(name)
            with patch("benchmark_lua.ROOT", root):
                original = workload_digest("original")
                (root / "runtime.rs").write_text("changed runtime")
                (root / "benchmarks/lua-comparison/src/profile.rs").write_text("changed profiler")
                self.assertEqual(workload_digest("original"), original)
                (root / "benchmarks/lua-comparison/workloads/calls.kgr").write_text("changed input")
                self.assertNotEqual(workload_digest("original"), original)

    def test_setup_and_lua_changes_are_diagnostics_only(self):
        report = fixture(after=[88] * 7)
        for row in report["raw"]:
            if row["variant"] == "candidate" and (row["phase"] != "execute" or row["engine"] == "lua54"):
                row["ns"] = 200
        result = compare(report, {"calls": 0.90})
        self.assertTrue(result["passed"])
        self.assertEqual(len(result["gates"]), 1)
        self.assertTrue(all(row["ratio"] == 2 for row in result["diagnostics"]))
        self.assertTrue(all("status" not in row for row in result["diagnostics"]))
        # Diagnostic timings do not gate execution, but incomplete evidence still fails.
        report["raw"].pop()
        with self.assertRaises(EvidenceError):
            compare(report)

    def test_image_paths_are_recorded(self):
        for field in ("candidate_executable", "baseline_executable"):
            for replacement in (None, "relative.exe"):
                with self.subTest(field=field, replacement=replacement):
                    report = fixture()
                    report["metadata"][field] = replacement
                    with self.assertRaises(EvidenceError):
                        compare(report)

    def test_summary_uses_process_medians_instead_of_pooling(self):
        distributions = [[0, 0, 0], [1, 10, 10], [2, 11, 11]]
        rows = [dict(phase="execute", workload="calls", engine="kagari_vm", run=run,
                     ns=value, batch=1) for run, values in enumerate(distributions) for value in values]
        self.assertEqual(summarize(rows)[0]["median_ns"], 10)

    def test_paired_median_is_not_ratio_of_aggregate_medians(self):
        result = compare(fixture([10, 10, 10, 100, 100, 100, 100], [9, 9, 9, 9, 90, 90, 90]))
        self.assertTrue(result["passed"])
        self.assertAlmostEqual(result["gates"][0]["ratio"], 0.9)
        self.assertAlmostEqual(result["gates"][0]["min_ratio"], 0.09)
        self.assertAlmostEqual(result["gates"][0]["max_ratio"], 0.9)

    def test_missing_and_duplicate_rows_or_processes_are_rejected(self):
        for operation in (lambda rows: rows.pop(), lambda rows: rows.append(rows[0].copy()),
                          lambda rows: rows.__setitem__(slice(None), [r for r in rows if r["run"] != 3]),
                          lambda rows: rows.__setitem__(slice(None), [r for r in rows if r["phase"] != "execute"])):
            with self.subTest(operation=operation):
                report = fixture()
                operation(report["raw"])
                with self.assertRaises(EvidenceError):
                    compare(report)

    def test_pair_sample_checksum_and_configuration_tampering_is_rejected(self):
        for key, value in (("pair", 1), ("variant", "candidate"), ("sample", 1),
                           ("checksum", -1), ("size", 11), ("batch", 2), ("ns", -1)):
            with self.subTest(key=key):
                report = fixture()
                report["raw"][0][key] = value
                with self.assertRaises(EvidenceError):
                    compare(report)
        for key in ("workload_sha256", "lock_sha256", "rustc", "machine", "baseline_binary_sha256"):
            with self.subTest(key=key):
                report = fixture()
                report["metadata"][key] = "different"
                with self.assertRaises(EvidenceError):
                    compare(report)

    def test_consistently_changed_checksum_is_not_a_new_baseline(self):
        report = fixture()
        for row in report["raw"]:
            if row["phase"] == "execute":
                row["checksum"] = 43
        with self.assertRaises(EvidenceError):
            compare(report)

    def test_thresholds_use_one_ratio_without_an_extra_margin(self):
        self.assertTrue(compare(fixture(after=[88] * 7), {"calls": 0.90})["passed"])
        self.assertFalse(compare(fixture(after=[91] * 7), {"calls": 0.90})["passed"])
        result = compare(fixture(after=[90] * 7), {"calls": 0.90})
        self.assertTrue(result["passed"])
        with self.assertRaises(EvidenceError):
            compare(fixture(), {"absent": 0.9})

    def test_zero_timing_and_insufficient_samples_cannot_pass(self):
        result = compare(fixture(before=[0] * 7))
        self.assertFalse(result["passed"])
        self.assertTrue(all(row["status"] == "inconclusive" for row in result["gates"]))
        with self.assertRaises(EvidenceError):
            compare(fixture(before=[100] * 6, after=[90] * 6))

    def test_baseline_configuration_cannot_be_omitted(self):
        report = fixture()
        del report["baseline"]["metadata"]["machine"]
        with self.assertRaises(EvidenceError):
            compare(report)

    def test_focused_workload_uses_the_full_frozen_contract(self):
        report = fixture(after=[88] * 7)
        baseline = report["baseline"]
        baseline["metadata"]["workload"] = None
        baseline["raw"] = [dict(row, workload=name) for name in ORIGINAL for row in baseline["raw"]]
        self.assertTrue(compare(report, {"calls": 0.9})["passed"])
        baseline["raw"] = [row for row in baseline["raw"] if row["workload"] != "fibonacci"]
        with self.assertRaises(EvidenceError):
            compare(report)

    def test_forms_require_separate_setup_and_all_expected_workloads(self):
        report = fixture()["baseline"]
        report["metadata"].update(suite="forms", workload=None, setup_samples_per_workload=1)
        report["raw"] = []
        for (phase, workload, engine), count in expected_rows(report["metadata"]).items():
            for sample in range(count):
                report["raw"].append(dict(phase=phase, workload=workload, engine=engine,
                    size=5000, batch=1, ns=100, checksum=42 if phase == "execute" else 0,
                    sample=sample, run=0, pair=0, variant="candidate"))
        validate_report(report)
        missing = copy.deepcopy(report)
        missing["raw"] = [row for row in missing["raw"] if row["workload"] != "forms_helper"]
        with self.assertRaises(EvidenceError):
            validate_report(missing)

    def test_session_controls_keep_retirement_and_collection_separate(self):
        report = fixture()["baseline"]
        report["metadata"].update(suite="controls", workload=None, setup_samples_per_workload=1)
        report["raw"] = []
        for (phase, workload, engine), count in expected_rows(report["metadata"]).items():
            for sample in range(count):
                report["raw"].append(dict(phase=phase, workload=workload, engine=engine,
                    size=256, batch=1, ns=100, checksum=256 if phase == "retire" else 0,
                    sample=sample, run=0, pair=0, variant="candidate"))
        validate_report(report)
        report["raw"] = [row for row in report["raw"] if row["phase"] != "collect"]
        with self.assertRaises(EvidenceError):
            validate_report(report)

    def test_window_harness_framing_and_batched_regions(self):
        rows = list(window_rows(["running 1 test\n",
            "test probe ... WINDOW,32,8,tail,32,0,3200,6400,9600\n", "ok\n"]))
        self.assertEqual([row["phase"] for row in rows], ["admit", "retire", "fragment"])
        self.assertEqual([int(row["ns"]) / int(row["batch"]) for row in rows], [100, 200, 300])
        report = fixture()["baseline"]
        report["metadata"].update(suite="windows", workload=None, setup_samples_per_workload=1)
        report["raw"] = [dict(phase=phase, workload=workload, engine=engine,
            size=256, batch=32, ns=3200, checksum=0, sample=sample, run=0, pair=0, variant="candidate")
            for (phase, workload, engine), count in expected_rows(report["metadata"]).items()
            for sample in range(count)]
        validate_report(report)
        report["raw"][0]["batch"] = 1
        with self.assertRaises(EvidenceError):
            validate_report(report)


if __name__ == "__main__":
    unittest.main()
