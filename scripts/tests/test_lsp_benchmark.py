"""Offline archive/peer contracts; incorrect runs must never become speed evidence."""
from copy import deepcopy
from collections import deque
from io import BytesIO
from pathlib import Path
import sys
import queue
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import lsp_benchmark_inputs as inputs
import lsp_benchmark_protocol as protocol
import lsp_benchmark_results as results
import lsp_benchmark_diagnostics as diagnostics
import benchmark_lsp as driver
import lsp_benchmark_queries as queries


def report(paired=False, check=False, suite="protocol"):
    groups = 1 if check else 7
    images = {"candidate": {"path": "candidate.exe", "sha256": "a" * 64}}
    if paired:
        images["baseline"] = {"path": "baseline.exe", "sha256": "b" * 64}
    samples = []
    for group in range(groups):
        for image in images:
            duration = 10_000_000 if image == "baseline" else 16_000_000
            samples.append({"image": image, "group": group, "workload": "B01.protocol", "errors": 0,
                            "completed": 2, "spawn_to_initialize_ns": duration * 2,
                            "initialize_round_trip_ns": duration,
                            "evidence": {"positionEncoding": "utf-16", "openClose": True, "change": 1,
                                         "serverName": "kagari-analyzer", "serverVersion": "test",
                                         "shutdown_is_null": True, "exit_code": 0}})
    source = {"files": {}, "source_sha256": inputs.object_digest({})}
    metadata = {key: "test-only" for key in results.MATCH_METADATA}
    workload = {} if suite == "protocol" else {"sources": inputs.frozen("pilot"), "diagnostics": diagnostics.frozen_scenario()}
    metadata.update(workload_sha256=inputs.object_digest(workload), source=source, suite=suite)
    if suite == "diagnostics":
        for sample in samples:
            diagnostic_sample(sample)
    return {"schema": 1, "metadata": metadata, "inputs": workload, "source_before": source, "source_after": source,
            "groups": groups, "check_only": check, "images": images, "samples": samples,
            "unavailable_rows": results.unavailable(suite)}


def diagnostic_sample(sample):
    sample.update(workload="B01+B03.diagnostics", completed=8, operations=[], notification_history=[],
                  document_uris={path: "file:///benchmark/" + path for path in (diagnostics.APP, diagnostics.API, diagnostics.UNAFFECTED)})
    for number, step in enumerate(diagnostics.frozen_scenario()["steps"]):
        started = 100_000_000 * (number + 1)
        observations = {}
        for index, (path, expected) in enumerate(step["expected"].items()):
            received = started + 10_000_000 + index
            params = {"uri": sample["document_uris"][path], **deepcopy(expected)}
            observations[path] = {"received_ns": received, "params": params}
            sample["notification_history"].append({"received_ns": received,
                "frame": {"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": deepcopy(params)}})
        ended = observations[diagnostics.APP]["received_ns"] if step["name"] == "open" else max(item["received_ns"] for item in observations.values())
        duration = ended - started
        if step["metric"]:
            sample[step["metric"]] = duration
        sample["operations"].append({"name": step["name"], "started_ns": started, "completed_ns": ended,
                                    "duration_ns": duration, "changes": deepcopy(step["changes"]), "observations": observations})


