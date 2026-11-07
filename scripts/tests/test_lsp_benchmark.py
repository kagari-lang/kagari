"""Offline archive/peer contracts; incorrect runs must never become speed evidence."""
from copy import deepcopy
from io import BytesIO
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import lsp_benchmark_inputs as inputs
import lsp_benchmark_protocol as protocol
import lsp_benchmark_results as results


def report(paired=False, check=False):
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
    metadata.update(workload_sha256=inputs.object_digest({}), source=source)
    return {"schema": 1, "metadata": metadata, "inputs": {}, "source_before": source, "source_after": source,
            "groups": groups, "check_only": check, "images": images, "samples": samples,
            "unavailable_rows": ["B01.diagnostics", "B02", "B03", "B04", "B05", "B06", "B07", "B08"]}


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
        for key in ("workload_sha256", "lock_sha256", "rustc", "transport", "power_mode"):
            changed = {**metadata, key: "different"}
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, key):
                results.validate_compatible(metadata, changed)


class PeerContracts(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
