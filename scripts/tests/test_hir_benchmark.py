"""Small offline contract tests: malformed archives must never become speed evidence."""
from copy import deepcopy

from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
import benchmark_hir as runner
import hir_benchmark_results as results


def manifest(case):
    files = 5 if case["workload"] == "dependency_edit" else 1
    return {"schema": 1, "workload": case["workload"], "size": case["size"],
            "input_hash": "a" * 64, "source_bytes": 12000, "source_files": files,
            "scenario_names": results.scenario_names(**case), "generation": "fixed test workload",
            "logical_user_modules": files, "source_bytes_scope": "original user sources",
            "input_hash_algorithm": "blake3 length-framed inputs"}


def report(case, inputs, check=False):
    dependency = case["workload"] == "dependency_edit"

    def summary(signature=False):
        return {"diagnostics": 0, "user_functions": case["size"] + (4 if case["workload"] == "generic" else 1),
                "provider_functions": 10, "provider_modules": 2,
                "logical_user_modules": inputs["source_files"], "representative_type": "i64" if signature else "i32"}

    rows = []
    for name in inputs["scenario_names"]:
        signature = name in ("signature_edit", "selected_signature_edit")
        evidence = {"diagnostics": 0}
        if name.startswith("selected"):
            evidence.update(representative_type="i64" if signature else "i32", selected_function="consumer_a::f0",
                            construction_stats={"first": {"checked_bodies": 1, "reused_bodies": 0}},
                            construction_stats_scope="result construction, not query work",
                            owner_preparation="declarations only",
                            full_validation_after_measurement=summary(signature), checked_modules=7)
            if name != "selected":
                evidence["construction_stats"]["second"] = {"checked_bodies": 1, "reused_bodies": 0}
                evidence["whole_result_reused"] = not signature
        elif name in ("body_edit", "signature_edit", "lifecycle"):
            evidence.update(old=summary(), new=summary(signature), checked_modules=inputs["source_files"] + 2)
        else:
            evidence["summary"] = summary()
        if name in ("staged", "checked"):
            evidence["checked_modules"] = inputs["source_files"] + 2
        if name == "staged":
            evidence["route"] = "declarations -> signatures -> snapshot -> check_program"
        if name in ("warm", "selected_warm"):
            evidence["unchanged_source"] = True
        if name == "warm":
            evidence["user_file_results_reused"] = True
        if name in ("body_edit", "signature_edit", "selected_signature_edit", "lifecycle"):
            evidence["edit"] = {"changed_source": "shared.kgr" if dependency else "profile.kgr",
                                "effective_edit": True, "edit_kind": "signature" if signature else "body",
                                "edit": "value return i32 -> i64" if signature else
                                        ("value literal 1 -> 9" if dependency else "first x + 0 -> x + 9"),
                                "old_type": "i32", "new_type": "i64" if signature else "i32", "old_snapshot_valid": True}
        if name == "lifecycle":
            evidence.update(lifetime="old and new retained, then released", source_setup="sources retained",
                            measurement="sum of measured blocks", residual="sources and catalog can remain")
        if case["mode"] == "memory":
            evidence.update(allocation_accounting="cumulative successful requests and frees, not RSS",
                            timing_is_speed_evidence=False)
            points = []
            # Live falls below baseline and the final residual is nonzero, both legal.
            for checkpoint, requested, freed, peak in zip(results.CHECKPOINTS,
                                                         (100, 160, 220, 230, 240, 250),
                                                         (20, 30, 40, 130, 180, 200),
                                                         (80, 130, 180, 180, 180, 180)):
                points.append({"name": checkpoint, "requests": requested // 10, "requested_bytes": requested,
                               "frees": freed // 10, "freed_bytes": freed, "live_bytes": requested - freed,
                               "peak_bytes": peak})
            rows.append({"name": name, "evidence": evidence, "checkpoints": points})
        else:
            metrics = {metric: 10.0 for metric in results.METRICS[name]}
            if name == "lifecycle":
                metrics["measured_sum_ns"] = 90.0
            rows.append({"name": name, "evidence": evidence, "metrics_ns": metrics})
    return {**{k: v for k, v in inputs.items() if k != "scenario_names"},
            "mode": case["mode"], "check_only": check, "warmups": int(case["mode"] == "timing" and not check),
            "provider_setup_ns": 10.0, "scenarios": rows}