class ArchiveContracts(unittest.TestCase):
    def test_missing_duplicate_or_failed_samples_are_rejected(self):
        original = report()
        results.validate_report(original)
        for kind in ("missing", "duplicate", "error", "wrong_encoding", "zero", "wrong_clock", "changed_inputs", "changed_source"):
            broken = deepcopy(original)
            if kind == "missing":
                broken["samples"].pop()
            elif kind == "duplicate":
                broken["samples"].append(deepcopy(broken["samples"][0]))
            elif kind == "error":
                broken["samples"][0]["errors"] = 1
            elif kind == "wrong_encoding":
                broken["samples"][0]["evidence"]["positionEncoding"] = "utf-8"
            elif kind == "zero":
                broken["samples"][0]["spawn_to_initialize_ns"] = 0
            elif kind == "wrong_clock":
                broken["samples"][0]["spawn_to_initialize_ns"] = 1
            elif kind == "changed_inputs":
                broken["inputs"] = {"changed": True}
            else:
                broken["source_after"] = {"changed": True}
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                results.validate_report(broken)

    def test_correctness_only_is_not_timing_evidence_and_pairs_use_groups(self):
        summary = results.summarize(report(check=True))
        self.assertEqual(summary["rows"]["spawn_to_initialize"]["status"], "correctness-only")
        summary = results.summarize(report(paired=True))
        row = summary["rows"]["initialize_round_trip"]
        self.assertEqual(row["median_paired_change"], 6.0)
        self.assertEqual(row["median_paired_change_percent"], 60.0)
        self.assertEqual(row["confidence_interval_95"], [6.0, 6.0])
        self.assertEqual(row["status"], "regression")
        self.assertIn("no p95", row["tails"])
        original = report(paired=True)
        for sample in original["samples"]:
            if sample["image"] == "candidate":
                sample["initialize_round_trip_ns"] = 10_000_000
        self.assertEqual(results.summarize(original)["rows"]["initialize_round_trip"]["status"], "inconclusive")

    def test_changed_inputs_environment_or_lock_refuse_comparison(self):
        metadata = report()["metadata"]
        results.validate_compatible(metadata, deepcopy(metadata))
        for key in ("workload_sha256", "lock_sha256", "rustc", "transport", "power_mode", "suite", "cache_state"):
            changed = {**metadata, key: "different"}
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, key):
                results.validate_compatible(metadata, changed)

    def test_diagnostics_require_exact_versioned_witnesses_and_complete_history(self):
        original = report(check=True, suite="diagnostics")
        results.validate_report(original)
        summary = results.summarize(original)
        self.assertEqual(summary["rows"]["open_to_first_diagnostics"]["status"], "correctness-only")
        self.assertIn("B03.queries", summary["unavailable_rows"])
        for kind in ("missing", "empty_startup", "empty_repair", "wrong_version", "wrong_range", "wrong_message", "missing_history", "unaffected", "clock"):
            broken = deepcopy(original)
            sample = broken["samples"][0]
            observation = sample["operations"][0]["observations"][diagnostics.APP]
            if kind == "missing":
                sample["operations"].pop()
            elif kind == "empty_startup":
                observation["params"]["diagnostics"] = []
            elif kind == "empty_repair":
                sample["operations"][3]["observations"][diagnostics.APP]["params"]["diagnostics"] = []
            elif kind == "wrong_version":
                observation["params"]["version"] = 2
            elif kind == "wrong_range":
                observation["params"]["diagnostics"][0]["range"]["start"]["character"] = 1
            elif kind == "wrong_message":
                observation["params"]["diagnostics"][0]["message"] = "another error"
            elif kind == "missing_history":
                sample["notification_history"].pop(0)
            elif kind == "unaffected":
                sample["notification_history"][2]["frame"]["params"]["diagnostics"] = [deepcopy(diagnostics.ERROR)]
            else:
                sample["signature_error_ns"] += 1
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                results.validate_report(broken)


