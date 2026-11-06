"""Runner evidence, archival and failure behavior without Cargo or benchmarks."""
import argparse
from contextlib import redirect_stdout
import copy
import hashlib
import io
from itertools import count
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import call, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import benchmark_gc as runner
from gc_benchmark_results import EvidenceError


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def metric(value=10, name="ns_per_op", observations=11):
    return dict(case="leaf", metric=name, unit="ns", direction="lower",
                value=value, observations=observations)


def metadata(suites, runs=2):
    # Include every matched field, including recipe fields added by the runner.
    result = {key: f"fixture-{key}" for key in runner.MATCHED}
    result.update(
        suites=list(suites), runs=runs, revision="fixture-revision", git_status="",
        machine={"host": "fixture-host", "logical_cpus": 4},
        rustc="fixture rustc", cargo="fixture cargo", environment={},
        measurement={"profile": "workspace release", "estimator": "process medians"},
        gc_source_sha256="a" * 64, tracked_diff_sha256="b" * 64,
        fixture_sha256="c" * 64, analysis_sha256="d" * 64, lock_sha256="e" * 64,
        build_config_sha256="f" * 64, cargo_config_sha256="0" * 64,
    )
    return result


def write_json(path, value):
    path.write_text(json.dumps(value), encoding="utf-8")


def executable_name(example):
    return example + (".exe" if runner.os.name == "nt" else "")


class RunnerTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name) / "project"
        self.root.mkdir()
        self.output = self.root / "result"
        self.enterContext(patch.object(runner, "ROOT", self.root))
        self.enterContext(redirect_stdout(io.StringIO()))
        self.enterContext(patch.object(runner.time, "perf_counter", side_effect=count()))
        self.parse = self.enterContext(patch.object(runner, "parse_run", return_value=[metric()]))
        self.launch = self.enterContext(patch.object(runner.subprocess, "run", side_effect=self.fake_launch))
        # Unexpected metadata commands must not invoke Git, rustc or PowerShell.
        self.enterContext(patch.object(runner.subprocess, "check_output",
                                       side_effect=AssertionError("unexpected external command")))
        self.collect_metadata = runner.metadata
        self.meta = self.enterContext(patch.object(runner, "metadata", side_effect=metadata))

    def args(self, **overrides):
        values = dict(output=self.output, baseline=None, suite=["access-borrow", "collection"],
                      runs=2, timeout=3.5, build_timeout=7.5)
        values.update(overrides)
        return argparse.Namespace(**values)

    def fake_launch(self, argv, **kwargs):
        if argv[0] == "cargo":
            products = self.root / "compiled"
            products.mkdir(exist_ok=True)
            for index, argument in enumerate(argv):
                if argument != "--example":
                    continue
                name = argv[index + 1]
                binary = products / executable_name(name)
                binary.write_bytes(f"binary:{name}".encode())
                item = dict(reason="compiler-artifact", target=dict(name=name, kind=["example"]),
                            executable=str(binary))
                kwargs["stdout"].write(json.dumps(item) + "\n")
        else:
            # Parsing belongs to gc_benchmark_results; these tests check delegation.
            kwargs["stdout"].write("fixture CSV stdout\n")
        kwargs["stderr"].write("fixture stderr\n")
        return subprocess.CompletedProcess(argv, 0)

    def baseline(self, suites=("access-borrow", "collection"), runs=2):
        directory = self.root / "baseline"
        directory.mkdir()
        (directory / "bin").mkdir()
        (directory / "raw").mkdir()
        previous = metadata(suites, runs)
        previous["binaries"] = {}
        for example in {runner.SUITES[suite][0] for suite in suites}:
            binary = directory / "bin" / executable_name(example)
            binary.write_bytes(f"archived:{example}".encode())
            previous["binaries"][example] = dict(file=f"bin/{binary.name}", sha256=sha256(binary.read_bytes()))
        processes = []
        for suite in suites:
            for index in range(runs):
                text = f"fixture CSV {suite} {index}\n"
                path = directory / "raw" / f"{suite}-candidate-{index}.csv"
                path.write_text(text, encoding="utf-8", newline="")
                processes.append(dict(suite=suite, variant="candidate", process=index,
                                      raw_sha256=sha256(path.read_bytes()), metrics=[metric()]))
        report = dict(schema=runner.SCHEMA, status="complete", metadata=previous,
                      processes=processes, summary=[])
        write_json(directory / "report.json", report)
        return directory, report, metadata(suites, runs)

    def assert_failed(self, output, error_type):
        state = json.loads((output / "state.json").read_text(encoding="utf-8"))
        self.assertEqual(state["status"], "failed")
        self.assertIn(error_type.__name__, state["error"])
        self.assertNotIn("summary", state)
        for name in ("report.json", "report.md", "summary.csv"):
            self.assertFalse((output / name).exists(), name)
        return state

    def cargo_config_environment(self):
        sandbox = self.root.parent
        home = sandbox / "cargo-home"
        home.mkdir()
        self.enterContext(patch.dict(runner.os.environ, {"CARGO_HOME": str(home)}, clear=True))
        self.enterContext(patch.object(Path, "home", return_value=sandbox / "user-home"))
        is_file = Path.is_file
        # Do not read the developer's real config files above the temp directory.
        self.enterContext(patch.object(Path, "is_file", autospec=True,
                                       side_effect=lambda path: path.is_relative_to(sandbox) and is_file(path)))
        return home

    def test_config_digest_tracks_root_ancestor_and_cargo_home_config_files(self):
        home = self.cargo_config_environment()
        empty = runner.cargo_config_digest()
        self.assertEqual(empty, sha256(b""))
        paths = []
        for directory in (self.root / ".cargo", self.root.parent / ".cargo", home):
            directory.mkdir(exist_ok=True)
            for name in ("config", "config.toml"):
                path = directory / name
                before = runner.cargo_config_digest()
                path.write_bytes(b"[build]\njobs = 2\n")
                with self.subTest(added=path):
                    self.assertNotEqual(runner.cargo_config_digest(), before)
                paths.append(path)
        original = runner.cargo_config_digest()
        self.assertRegex(original, r"^[0-9a-f]{64}$")
        for path in paths:
            contents = path.read_bytes()
            path.write_bytes(contents + b"# changed config\n")
            with self.subTest(mutated=path):
                self.assertNotEqual(runner.cargo_config_digest(), original)
                path.write_bytes(contents)
                self.assertEqual(runner.cargo_config_digest(), original)
                path.unlink()
                self.assertNotEqual(runner.cargo_config_digest(), original)
                path.write_bytes(contents)

    def test_config_digest_ignores_unrelated_files(self):
        home = self.cargo_config_environment()
        directories = (self.root / ".cargo", self.root.parent / ".cargo", home)
        for directory in directories:
            directory.mkdir(exist_ok=True)
            (directory / "config.toml").write_bytes(b"[build]\njobs = 2\n")
        original = runner.cargo_config_digest()
        unrelated = [self.root / "Cargo.toml"] + [directory / name for directory in directories
                     for name in ("credentials.toml", "config.toml.bak", "README.md")]
        for path in unrelated:
            with self.subTest(path=path):
                path.write_bytes(b"unrelated file\n")
                self.assertEqual(runner.cargo_config_digest(), original)
                path.write_bytes(b"modified unrelated file\n")
                self.assertEqual(runner.cargo_config_digest(), original)

    def test_config_digest_uses_default_cargo_home_when_unset(self):
        self.cargo_config_environment()
        directory = Path.home() / ".cargo"
        directory.mkdir(parents=True)
        with patch.dict(runner.os.environ, {}, clear=True):
            before = runner.cargo_config_digest()
            (directory / "config.toml").write_bytes(b"[build]\njobs = 2\n")
            self.assertNotEqual(runner.cargo_config_digest(), before)

    def test_relative_cargo_home_is_resolved_against_root_not_process_cwd(self):
        self.cargo_config_environment()
        home = self.root / "relative-cargo-home"
        home.mkdir()
        config = home / "config.toml"
        config.write_bytes(b"[build]\njobs = 2\n")
        with patch.dict(runner.os.environ, {"CARGO_HOME": str(home)}, clear=True):
            absolute = runner.cargo_config_digest()
        with patch.dict(runner.os.environ, {"CARGO_HOME": home.name}, clear=True):
            self.assertEqual(runner.cargo_config_digest(), absolute)
            config.write_bytes(b"[build]\njobs = 3\n")
            self.assertNotEqual(runner.cargo_config_digest(), absolute)

    def test_metadata_records_effective_build_env_without_leaking_registry_tokens(self):
        home = self.cargo_config_environment()
        (home / "config.toml").write_text('[registry]\ntoken = "config-secret"\n', encoding="utf-8")
        expected_env = {"CARGO_BUILD_RUSTFLAGS": "-C opt-level=2",
                        "CARGO_BUILD_RUSTC_WRAPPER": "fixture-wrapper",
                        "CARGO_BUILD_JOBS": "3", "CARGO_HOME": str(home), "CARGO_INCREMENTAL": "0"}
        environment = {**expected_env, "CARGO_REGISTRY_TOKEN": "registry-secret",
                       "CARGO_REGISTRIES_CRATES_IO_TOKEN": "named-registry-secret", "UNRELATED": "ignored"}
        with patch.dict(runner.os.environ, environment, clear=True), \
                patch.object(runner, "command", return_value="fixture command"), \
                patch.object(runner.subprocess, "check_output", return_value=b"fixture diff"), \
                patch.object(runner, "machine", return_value={"host": "fixture-host"}), \
                patch.object(runner, "tree_digest", return_value="1" * 64), \
                patch.object(runner, "digest", return_value="2" * 64):
            config_hash = runner.cargo_config_digest()
            result = self.collect_metadata(["access"], 2)
        self.assertEqual(result["environment"], expected_env)
        self.assertEqual(result["cargo_config_sha256"], config_hash)
        self.assertIn("cargo_config_sha256", runner.MATCHED)
        for secret in ("registry-secret", "named-registry-secret", "config-secret"):
            self.assertNotIn(secret, json.dumps(result))
        self.meta.assert_not_called()
        self.launch.assert_not_called()

    def test_good_baseline_reparses_each_candidate_and_checks_archived_binaries(self):
        directory, report, current = self.baseline()
        result = runner.load_baseline(directory, current)
        self.assertEqual(result, report)
        self.parse.assert_has_calls([
            call(record["suite"], f"fixture CSV {record['suite']} {record['process']}\n", record["process"])
            for record in report["processes"]
        ])
        self.assertEqual(self.parse.call_count, 4)
        self.launch.assert_not_called()

    def test_baseline_rejects_each_changed_or_missing_matched_metadata_field(self):
        directory, report, current = self.baseline()
        for key in runner.MATCHED:
            for missing in (False, True):
                modified = copy.deepcopy(report)
                if missing:
                    del modified["metadata"][key]
                else:
                    modified["metadata"][key] = {"changed": key}
                write_json(directory / "report.json", modified)
                with self.subTest(key=key, missing=missing), self.assertRaises(EvidenceError):
                    runner.load_baseline(directory, current)
        self.parse.assert_not_called()

    def test_baseline_rejects_incomplete_status_or_wrong_schema(self):
        directory, report, current = self.baseline()
        for field, value in (("status", "running"), ("status", "failed"),
                             ("status", None), ("schema", runner.SCHEMA + 1)):
            modified = copy.deepcopy(report)
            modified[field] = value
            write_json(directory / "report.json", modified)
            with self.subTest(field=field, value=value), self.assertRaises(EvidenceError):
                runner.load_baseline(directory, current)
        self.parse.assert_not_called()

    def test_baseline_requires_positive_integer_process_count(self):
        directory, report, current = self.baseline()
        for runs in (0, -1, True, "2", None):
            modified = copy.deepcopy(report)
            modified["metadata"]["runs"] = runs
            write_json(directory / "report.json", modified)
            with self.subTest(runs=runs), self.assertRaises(EvidenceError):
                runner.load_baseline(directory, current)
        self.parse.assert_not_called()

    def test_baseline_rejects_missing_whole_suite_missing_process_and_duplicate(self):
        directory, report, current = self.baseline()
        records = report["processes"]
        for label, processes in (
            ("whole suite", [r for r in records if r["suite"] != "collection"]),
            ("process", records[:-1]),
            ("duplicate", records + [records[0]]),
            ("no candidates", [dict(r, variant="baseline") for r in records]),
        ):
            modified = dict(report, processes=processes)
            write_json(directory / "report.json", modified)
            with self.subTest(label=label), self.assertRaises(EvidenceError):
                runner.load_baseline(directory, current)

    def test_baseline_rejects_unexpected_candidate_identity(self):
        directory, report, current = self.baseline()
        for field, value in (("process", 2), ("process", -1), ("suite", "capacity")):
            modified = copy.deepcopy(report)
            modified["processes"][0][field] = value
            write_json(directory / "report.json", modified)
            with self.subTest(field=field, value=value), self.assertRaises(EvidenceError):
                runner.load_baseline(directory, current)

    def test_baseline_rejects_corrupt_raw_before_parsing(self):
        directory, _, current = self.baseline()
        path = directory / "raw/access-borrow-candidate-0.csv"
        path.write_bytes(b"modified CSV\n")
        with self.assertRaisesRegex(EvidenceError, "CSV hash mismatch"):
            runner.load_baseline(directory, current)
        self.parse.assert_not_called()

    def test_baseline_rejects_invalid_csv_even_with_matching_digest(self):
        directory, report, current = self.baseline()
        path = directory / "raw/access-borrow-candidate-0.csv"
        path.write_bytes(b"truncated but correctly hashed CSV\n")
        report["processes"][0]["raw_sha256"] = sha256(path.read_bytes())
        write_json(directory / "report.json", report)
        self.parse.side_effect = EvidenceError("invalid fixture evidence")
        with self.assertRaisesRegex(EvidenceError, "invalid fixture evidence"):
            runner.load_baseline(directory, current)
        self.parse.assert_called_once_with("access-borrow", path.read_text(encoding="utf-8"), 0)

    def test_baseline_rejects_corrupt_archived_executable(self):
        directory, _, current = self.baseline()
        binary = directory / "bin" / executable_name("field_access")
        binary.write_bytes(b"modified executable")
        with self.assertRaisesRegex(EvidenceError, "executable hash mismatch"):
            runner.load_baseline(directory, current)
        self.launch.assert_not_called()

    def test_measure_routes_process_before_flags_only_for_indexed_examples(self):
        (self.output / "raw").mkdir(parents=True)
        for suite, (example, flags) in runner.SUITES.items():
            binary = self.root / "archive" / executable_name(example)
            result = runner.measure(self.output, binary, suite, "candidate", 7, 2.25)
            argv = [str(binary)]
            if example in ("field_access", "arena_comparison"):
                argv.append("7")
            argv.extend(flags)
            with self.subTest(suite=suite):
                launch = self.launch.call_args
                self.assertEqual(launch.args, (argv,))
                self.assertEqual(launch.kwargs["timeout"], 2.25)
                self.assertIs(launch.kwargs["check"], True)
                self.assertEqual(launch.kwargs["cwd"], self.root)
                self.assertEqual(result["command"], argv)
                self.assertEqual(result["metrics"], [metric()])
                self.assertEqual(result["process"], 7)
                raw = self.output / "raw" / f"{suite}-candidate-7.csv"
                self.assertEqual(result["raw_sha256"], sha256(raw.read_bytes()))
                self.parse.assert_called_with(suite, "fixture CSV stdout\n", 7)
                self.assertEqual(raw.with_suffix(".stderr.log").read_text(encoding="utf-8"), "fixture stderr\n")

    def test_duplicate_measure_preserves_original_raw_and_stderr(self):
        (self.output / "raw").mkdir(parents=True)
        binary = self.root / "archive" / executable_name("field_access")
        result = runner.measure(self.output, binary, "access-borrow", "candidate", 7, 2.25)
        raw = self.output / "raw/access-borrow-candidate-7.csv"
        stderr = raw.with_suffix(".stderr.log")
        original_raw, original_stderr = raw.read_bytes(), stderr.read_bytes()
        self.launch.reset_mock()
        self.parse.reset_mock()

        with self.assertRaises(FileExistsError):
            runner.measure(self.output, binary, "access-borrow", "candidate", 7, 2.25)

        self.assertEqual(raw.read_bytes(), original_raw)
        self.assertEqual(stderr.read_bytes(), original_stderr)
        self.assertEqual(sha256(raw.read_bytes()), result["raw_sha256"])
        self.assertEqual(set(raw.parent.iterdir()), {raw, stderr})
        self.launch.assert_not_called()
        self.parse.assert_not_called()

    def test_measure_preserves_existing_stderr_when_raw_is_absent(self):
        (self.output / "raw").mkdir(parents=True)
        raw = self.output / "raw/collection-candidate-0.csv"
        stderr = raw.with_suffix(".stderr.log")
        stderr.write_bytes(b"original stderr evidence\n")
        binary = self.root / "archive" / executable_name("collection_profile")

        with self.assertRaises(FileExistsError):
            runner.measure(self.output, binary, "collection", "candidate", 0, 2.25)

        self.assertEqual(stderr.read_bytes(), b"original stderr evidence\n")
        self.launch.assert_not_called()
        self.parse.assert_not_called()

    def test_build_deduplicates_examples_and_archives_cargo_artifacts(self):
        (self.output / "bin").mkdir(parents=True)
        result = runner.build(self.output, ["access", "access-borrow", "collection"], 12)
        argv = self.launch.call_args.args[0]
        examples = [argv[i + 1] for i, arg in enumerate(argv) if arg == "--example"]
        self.assertEqual(examples, ["collection_profile", "field_access"])
        self.assertEqual(argv[:3], ["cargo", "build", "--release"])
        self.assertIn("--locked", argv)
        self.assertEqual(self.launch.call_args.kwargs["timeout"], 12)
        self.assertTrue(self.launch.call_args.kwargs["check"])
        self.assertEqual(set(result["binaries"]), set(examples))
        for example, evidence in result["binaries"].items():
            saved = self.output / evidence["file"]
            self.assertEqual(saved.read_bytes(), f"binary:{example}".encode())
            self.assertEqual(evidence["sha256"], sha256(saved.read_bytes()))
        self.assertGreaterEqual(result["build_wall_seconds"], 0)
        self.parse.assert_not_called()

    def test_build_ignores_non_example_messages_and_requires_all_binaries(self):
        (self.output / "bin").mkdir(parents=True)

        def messages(argv, **kwargs):
            for item in (
                dict(reason="compiler-message"),
                dict(reason="compiler-artifact", executable=None),
                dict(reason="compiler-artifact", executable="not-a-file",
                     target=dict(name="field_access", kind=["lib"])),
                dict(reason="compiler-artifact", executable="not-a-file",
                     target=dict(name="unselected", kind=["example"])),
            ):
                kwargs["stdout"].write(json.dumps(item) + "\n")
            return subprocess.CompletedProcess(argv, 0)

        self.launch.side_effect = messages
        with self.assertRaisesRegex(EvidenceError, "every selected example"):
            runner.build(self.output, ["access"], 12)
        self.assertEqual(list((self.output / "bin").iterdir()), [])

    def test_build_or_measure_timeout_and_nonzero_leave_failed_state_not_reports(self):
        for stage in ("build", "measure"):
            for error_type in (subprocess.TimeoutExpired, subprocess.CalledProcessError):
                output = self.root / f"{stage}-{error_type.__name__}"

                def failing_launch(argv, **kwargs):
                    result = self.fake_launch(argv, **kwargs)
                    if (argv[0] == "cargo") == (stage == "build"):
                        if error_type is subprocess.TimeoutExpired:
                            raise error_type(argv, kwargs["timeout"])
                        raise error_type(9, argv)
                    return result

                self.launch.side_effect = failing_launch
                self.parse.reset_mock()
                with self.subTest(stage=stage, error=error_type.__name__):
                    with self.assertRaises(error_type):
                        runner.run(self.args(output=output))
                    state = self.assert_failed(output, error_type)
                    self.assertEqual(state["processes"], [])
                    self.parse.assert_not_called()
                    expected_timeout = 7.5 if stage == "build" else 3.5
                    self.assertEqual(self.launch.call_args.kwargs["timeout"], expected_timeout)
                    log = output / ("build.stderr.log" if stage == "build" else "raw/access-borrow-candidate-0.stderr.log")
                    self.assertEqual(log.read_text(encoding="utf-8"), "fixture stderr\n")

    def test_failed_later_measurement_preserves_completed_process_evidence(self):
        fixture_calls = 0

        def fail_second(argv, **kwargs):
            nonlocal fixture_calls
            result = self.fake_launch(argv, **kwargs)
            if argv[0] != "cargo":
                fixture_calls += 1
                if fixture_calls == 2:
                    raise subprocess.TimeoutExpired(argv, kwargs["timeout"])
            return result

        self.launch.side_effect = fail_second
        with self.assertRaises(subprocess.TimeoutExpired):
            runner.run(self.args())
        state = self.assert_failed(self.output, subprocess.TimeoutExpired)
        self.assertEqual(len(state["processes"]), 1)
        completed = state["processes"][0]
        self.assertEqual((completed["suite"], completed["process"]), ("access-borrow", 0))
        raw = self.output / "raw/access-borrow-candidate-0.csv"
        self.assertEqual(completed["raw_sha256"], sha256(raw.read_bytes()))
        self.assertTrue((self.output / "raw/collection-candidate-0.csv").exists())

    def test_invalid_measured_csv_cannot_publish_complete_report(self):
        self.parse.side_effect = EvidenceError("missing measured samples")
        with self.assertRaisesRegex(EvidenceError, "missing measured samples"):
            runner.run(self.args())
        self.assert_failed(self.output, EvidenceError)

    def test_run_rejects_source_recipe_or_environment_changes_after_measurement(self):
        fields = (*runner.MATCHED, "gc_source_sha256", "revision", "tracked_diff_sha256", "git_status")
        for key in fields:
            args = self.args(output=self.root / f"changed-{key}")
            initial = metadata(args.suite, args.runs)
            final = copy.deepcopy(initial)
            value = final[key]
            if isinstance(value, dict):
                final[key] = {**value, "changed_during_measurement": True}
            elif isinstance(value, list):
                final[key] = list(reversed(value))
            elif key.endswith("sha256"):
                final[key] = sha256(f"changed {key}".encode())
            else:
                final[key] = value + "-changed"
            self.meta.reset_mock()
            self.meta.side_effect = [initial, final]
            self.parse.reset_mock()

            with self.subTest(field=key):
                with self.assertRaisesRegex(EvidenceError, f"inputs/environment changed during measurement: {key}"):
                    runner.run(args)
                state = self.assert_failed(args.output, EvidenceError)
                self.assertEqual(len(state["processes"]), 4)
                self.assertEqual(self.parse.call_count, 4)
                self.assertEqual(self.meta.call_args_list, [call(args.suite, args.runs)] * 2)
                self.assertEqual(state["metadata"][key], initial[key])

    def test_successful_run_writes_complete_report_after_all_processes(self):
        output = runner.run(self.args())
        self.assertEqual(output, self.output)
        report = json.loads((output / "report.json").read_text(encoding="utf-8"))
        state = json.loads((output / "state.json").read_text(encoding="utf-8"))
        self.assertEqual(report["status"], "complete")
        self.assertEqual(state, report)
        self.assertEqual(len(report["processes"]), 4)
        self.assertEqual([(p["suite"], p["process"]) for p in report["processes"]],
                         [("access-borrow", 0), ("collection", 0),
                          ("collection", 1), ("access-borrow", 1)])
        self.assertTrue((output / "report.md").exists())
        self.assertTrue((output / "summary.csv").exists())
        self.assertTrue(all(row["processes"] == 2 for row in report["summary"]))
        self.assertEqual(self.parse.call_count, 4)

    def test_paired_run_uses_archived_binaries_and_alternates_variant_order(self):
        directory, _, _ = self.baseline()
        runner.run(self.args(baseline=directory))
        report = json.loads((self.output / "report.json").read_text(encoding="utf-8"))
        expected = []
        for index, suites in ((0, ("access-borrow", "collection")),
                              (1, ("collection", "access-borrow"))):
            variants = ("baseline", "candidate") if index == 0 else ("candidate", "baseline")
            expected.extend((suite, variant, index) for suite in suites for variant in variants)
        self.assertEqual([(p["suite"], p["variant"], p["process"]) for p in report["processes"]], expected)
        for record in report["processes"]:
            archive = directory if record["variant"] == "baseline" else self.output
            example = runner.SUITES[record["suite"]][0]
            self.assertEqual(record["command"][0], str(archive / "bin" / executable_name(example)))
        candidate = [r for r in report["summary"] if r["variant"] == "candidate"]
        self.assertTrue(all(r["paired_ratio"] == 1 for r in candidate))

    def test_existing_output_is_not_overwritten_or_used_for_a_new_run(self):
        self.output.mkdir()
        sentinel = self.output / "report.json"
        sentinel.write_bytes(b"original evidence; must survive")
        with self.assertRaises(FileExistsError):
            runner.run(self.args())
        self.assertEqual(sentinel.read_bytes(), b"original evidence; must survive")
        self.assertEqual(list(self.output.iterdir()), [sentinel])
        self.meta.assert_not_called()
        self.launch.assert_not_called()
        self.parse.assert_not_called()

    def test_render_unobserved_and_zero_denominators_as_unavailable(self):
        processes = []
        for variant, values in (("baseline", (None, 0, 10, 100)),
                                ("candidate", (None, 10, 0, 50))):
            metrics = [metric(value, name, 0 if value is None else 11)
                       for name, value in zip(("assist_max_ns", "step_max_ns", "cleanup_ns", "batch_p99_ns"), values)]
            processes.append(dict(suite="sustained", variant=variant, process=0, metrics=metrics))
        report = dict(metadata=metadata(["sustained"], 1), summary=runner.summarize(processes))
        rendered = runner.render(report)
        cells = [list(map(str.strip, line.split("|")[1:-1])) for line in rendered.splitlines()
                 if line.startswith("| leaf |")]
        by_metric = {row[1]: row for row in cells}
        self.assertEqual(by_metric["assist_max_ns"][4], "unobserved [unobserved, unobserved]")
        self.assertEqual(by_metric["assist_max_ns"][6:], ["n/a", "0"])
        self.assertEqual(by_metric["step_max_ns"][5:7], ["0", "n/a"])
        # A zero numerator is a real -100% change, not a zero denominator.
        self.assertEqual(by_metric["cleanup_ns"][6], "-100.00%")
        self.assertEqual(by_metric["batch_p99_ns"][6], "-50.00%")
        self.assertIn("not RSS", rendered)
        self.assertIn("not full teardown", rendered)
        self.assertIn("Exploratory run", rendered)
        self.assertNotIn("nan%", rendered.lower())
        self.assertNotIn("inf%", rendered.lower())


if __name__ == "__main__":
    unittest.main()