def campaign(paired=True, check=False):
    cases = [{"mode": "timing", "workload": "plain", "size": 96},
             {"mode": "timing", "workload": "dependency_edit", "size": 96},
             {"mode": "memory", "workload": "plain", "size": 96}]
    metadata = {key: "fixed-test-configuration" for key in results.MATCH_METADATA}
    metadata.update(cases=cases, runs=1 if check else 2, check_only=check, sample_protocol=runner.PROTOCOL)
    manifests = {results.case_key(case): manifest(case) for case in cases}
    run = {"metadata": metadata, "manifests": manifests,
           "images": {image: {} for image in (["candidate", "baseline"] if paired else ["candidate"])}, "samples": []}
    if paired:
        run["baseline_metadata"] = deepcopy(metadata)
    for index in range(metadata["runs"]):
        for case in cases:
            key = results.case_key(case)
            order = (["baseline", "candidate"] if index % 2 == 0 else ["candidate", "baseline"]) if paired else ["candidate"]
            run["samples"].append({"case": key, "index": index, "order": order, "manifest": manifests[key],
                                   "reports": {image: report(case, manifests[key], check) for image in order}})
    return run


class HirBenchmarkTests(unittest.TestCase):
    def test_manifest_and_configuration_compatibility(self):
        run = campaign()
        results.aggregate(run)
        for key in results.MATCH_METADATA:
            with self.subTest(metadata=key):
                changed = deepcopy(run["metadata"])
                changed[key] = None
                with self.assertRaisesRegex(ValueError, "metadata"):
                    results.validate_metadata(run["metadata"], changed)
        for mutate in (
            lambda r: r["manifests"]["memory/plain/96"].update(input_hash="b" * 64),
            lambda r: r["manifests"]["timing/plain/96"]["scenario_names"].pop(),
            lambda r: r["samples"][0]["reports"]["candidate"].update(input_hash="b" * 64),
        ):
            changed = deepcopy(run)
            mutate(changed)
            with self.assertRaises(ValueError):
                results.aggregate(changed)

    def test_timing_metrics_and_semantics_not_optimization_counters(self):
        run = campaign()
        for invalid in (None, -1, float("nan"), float("inf"), True):
            with self.subTest(metric=invalid):
                changed = deepcopy(run)
                metrics = changed["samples"][0]["reports"]["candidate"]["scenarios"][0]["metrics_ns"]
                if invalid is None:
                    del metrics["install"]
                else:
                    metrics["install"] = invalid
                with self.assertRaises(ValueError):
                    results.aggregate(changed)
        changed = deepcopy(run)
        changed["samples"][0]["reports"]["candidate"]["scenarios"].pop()
        with self.assertRaisesRegex(ValueError, "scenarios"):
            results.aggregate(changed)
        changed = deepcopy(run)
        selected = next(row for row in changed["samples"][1]["reports"]["candidate"]["scenarios"] if row["name"] == "selected_warm")
        selected["evidence"]["whole_result_reused"] = False
        selected["evidence"]["construction_stats"]["first"]["checked_bodies"] = 42
        results.aggregate(changed)  # Observation changes alone are not semantic regressions.
        for field, value in (("new_type", "i32"), ("changed_source", "consumer_a.kgr"), ("old_snapshot_valid", False)):
            changed = deepcopy(run)
            row = next(row for row in changed["samples"][1]["reports"]["candidate"]["scenarios"] if row["name"] == "signature_edit")
            row["evidence"]["edit"][field] = value
            with self.assertRaisesRegex(ValueError, "evidence"):
                results.aggregate(changed)
        changed = deepcopy(run)
        changed["samples"][0]["reports"]["candidate"]["scenarios"][0]["evidence"]["summary"]["provider_functions"] += 1
        with self.assertRaisesRegex(ValueError, "evidence"):
            results.aggregate(changed)

    def test_pairs_checks_and_saved_only_ratios(self):
        run = campaign()
        for mutate in (lambda r: r["samples"].pop(),
                       lambda r: r["samples"][0]["reports"].pop("baseline"),
                       lambda r: r["samples"][3].update(order=["baseline", "candidate"])):
            changed = deepcopy(run)
            mutate(changed)
            with self.assertRaises(ValueError):
                results.aggregate(changed)
        check = campaign(paired=False, check=True)
        results.validate_run(check, performance=False)
        with self.assertRaisesRegex(ValueError, "correctness"):
            results.compare_saved(check, run)
        changed = deepcopy(run)
        changed["samples"][0]["reports"]["baseline"]["scenarios"][0]["metrics_ns"]["install"] = 0.0
        summary = results.aggregate(changed)
        row = next(row for row in summary["rows"] if row["case"] == "timing/plain/96" and row["metric"] == "staged/install_ns")
        self.assertIsNone(row["paired_candidate_over_baseline"])
        self.assertTrue(summary["warnings"])
        run["samples"][0]["reports"]["baseline"]["scenarios"][0]["metrics_ns"]["install"] = 1.0
        paired = results.aggregate(run)
        row = next(row for row in paired["rows"] if row["case"] == "timing/plain/96" and row["metric"] == "staged/install_ns")
        self.assertEqual(row["paired_candidate_over_baseline"]["median"], 5.5)  # median(10/1, 10/10)
        self.assertEqual(row["candidate"], {"samples": [10, 10], "median": 10, "mad": 0, "min": 10, "max": 10})
        before = deepcopy(run)
        before["samples"][0]["reports"]["candidate"]["scenarios"][0]["metrics_ns"]["install"] = 1.0
        historical = results.compare_saved(before, run)
        self.assertIn("unpaired", historical["comparison"])
        self.assertTrue(all("paired_candidate_over_baseline" not in row for row in historical["rows"]))
        row = next(row for row in historical["rows"] if row["case"] == "timing/plain/96" and row["metric"] == "staged/install_ns")
        self.assertAlmostEqual(row["unpaired_median_candidate_over_baseline"], 10 / 5.5)

    def test_memory_cumulative_deallocation_and_retained_checkpoints(self):
        run = campaign()
        summary = results.aggregate(run)
        memory = [row for row in summary["rows"] if row["case"].startswith("memory/")]
        self.assertTrue(all("paired_candidate_over_baseline" not in row for row in memory))
        delta = next(row for row in memory if row["metric"] == "database_released/baseline_delta/live_bytes")
        self.assertEqual(delta["candidate"]["median"], -30)
        self.assertEqual(len(memory), len(results.CHECKPOINTS) * len(results.COUNTS) * 2)
        for mutate in (
            lambda p: p.pop(2),
            lambda p: p[3].update(live_bytes=999),
            lambda p: p[2].update(peak_bytes=1),
            lambda p: p[3].update(requests=0),
            lambda p: p[3].update(frees=0),
            lambda p: p[3].update(freed_bytes=-1),
            lambda p: p[3].pop("requested_bytes"),
            lambda p: p[0].update(peak_bytes=81),
        ):
            changed = deepcopy(run)
            points = changed["samples"][2]["reports"]["candidate"]["scenarios"][0]["checkpoints"]
            mutate(points)
            with self.assertRaises(ValueError):
                results.aggregate(changed)

    def test_cli_selection_and_frozen_executable_integrity(self):
        args = runner.parser().parse_args([])
        cases = runner.selection(args)
        self.assertEqual(sum(c["mode"] == "timing" for c in cases), 7)
        self.assertEqual(sum(c["mode"] == "memory" for c in cases), 3)
        cases = runner.selection(runner.parser().parse_args(["--check"]))
        self.assertEqual(len(cases), 6)
        self.assertTrue(all(c["size"] == 96 for c in cases))
        explicit = runner.selection(runner.parser().parse_args(["--workload", "plain", "--workload", "generic", "--size", "24"]))
        self.assertEqual(len(explicit), 4)
        with self.assertRaises(ValueError):
            runner.selection(runner.parser().parse_args(["--size", "24"]))
        for invalid in ("NaN", "inf", "0", "-1"):
            with self.assertRaises(runner.argparse.ArgumentTypeError):
                runner.positive_seconds(invalid)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            messages = [{"reason": "compiler-artifact", "package_id": "kagari-hir",
                         "target": {"name": name, "kind": ["bench"]}, "features": [],
                         "profile": {"opt_level": "3"}, "executable": str(root / f"{name}.exe")}
                        for name in runner.BENCHES.values()]
            messages.append({"reason": "compiler-artifact", "package_id": "dependency",
                             "target": {"name": "dependency", "kind": ["lib"]}, "features": ["enabled"],
                             "profile": {"opt_level": "3"}, "executable": None})
            output = "\n".join(runner.json.dumps(message) for message in messages)
            with patch.object(runner, "command", return_value=runner.subprocess.CompletedProcess([], 0, output, "")) as command:
                artifacts, build_info = runner.build(1, root)
                self.assertEqual(set(artifacts), {"timing", "memory"})
                self.assertIn("--locked", command.call_args.args[0])
                self.assertIn("--release", command.call_args.args[0])
                self.assertEqual(command.call_args.args[0].count("--bench"), 2)
                self.assertNotIn("--example", command.call_args.args[0])
                self.assertEqual(len(build_info["resolved_configuration"]), 3)
                dependency = next(c for c in build_info["resolved_configuration"] if c["package"] == "dependency")
                self.assertEqual(dependency["features"], ["enabled"])
            source = root / "hir_baseline.exe"
            source.write_bytes(b"frozen image")
            image = runner.archive(source, root, "candidate", "timing")
            archived = runner.image_path(root, image)
            self.assertNotEqual(archived, source)
            source.write_bytes(b"new build")
            self.assertEqual(archived.read_bytes(), b"frozen image")
            with patch.object(runner, "command", return_value=runner.subprocess.CompletedProcess([], 0, '{}', '')) as command:
                runner.invoke(root, image, {"workload": "plain", "size": 96}, 1, check=True)
                self.assertEqual(command.call_args.args[0][0], str(archived))
            archived.write_bytes(b"tampered image")
            with self.assertRaisesRegex(ValueError, "hash mismatch"):
                runner.image_path(root, image)
        for invalid in ('{"duration": NaN}', '{"duration": 1, "duration": 2}'):
            with self.assertRaises(ValueError):
                runner.decode(invalid)

    def test_frozen_archive_roundtrip_and_raw_consistency(self):
        run = campaign(paired=False)
        files = {"Cargo.lock": "a" * 64,
                 "crates/kagari-hir/benches/hir_baseline/timing.rs": "b" * 64,
                 "scripts/benchmark_hir.py": "c" * 64}
        state = {"files": files, "source_sha256": runner.object_sha(files), "lock_sha256": files["Cargo.lock"],
                 "harness_sha256": runner.object_sha({k: v for k, v in files.items() if k.startswith("crates/")}),
                 "tooling_sha256": runner.object_sha({k: v for k, v in files.items() if k.startswith("scripts/")})}
        run["metadata"].update({k: v for k, v in state.items() if k != "files"})
        run.update(source_before=state, source_after=state, complete=True)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for mode, name in runner.BENCHES.items():
                source = root / f"{name}.exe"
                source.write_bytes(name.encode())
                run["images"]["candidate"][mode] = runner.archive(source, root, "candidate", mode)
            for filename, value in (("run.json", run), ("metadata.json", run["metadata"]),
                                    ("images.json", run["images"]), ("manifests.json", run["manifests"]),
                                    ("source-before.json", state), ("source-after.json", state)):
                runner.save(root / filename, value)
            for index, sample in enumerate(run["samples"]):
                runner.save(root / "samples" / f"{index:04d}.json", sample)
                runner.save(root / "raw" / f"{index:04d}-candidate.json", sample["reports"]["candidate"])
            self.assertEqual(runner.load_run(root), run)
            (root / "raw" / "0000-candidate.json").write_text("{}", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "raw report/archive mismatch"):
                runner.load_run(root)

    def test_dirty_content_fingerprint_not_status_names(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            harness = root / "crates/kagari-hir/benches/hir_baseline"
            harness.mkdir(parents=True)
            names = ["Cargo.lock", "src.rs"] + [f"crates/kagari-hir/benches/hir_baseline/{name}.rs"
                                                  for name in ("main", "memory", "report", "scenarios", "timing", "workloads", "allocation")]
            for name in names:
                (root / name).write_text("original", encoding="utf-8")

            def command(args, timeout):
                if "ls-files" in args:
                    return runner.subprocess.CompletedProcess(args, 0, "\0".join(names), "")
                return runner.subprocess.CompletedProcess(args, 0, "unchanged git metadata", "")

            with patch.object(runner, "ROOT", root), patch.object(runner, "command", side_effect=command):
                before = runner.source_state(1)
                (root / "src.rs").write_text("optimization", encoding="utf-8")
                after = runner.source_state(1)
                self.assertNotEqual(before["source_sha256"], after["source_sha256"])
                self.assertEqual(before["harness_sha256"], after["harness_sha256"])
                (harness / "timing.rs").write_text("changed measured operation", encoding="utf-8")
                self.assertNotEqual(after["harness_sha256"], runner.source_state(1)["harness_sha256"])


if __name__ == "__main__":
    unittest.main()