class PeerContracts(unittest.TestCase):
    def test_notification_waits_reject_empty_readiness_and_missing_versions(self):
        peer = protocol.Peer.__new__(protocol.Peer)
        peer.frames = queue.Queue(64)
        peer.history = deque(maxlen=32)
        peer.notifications = deque(maxlen=128)
        peer.notification_history = []
        peer.history_limit = 512
        peer.notification_bytes = 0
        uri = "file:///benchmark/src/lib.kgr"
        expected = {"version": 1, "diagnostics": [deepcopy(diagnostics.ERROR)]}
        for received, params in [(10, {"version": 0, "diagnostics": [deepcopy(diagnostics.ERROR)]}),
                                 (20, {"version": 1, "diagnostics": []}), (30, expected)]:
            peer.frames.put(({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {"uri": uri, **params}}, received))
        value, received = peer.wait_notification("textDocument/publishDiagnostics", lambda params: diagnostics.matches(params, uri, expected))
        self.assertEqual(received, 30)
        self.assertEqual(value["diagnostics"], [diagnostics.ERROR])
        self.assertEqual(len(peer.notification_history), 3)
        repaired = {"uri": uri, "version": 2, "diagnostics": [deepcopy(diagnostics.SENTINEL)]}
        for received in (40, 50):
            peer.frames.put(({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": repaired}, received))
        _, received = peer.wait_notification("textDocument/publishDiagnostics",
            lambda params: diagnostics.matches(params, uri, repaired), after_ns=45)
        self.assertEqual(received, 50, "same-version historical witnesses cannot satisfy a new edit")
        peer.frames.put((None, 60))
        with self.assertRaisesRegex(ValueError, "EOF before expected"):
            peer.wait_notification("textDocument/publishDiagnostics", lambda params: diagnostics.matches(params, uri, {"version": 3, "diagnostics": []}))
        with self.assertRaisesRegex(ValueError, "protocol timeout"):
            peer.wait_notification("missing", lambda _: True, timeout=0)
        peer.notification_bytes = protocol.MAX_NOTIFICATION_HISTORY_BYTES
        with self.assertRaisesRegex(ValueError, "history byte budget"):
            peer._dispatch({"jsonrpc": "2.0", "method": "window/logMessage", "params": {}}, 50)

    def test_literal_frames_keep_unicode_lengths_and_coalesced_tails(self):
        reader = BytesIO(b'Content-Length: 40\r\n\r\n{"jsonrpc":"2.0","id":1,"result":"\xf0\x9f\x98\x80"}'
                         b'Content-Length: 38\r\n\r\n{"jsonrpc":"2.0","id":2,"result":null}')
        self.assertEqual(protocol.read_frame(reader)["result"], "😀")
        self.assertIsNone(protocol.read_frame(reader)["result"])
        self.assertIsNone(protocol.read_frame(reader))

    def test_protocol_errors_cannot_be_silently_skipped(self):
        for data in (b"log line\n", b"Content-Length: 1\r\nContent-Length: 1\r\n\r\n{",
                     b"Content-Length: 3\r\n\r\n{", b"Content-Length: 9000000\r\n\r\n"):
            with self.subTest(data=data), self.assertRaises((ValueError, UnicodeDecodeError)):
                protocol.read_frame(BytesIO(data))


class WorkloadContracts(unittest.TestCase):
    def test_archived_source_and_edit_bytes_must_match_the_frozen_manifest(self):
        target = inputs.ROOT / "target"
        target.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="lsp-benchmark-test-", dir=target) as directory:
            root = Path(directory)
            manifest = {"sources": inputs.materialize("pilot", root / "inputs"), "diagnostics": diagnostics.frozen_scenario()}
            driver.save(root / "edit-inputs.json", diagnostics.variants())
            driver.verify_inputs(root, manifest, "diagnostics")
            source = root / "inputs" / diagnostics.APP
            original = source.read_bytes()
            source.write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "source input hash mismatch"):
                driver.verify_inputs(root, manifest, "diagnostics")
            source.write_bytes(original)
            (root / "edit-inputs.json").write_text("{}", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "edit inputs changed"):
                driver.verify_inputs(root, manifest, "diagnostics")

    def test_frozen_shapes_have_semantic_scale_and_bounded_churn(self):
        pilot = inputs.frozen("pilot")
        workspace = inputs.frozen("workspace")
        churn = inputs.frozen("churn")
        self.assertEqual((pilot["packages"], pilot["source_files"]), (2, 20))
        self.assertEqual((workspace["packages"], workspace["source_files"]), (4, 200))
        self.assertGreater(pilot["generated_function_bodies"], 1900)
        self.assertGreater(workspace["generated_call_references"], 19000)
        self.assertEqual(len(churn["schedule"]["churn_edits"]), 1000)
        self.assertEqual(churn["file_sha256"], pilot["file_sha256"])
        self.assertEqual(churn["schedule"]["retention_checkpoints"], [100, 500, 1000])
        self.assertIn(b"\r\n", inputs.source_files("pilot")["src/scale0.kgr"])
        scenario = diagnostics.frozen_scenario()
        variants = diagnostics.variants()
        self.assertEqual(len(scenario["steps"]), 6)
        for step in scenario["steps"]:
            for change in step["changes"]:
                self.assertEqual(change["sha256"], inputs.digest(variants[change["variant"]].encode()))
        self.assertIn("    true\n}", variants["body_error"])
        self.assertIn("pub fn default_value() -> bool", variants["signature_error"])


class QueryEvidenceContracts(unittest.TestCase):
    def test_query_oracle_rejects_wrong_type_and_shifted_destination(self):
        root = inputs.ROOT / "target" / "query-oracle"
        queries.frozen_scenario()
        answer = {"contents":[{"language":"kagari","value":"value: i32"}]}
        queries.assert_answer("hover", answer, root)
        with self.assertRaisesRegex(ValueError, "checked type"):
            queries.assert_answer("hover", {"contents":[{"value":"value: bool"}]}, root)
        start = queries.position(diagnostics.variants()["api"], "fn default_value", len("fn "))
        destination = {"uri": (root / diagnostics.API).as_uri(), "range": {
            "start":start, "end":{**start, "character":start["character"] + len("default_value")}}}
        queries.assert_answer("definition", destination, root)
        destination["range"]["end"]["character"] += 1
        with self.assertRaisesRegex(ValueError, "authoritative API name range"):
            queries.assert_answer("definition", destination, root)
        with self.assertRaisesRegex(ValueError, "complete frozen coverage"):
            queries.assert_answer("implementation", [], root)

    def test_diagnostic_tail_schedule_keeps_versions_and_witnesses_monotonic(self):
        steps = diagnostics.frozen_tail_scenario()["steps"]
        self.assertEqual(len(steps), 2 + 4 * diagnostics.TAIL_ROUNDS)
        versions = {}
        for step in steps:
            for change in step["changes"]:
                self.assertGreater(change["version"], versions.get(change["file"], 0))
                versions[change["file"]] = change["version"]
            for expected in step["expected"].values():
                self.assertGreaterEqual(expected["version"], 1)
            self.assertIn(diagnostics.SENTINEL, step["expected"][diagnostics.APP]["diagnostics"])
        raw = [{"name": step["name"], "duration_ns": 100 + index} for index, step in enumerate(steps)]
        metrics = diagnostics.tail_metrics(raw)
        broken = deepcopy(raw)
        del broken[-1]
        with self.assertRaisesRegex(ValueError, "observation count"):
            diagnostics.tail_metrics(broken)
        self.assertGreater(metrics["signature_repair_p95_ns"], metrics["signature_repair_p50_ns"])

    def test_query_percentiles_use_retained_within_process_samples(self):
        samples = list(range(1, 201))
        self.assertEqual(queries.percentile(samples, .50), 100)
        self.assertEqual(queries.percentile(samples, .95), 190)
        with self.assertRaisesRegex(ValueError, "not unique"):
            queries.position("ambiguous ambiguous", "ambiguous")


if __name__ == "__main__":
    unittest.main()
