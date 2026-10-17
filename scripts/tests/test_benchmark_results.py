"""Process evidence must fail closed and preserve the declared paired estimator."""
import copy
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from benchmark_lua import summarize, window_rows
from benchmark_reference import SOURCE, load
from benchmark_results import ORIGINAL, EvidenceError, compare, expected_rows, validate_extension, validate_report


def metadata(paired=False, runs=1):
    return dict(rustc="test compiler", cargo="test cargo", machine={"cpu": "test"},
                profile="release", features="source,native", parallelism="default",
                environment={}, source_sha256={"fixture": "a" * 64,
                    "benchmarks/lua-comparison/workloads/calls.lua": "a" * 64}, lock_sha256="b" * 64,
                suite="original", workload="calls", samples_per_process=11,
                warmups_per_route=3, setup_samples_per_workload=3, setup_scope="per workload",
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
    config = report["metadata"]
    config["lua_setup_reference"] = dict(executable="C:/bench/reference.exe", binary_sha256="d" * 64,
        manifest_sha256="e" * 64, source_sha256={SOURCE: "f" * 64}, features="mlua lua54,vendored",
        batch=32, samples=3, **{key: config[key] for key in ("rustc", "cargo", "profile", "lock_sha256", "environment")})
    report.update(reference_raw=[], reference_processes=[])
    for run in range(config["runs"] * 2):
        subject = next(row for row in report["raw"] if row["run"] == run and row["phase"] == "execute")
        value = (before if subject["variant"] == "baseline" else after)[subject["pair"]]
        identity = {key: subject[key] for key in ("run", "pair", "variant", "workload")}
        source_key = "benchmarks/lua-comparison/workloads/calls.lua"
        report["reference_processes"].append(dict(**identity, executable="C:/bench/reference.exe",
            binary_sha256="d" * 64, source=f"C:/repo/{source_key}", source_key=source_key,
            source_sha256="a" * 64, execution_batch=1, expected_checksum=42))
        for phase in ("engine_init", "source_to_chunk", "module_init", "validate"):
            for sample in range(3):
                report["reference_raw"].append(dict(**identity, phase=phase, engine="lua54", size=10,
                    batch=32, sample=sample, ns=0 if phase == "validate" else value * 32,
                    checksum=42 if phase == "validate" else 0))
    return report


class BenchmarkResultsTests(unittest.TestCase):
    def test_frozen_reference_loader_rejects_changed_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / SOURCE
            source.parent.mkdir(parents=True)
            source.write_bytes(b"reference source")
            image = root / "reference.exe"
            image.write_bytes(b"frozen image")
            subject = metadata()
            config = dict(fixture()["metadata"]["lua_setup_reference"], executable=str(image),
                          binary_sha256=hashlib.sha256(image.read_bytes()).hexdigest(),
                          source_sha256={SOURCE: hashlib.sha256(source.read_bytes()).hexdigest()})
            Path(str(image) + ".json").write_text(json.dumps(config), encoding="utf-8")
            self.assertEqual(load(root, image, subject)["binary_sha256"], config["binary_sha256"])
            image.write_bytes(b"another image")
            with self.assertRaises(ValueError):
                load(root, image, subject)
            image.write_bytes(b"frozen image")
            source.write_bytes(b"changed source")
            with self.assertRaises(ValueError):
                load(root, image, subject)

    def test_reference_owner_is_independent_and_embedded_failures_remain_visible(self):
        report = fixture(after=[88] * 7)
        for row in report["raw"]:
            if row["engine"] == "lua54" and row["phase"] != "execute" and row["variant"] == "candidate":
                row["ns"] = 200
        result = compare(report, {"calls": 0.90})
        self.assertTrue(result["passed"])
        self.assertTrue(all(row["status"] == "fail" for row in result["embedded_lua_setup_diagnostics"]["gates"]))
        controls = [row for row in result["gates"] if row["origin"] == "independent_lua_setup"]
        self.assertEqual(len(controls), 3)
        self.assertTrue(all(row["paired_ratio"] == 0.88 for row in controls))
        for row in report["reference_raw"]:
            if row["variant"] == "candidate" and row["phase"] != "validate":
                row["ns"] = 200 * 32
        self.assertFalse(compare(report)["passed"])

    def test_reference_provenance_samples_and_checksum_fail_closed(self):
        changes = [
            lambda r: r["metadata"].pop("lua_setup_reference"),
            lambda r: r.pop("reference_raw"),
            lambda r: r["reference_raw"].pop(),
            lambda r: r["reference_raw"].append(r["reference_raw"][0].copy()),
            lambda r: r["reference_processes"].pop(),
            lambda r: r["reference_processes"].append(r["reference_processes"][0].copy()),
            lambda r: r["reference_processes"][0].update(executable="C:/copy/reference.exe"),
            lambda r: r["reference_processes"][0].update(binary_sha256="0" * 64),
            lambda r: r["reference_processes"][0].update(source_sha256="0" * 64),
            lambda r: r["reference_processes"][0].update(expected_checksum=43),
            lambda r: r["reference_processes"][0].update(run=False),
            lambda r: r["reference_raw"][0].update(pair=1),
            lambda r: r["reference_raw"][0].update(variant="candidate"),
            lambda r: r["reference_raw"][0].update(batch=1),
            lambda r: r["reference_raw"][0].update(sample=1),
            lambda r: r["reference_raw"][0].update(ns=-1),
            lambda r: next(row for row in r["reference_raw"] if row["phase"] == "validate").update(checksum=43),
        ]
        for change in changes:
            with self.subTest(change=change):
                report = fixture()
                change(report)
                with self.assertRaises(EvidenceError):
                    compare(report)

    def test_reference_identity_is_fixed_through_extensions(self):
        previous = fixture()
        current = dict(previous["metadata"], runs=21)
        for key, value in (("executable", "C:/copy/reference.exe"), ("binary_sha256", "0" * 64),
                           ("manifest_sha256", "0" * 64), ("batch", 64), ("samples", 11)):
            with self.subTest(key=key):
                changed = copy.deepcopy(current)
                changed["lua_setup_reference"][key] = value
                with self.assertRaises(EvidenceError):
                    validate_extension(changed, previous)
        previous["reference_raw"].pop()
        with self.assertRaises(EvidenceError):
            validate_extension(current, previous)

    def test_extension_preserves_binary_configuration_and_all_prior_pairs(self):
        previous = fixture()
        current = dict(previous["metadata"], runs=21)
        self.assertEqual(validate_extension(current, previous), 7)
        for field, value in (("runs", 7), ("binary_sha256", "d" * 64),
                             ("baseline_binary_sha256", "d" * 64), ("workload", "fibonacci"),
                             ("candidate_executable", "C:/saved/same-bytes.exe"),
                             ("baseline_executable", "C:/saved/baseline.exe")):
            with self.subTest(field=field), self.assertRaises(EvidenceError):
                validate_extension(dict(current, **{field: value}), previous)
        previous["raw"].pop()
        with self.assertRaises(EvidenceError):
            validate_extension(current, previous)

    def test_image_paths_are_required_and_preserved_through_continuation_history(self):
        report = fixture()
        original = dict(report["metadata"], runs=1)
        intermediate = dict(report["metadata"], runs=3, continued_metadata=original)
        report["metadata"]["continued_metadata"] = intermediate
        self.assertTrue(compare(report)["passed"])
        for field in ("candidate_executable", "baseline_executable"):
            for replacement in (None, "relative.exe", "C:/relocated/same-bytes.exe"):
                with self.subTest(field=field, replacement=replacement):
                    changed = copy.deepcopy(report)
                    changed["metadata"]["continued_metadata"]["continued_metadata"][field] = replacement
                    with self.assertRaises(EvidenceError):
                        compare(changed)
            changed = copy.deepcopy(report)
            del changed["metadata"][field]
            with self.assertRaises(EvidenceError):
                compare(changed)

    def test_summary_uses_process_medians_instead_of_pooling(self):
        distributions = [[0, 0, 0], [1, 10, 10], [2, 11, 11]]
        rows = [dict(phase="execute", workload="calls", engine="kagari_vm", run=run,
                     ns=value, batch=1) for run, values in enumerate(distributions) for value in values]
        self.assertEqual(summarize(rows)[0]["median_ns"], 10)

    def test_both_estimators_and_alternating_process_order(self):
        result = compare(fixture([10, 10, 10, 100, 100, 100, 100], [9, 9, 9, 9, 90, 90, 90]))
        self.assertTrue(result["passed"])
        self.assertAlmostEqual(result["gates"][0]["aggregate_ratio"], 0.09)
        self.assertAlmostEqual(result["gates"][0]["paired_ratio"], 0.9)

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
        for key in ("source_sha256", "lock_sha256", "rustc", "machine", "baseline_binary_sha256"):
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

    def test_thresholds_and_near_gate_noise(self):
        self.assertTrue(compare(fixture(after=[88] * 7), {"calls": 0.90})["passed"])
        self.assertFalse(compare(fixture(after=[91] * 7), {"calls": 0.90})["passed"])
        result = compare(fixture(after=[90] * 7), {"calls": 0.90})
        self.assertTrue(any(row["status"] == "inconclusive" for row in result["gates"]))
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
