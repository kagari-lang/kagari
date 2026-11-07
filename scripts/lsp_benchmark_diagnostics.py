"""Frozen pilot diagnostic witnesses and exact independent notification checks."""
from __future__ import annotations

import json
import time

from lsp_benchmark_inputs import FIXTURES, digest, object_digest, source_files

ERROR = {"code": "KG_TYPE_RETURN_TYPE_MISMATCH", "severity": 1, "source": "kagari",
         "message": "return type mismatch in function `baseline`: expected `i32`, found `bool`",
         "range": {"start": {"line": 16, "character": 0}, "end": {"line": 19, "character": 1}}}
SENTINEL = {"code": "KG_TYPE_RETURN_TYPE_MISMATCH", "severity": 1, "source": "kagari",
            "message": "return type mismatch in function `benchmark_guard`: expected `i32`, found `bool`",
            "range": {"start": {"line": 29, "character": 0}, "end": {"line": 31, "character": 1}}}
APP = "src/lib.kgr"
API = "api/src/lib.kgr"
UNAFFECTED = "src/scale0.kgr"
METRICS = ("open_to_first_diagnostics_ns", "body_error_ns", "body_repair_ns",
           "signature_error_ns", "signature_repair_ns")


def variants():
    sources = source_files("pilot")
    original = {name: sources[name].decode("utf-8") for name in (APP, API, UNAFFECTED)}
    original[APP] += "\n" + (FIXTURES / "diagnostic-sentinel.kgr").read_text(encoding="utf-8")
    body = "pub fn baseline() -> i32 {\n    val value = api::default_value();\n    value\n}"
    signature = "pub fn default_value() -> i32 {\n    7\n}"
    if original[APP].count(body) != 1 or original[API].count(signature) != 1:
        raise ValueError("frozen diagnostic edit anchors changed")
    return {"app": original[APP], "api": original[API], "unaffected": original[UNAFFECTED],
            "body_error": original[APP].replace(body, body.replace("    value\n}", "    true\n}")),
            "signature_error": original[API].replace(signature, signature.replace("i32", "bool").replace("7", "true"))}


def scenario():
    texts = variants()
    # The initial error is mandatory: an empty clear cannot establish readiness.
    operations = [
        ("open", "open_to_first_diagnostics_ns", [(APP, 1, "body_error"), (API, 1, "api"), (UNAFFECTED, 1, "unaffected")],
         {APP: (1, [ERROR, SENTINEL]), API: (1, []), UNAFFECTED: (1, [])}),
        ("startup_repair", None, [(APP, 2, "app")], {APP: (2, [SENTINEL])}),
        ("body_error", "body_error_ns", [(APP, 3, "body_error")], {APP: (3, [ERROR, SENTINEL])}),
        ("body_repair", "body_repair_ns", [(APP, 4, "app")], {APP: (4, [SENTINEL])}),
        ("signature_error", "signature_error_ns", [(API, 2, "signature_error")], {APP: (4, [ERROR, SENTINEL]), API: (2, [])}),
        ("signature_repair", "signature_repair_ns", [(API, 3, "api")], {APP: (4, [SENTINEL]), API: (3, [])}),
    ]
    return {"recipe": "pilot-diagnostics-v1", "debounce_policy_ms": 150,
            "clock": "client monotonic; before notification write to fully decoded expected publication",
            "steps": [{"name": name, "metric": metric,
                       "changes": [{"file": path, "version": version, "variant": variant,
                                    "sha256": digest(texts[variant].encode())} for path, version, variant in changes],
                       "expected": {path: {"version": version, "diagnostics": diagnostics}
                                    for path, (version, diagnostics) in expected.items()}}
                      for name, metric, changes, expected in operations]}


def matches(params, uri, expected):
    if (not isinstance(params, dict) or params.get("uri") != uri or type(params.get("version")) is not int
            or params["version"] != expected["version"]):
        return False
    actual = params.get("diagnostics")
    if not isinstance(actual, list) or len(actual) != len(expected["diagnostics"]):
        return False
    if not all(isinstance(observed, dict) for observed in actual):
        return False
    keys = {key for required in expected["diagnostics"] for key in required}
    return sorted(object_digest({key: observed.get(key) for key in keys}) for observed in actual) == sorted(
        object_digest(required) for required in expected["diagnostics"])


def frozen_scenario():
    value = scenario()
    frozen = json.loads((FIXTURES / "manifest.json").read_text(encoding="utf-8"))
    if object_digest(value) != frozen.get("diagnostic_scenario_sha256"):
        raise ValueError("diagnostic scenario changed; review/update its frozen manifest deliberately")
    return value


def exercise(peer, root):
    texts = variants()
    raw = []
    metrics = {}
    for step in frozen_scenario()["steps"]:
        started = time.perf_counter_ns()
        for change in step["changes"]:
            document = {"uri": (root / change["file"]).as_uri(), "version": change["version"]}
            text = texts[change["variant"]]
            if step["name"] == "open":
                document.update(languageId="kagari", text=text)
                peer.notify("textDocument/didOpen", {"textDocument": document})
            else:
                peer.notify("textDocument/didChange", {"textDocument": document, "contentChanges": [{"text": text}]})
        observations = {}
        for path, expected in step["expected"].items():
            uri = (root / path).as_uri()
            value, received = peer.wait_notification("textDocument/publishDiagnostics",
                lambda params, uri=uri, expected=expected: matches(params, uri, expected), after_ns=started, timeout=30)
            observations[path] = {"params": value, "received_ns": received}
        ended = observations[APP]["received_ns"] if step["name"] == "open" else max(item["received_ns"] for item in observations.values())
        duration = ended - started
        if step["metric"]:
            metrics[step["metric"]] = duration
        raw.append({"name": step["name"], "started_ns": started, "completed_ns": ended,
                    "duration_ns": duration, "changes": step["changes"], "observations": observations})
    return metrics, raw


def audit_notifications(history, root, after_ns):
    uri = (root / UNAFFECTED).as_uri()
    for value, received in history:
        if received >= after_ns and value.get("method") == "textDocument/publishDiagnostics" and value.get("params", {}).get("uri") == uri:
            if not matches(value["params"], uri, {"version": 1, "diagnostics": []}):
                raise ValueError("unaffected pilot module acquired unexpected diagnostics or version")
