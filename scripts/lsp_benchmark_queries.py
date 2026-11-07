"""Frozen, independently checked query and implementation-index process workloads."""
from __future__ import annotations

import json
import math
import time

from lsp_benchmark_inputs import FIXTURES, object_digest
from lsp_benchmark_diagnostics import APP, API, UNAFFECTED, ERROR, SENTINEL, matches, variants

METHODS = ("hover", "definition", "implementation", "completion")
SAMPLES = 200
WARMUP = 20
LOAD_OFFERS = 40
COMPLETED_REQUESTS = 1 + len(METHODS) * (WARMUP + SAMPLES) + 2 * (LOAD_OFFERS + 1) + 12
METRICS = (("cold_implementation_ns",) + tuple(f"{method}_{tail}_ns" for method in METHODS for tail in ("p50", "p95"))
           + ("load_normal_p95_ns", "load_slow_p95_ns")
           + tuple(f"{name}_query_ns" for name in ("body_edit", "body_repair", "signature_edit", "signature_repair")))


def position(text, needle, advance=0):
    if text.count(needle) != 1:
        raise ValueError(f"query anchor is not unique: {needle!r}")
    offset = text.index(needle) + advance
    before = text[:offset]
    return {"line": before.count("\n"), "character": len(before.rsplit("\n", 1)[-1].encode("utf-16-le")) // 2}


def scenario():
    texts = variants()
    return {"recipe": "pilot-queries-v1", "samples_per_method": SAMPLES, "warmup_per_method": WARMUP,
            "interactive_load": {"offers": LOAD_OFFERS, "period_ns": 40_000_000, "edit_every": 4,
                                 "slow_read_seconds_per_frame": .04},
            "edit_queries": ["body_edit", "body_repair", "signature_edit", "signature_repair"],
            "clock": "before request write to complete independently decoded response",
            "sites": {
                "hover": {"file": APP, "position": position(texts["app"], "    value\n}", 4)},
                "definition": {"file": APP, "position": position(texts["app"], "api::default_value()", len("api::"))},
                "implementation": {"file": API, "position": position(texts["api"], "trait Metric", len("trait "))},
                "completion": {"file": APP, "position": position(texts["app"], "counter.value()", len("counter."))},
            }}


def frozen_scenario():
    value = scenario()
    frozen = json.loads((FIXTURES / "manifest.json").read_text(encoding="utf-8"))
    if object_digest(value) != frozen.get("query_scenario_sha256"):
        raise ValueError("query scenario changed; review/update its frozen manifest deliberately")
    return value


def assert_answer(method, result, root):
    """Check semantic sites, destinations and edits independently of server internals."""
    texts = variants()
    if method == "hover":
        if not isinstance(result, dict) or not isinstance(result.get("contents"), list):
            raise ValueError("missing hover contents")
        if not any(isinstance(item, dict) and item.get("value") == "value: i32" for item in result["contents"]):
            raise ValueError(f"hover did not return the selected local's checked type: {result!r}")
    elif method == "definition":
        expected = position(texts["api"], "fn default_value", len("fn "))
        if not isinstance(result, dict) or result.get("uri") != (root / API).as_uri() or result.get("range") != {
                "start": expected, "end": {**expected, "character": expected["character"] + len("default_value")}}:
            raise ValueError("definition did not return the authoritative API name range")
    elif method == "implementation":
        if not isinstance(result, list) or len(result) != 3:
            raise ValueError("implementation search did not return complete frozen coverage")
        expected = []
        for needle, width in (("Counter {\n    fn", 7), ("DefaultCounter {}\n\npub struct", 14), ("Box<T> {}\n\npub fn", 6)):
            start = position(texts["api"], needle)
            expected.append({"start": start, "end": {**start, "character": start["character"] + width}})
        actual = [item.get("range") for item in result if item.get("uri") == (root / API).as_uri()]
        if sorted(object_digest(site) for site in actual) != sorted(object_digest(site) for site in expected):
            raise ValueError("implementation selection sites changed or used the wrong source")
    elif method == "completion":
        if not isinstance(result, dict) or result.get("isIncomplete") is not False or not isinstance(result.get("items"), list):
            raise ValueError("missing complete completion list")
        items = {item.get("label"): item for item in result["items"]}
        if not {"current", "value"}.issubset(items):
            raise ValueError("completion lost a checked field or trait method")
        start = position(texts["app"], "counter.value()", len("counter."))
        end = {**start, "character": start["character"] + len("value")}
        for label in ("current", "value"):
            if items[label].get("textEdit") != {"range": {"start": start, "end": end}, "newText": label}:
                raise ValueError("completion replacement range or plain edit changed")
    else:
        raise ValueError("unknown query method")


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * fraction) - 1)]


def exercise(peer, root):
    schedule = frozen_scenario()
    texts = variants()
    started = time.perf_counter_ns()
    for path, variant in ((APP, "app"), (API, "api"), (UNAFFECTED, "unaffected")):
        peer.notify("textDocument/didOpen", {"textDocument": {"uri": (root / path).as_uri(),
            "languageId": "kagari", "version": 1, "text": texts[variant]}})
    for path, records in ((APP, [SENTINEL]), (API, []), (UNAFFECTED, [])):
        expected = {"version": 1, "diagnostics": records}
        peer.wait_notification("textDocument/publishDiagnostics", lambda params, path=path, expected=expected:
            matches(params, (root / path).as_uri(), expected), after_ns=started, timeout=30)
    requests = {}
    for method, site in schedule["sites"].items():
        requests[method] = {"textDocument": {"uri": (root / site["file"]).as_uri()}, "position": site["position"]}
    cold, elapsed, _ = peer.request("cold-implementation", "textDocument/implementation", requests["implementation"])
    assert_answer("implementation", cold, root)
    metrics = {"cold_implementation_ns": elapsed}
    operations = {"cold_implementation": {"duration_ns": elapsed, "result": cold}, "methods": {}}
    identifier = 0
    for method in METHODS:
        samples = []
        canonical = None
        for index in range(WARMUP + SAMPLES):
            identifier += 1
            result, elapsed, received = peer.request(identifier, "textDocument/" + method, requests[method])
            assert_answer(method, result, root)
            digest = object_digest(result)
            if canonical is None:
                canonical = result
            elif result != canonical:
                raise ValueError("unchanged warm query returned a different semantic answer")
            if index >= WARMUP:
                samples.append({"duration_ns": elapsed, "received_ns": received, "answer_sha256": digest})
        metrics[f"{method}_p50_ns"] = percentile([sample["duration_ns"] for sample in samples], 0.50)
        metrics[f"{method}_p95_ns"] = percentile([sample["duration_ns"] for sample in samples], 0.95)
        operations["methods"][method] = {"canonical": canonical, "samples": samples}
    version = 1
    operations["loads"] = {}
    for name, read_delay in (("normal", 0), ("slow", .04)):
        load, version = exercise_load(peer, root, texts, requests, version, read_delay, name)
        operations["loads"][name] = load
        metrics[f"load_{name}_p95_ns"] = percentile([item["duration_ns"] for item in load["responses"]], .95)
    edits = exercise_edits(peer, root, texts, requests, version)
    operations["edit_queries"] = edits
    for edit in edits:
        metrics[f"{edit['name']}_query_ns"] = edit["affected_received_ns"] - edit["started_ns"]
    return metrics, operations


def hover_type(answer, expected):
    if not isinstance(answer, dict) or not isinstance(answer.get("contents"), list):
        raise ValueError("edit query lost its checked hover")
    if not any(isinstance(item, dict) and item.get("value") == expected for item in answer["contents"]):
        raise ValueError("edit query returned stale or unknown type facts")


def exercise_edits(peer, root, texts, requests, version):
    unaffected_text = texts["unaffected"]
    offset = unaffected_text.rindex("+ 1;") + 2
    prefix = unaffected_text[:offset]
    unaffected_site = {"textDocument": {"uri": (root / UNAFFECTED).as_uri()}, "position": {
        "line": prefix.count("\n"), "character": len(prefix.rsplit("\n", 1)[-1].encode("utf-16-le")) // 2}}
    operations = []
    for index, (name, path, variant, expected) in enumerate((
            ("body_edit", APP, "body_error", "bool"), ("body_repair", APP, "app", "value: i32"),
            ("signature_edit", API, "signature_error", "value: bool"), ("signature_repair", API, "api", "value: i32"))):
        document_version = version + index + 1 if path == APP else index
        started = time.perf_counter_ns()
        peer.notify("textDocument/didChange", {"textDocument": {"uri": (root / path).as_uri(), "version": document_version},
            "contentChanges": [{"text": texts[variant]}]})
        affected_site = requests["hover"]
        answer, elapsed, received = peer.request(f"edit-{name}", "textDocument/hover", affected_site)
        hover_type(answer, expected)
        unaffected, unaffected_elapsed, _ = peer.request(f"edit-{name}-unaffected", "textDocument/hover", unaffected_site)
        hover_type(unaffected, "i32")
        definition, definition_elapsed, _ = peer.request(f"edit-{name}-definition", "textDocument/definition", requests["definition"])
        assert_answer("definition", definition, root)
        app_version = document_version if path == APP else version + 2
        records = [SENTINEL] + ([ERROR] if name in ("body_edit", "signature_edit") else [])
        diagnostics, diagnostic_received = peer.wait_notification("textDocument/publishDiagnostics", lambda params:
            matches(params, (root / APP).as_uri(), {"version": app_version, "diagnostics": records}), after_ns=started, timeout=30)
        operations.append({"name": name, "file": path, "version": document_version, "variant": variant,
            "text_sha256": object_digest(texts[variant]), "started_ns": started, "affected_received_ns": received,
            "affected_round_trip_ns": elapsed, "affected": answer, "unaffected_round_trip_ns": unaffected_elapsed,
            "unaffected": unaffected, "definition_round_trip_ns": definition_elapsed, "definition": definition,
            "diagnostic_received_ns": diagnostic_received, "diagnostics": diagnostics})
    return operations


def exercise_load(peer, root, texts, requests, version, read_delay, name):
    """Offer independently of responses; retain lateness instead of hiding overload."""
    config = frozen_scenario()["interactive_load"]
    peer.read_delay_seconds = read_delay
    offered = []
    edits = []
    started = time.perf_counter_ns()
    for index in range(config["offers"]):
        planned = started + index * config["period_ns"]
        time.sleep(max(0, (planned - time.perf_counter_ns()) / 1e9))
        if index % config["edit_every"] == 0:
            version += 1
            variant = "body_error" if version % 2 == 0 else "app"
            peer.notify("textDocument/didChange", {"textDocument": {"uri": (root / APP).as_uri(), "version": version},
                "contentChanges": [{"text": texts[variant]}]})
            edits.append({"version": version, "variant": variant, "sha256": object_digest(texts[variant])})
        if index == 0:
            index_started = peer.begin_request(f"load-{name}-index", "textDocument/implementation", requests["implementation"])
        method = "hover" if index % 2 == 0 else "completion"
        identifier = f"load-{name}-{index}"
        written = peer.begin_request(identifier, "textDocument/" + method, requests[method])
        offered.append({"id": identifier, "method": method, "planned_ns": planned, "written_ns": written,
                        "version": version, "variant": variant})
    pending = len(peer.pending)
    index_answer, index_duration, _ = peer.response(f"load-{name}-index", index_started)
    assert_answer("implementation", index_answer, root)
    responses = []
    for offer in offered:
        answer, elapsed, received = peer.response(offer["id"], offer["written_ns"])
        if offer["method"] == "hover":
            hover_type(answer, "bool" if offer["variant"] == "body_error" else "value: i32")
        else:
            assert_answer(offer["method"], answer, root)
        responses.append({**offer, "duration_ns": elapsed, "received_ns": received, "answer": answer,
                          "answer_sha256": object_digest(answer), "canonical_result_json_bytes": len(json.dumps(answer, ensure_ascii=False, separators=(",", ":")).encode())})
    peer.read_delay_seconds = 0
    variant = edits[-1]["variant"]
    expected = {"version": version, "diagnostics": [SENTINEL] + ([ERROR] if variant == "body_error" else [])}
    peer.wait_notification("textDocument/publishDiagnostics", lambda params: matches(params, (root / APP).as_uri(), expected), after_ns=started, timeout=30)
    return {"offered": len(offered), "completed": len(responses), "errors": 0, "config": config, "edits": edits,
            "reader_delay_seconds_per_frame": read_delay, "client_pending_ids_before_collection": pending,
            "index_duration_ns": index_duration, "responses": responses,
            "scope": "client offer/write/decode times; client pending IDs include decoded responses; payload bytes are canonical result JSON, excluding envelopes/framing; server admission/queue times are not measured"}, version


def validate(sample, root):
    schedule = frozen_scenario()
    operations = sample.get("query_operations", {})
    cold = operations.get("cold_implementation", {})
    assert_answer("implementation", cold.get("result"), root)
    if type(cold.get("duration_ns")) is not int or cold["duration_ns"] <= 0 or cold["duration_ns"] != sample.get("cold_implementation_ns"):
        raise ValueError("invalid cold implementation timing")
    if set(operations.get("methods", {})) != set(METHODS):
        raise ValueError("missing warm query method")
    for method in METHODS:
        operation = operations["methods"][method]
        canonical = operation.get("canonical")
        assert_answer(method, canonical, root)
        samples = operation.get("samples", [])
        if len(samples) != schedule["samples_per_method"]:
            raise ValueError("wrong warm sample count")
        previous = 0
        for observation in samples:
            elapsed, received = observation.get("duration_ns"), observation.get("received_ns")
            if (type(elapsed) is not int or elapsed <= 0 or type(received) is not int or received <= previous
                    or elapsed >= received or observation.get("answer_sha256") != object_digest(canonical)):
                raise ValueError("invalid warm query timestamp or semantic answer")
            previous = received
        for tail, fraction in (("p50", .50), ("p95", .95)):
            if sample.get(f"{method}_{tail}_ns") != percentile([item["duration_ns"] for item in samples], fraction):
                raise ValueError("query percentile disagrees with raw samples")
    loads = operations.get("loads", {})
    if set(loads) != {"normal", "slow"}:
        raise ValueError("missing interactive load route")
    for name, delay in (("normal", 0), ("slow", .04)):
        load = loads[name]
        config = schedule["interactive_load"]
        if (load.get("config") != config or load.get("reader_delay_seconds_per_frame") != delay
                or load.get("offered") != config["offers"] or load.get("completed") != config["offers"]
                or load.get("errors") != 0 or load.get("client_pending_ids_before_collection") != config["offers"] + 1):
            raise ValueError("load configuration/count/reader bound changed")
        edits = load.get("edits", [])
        first_version = 2 if name == "normal" else 12
        expected_edits = []
        for version in range(first_version, first_version + 10):
            variant = "body_error" if version % 2 == 0 else "app"
            expected_edits.append({"version": version, "variant": variant, "sha256": object_digest(variants()[variant])})
        if edits != expected_edits:
            raise ValueError("interactive edit schedule changed")
        responses = load.get("responses", [])
        if len(responses) != config["offers"]:
            raise ValueError("load silently lost offered requests")
        previous_plan = None
        for index, response in enumerate(responses):
            method = "hover" if index % 2 == 0 else "completion"
            if (response.get("method") != method or response.get("id") != f"load-{name}-{index}"
                    or response.get("answer_sha256") != object_digest(response.get("answer"))
                    or response.get("version") != first_version + index // config["edit_every"]
                    or response.get("variant") != ("body_error" if response["version"] % 2 == 0 else "app")
                    or type(response.get("planned_ns")) is not int or response["written_ns"] < response["planned_ns"]
                    or response["received_ns"] - response["written_ns"] != response.get("duration_ns")
                    or response["duration_ns"] <= 0 or response.get("canonical_result_json_bytes") != len(json.dumps(response.get("answer"), ensure_ascii=False, separators=(",", ":")).encode())):
                raise ValueError("load answer/timestamp/output evidence changed")
            if method == "hover":
                hover_type(response["answer"], "bool" if response["variant"] == "body_error" else "value: i32")
            else:
                assert_answer(method, response["answer"], root)
            if previous_plan is not None and response["planned_ns"] - previous_plan != config["period_ns"]:
                raise ValueError("load offer schedule changed")
            previous_plan = response["planned_ns"]
        if sample.get(f"load_{name}_p95_ns") != percentile([item["duration_ns"] for item in responses], .95):
            raise ValueError("load percentile disagrees with offered requests")
    edits = operations.get("edit_queries", [])
    if [edit.get("name") for edit in edits] != schedule["edit_queries"]:
        raise ValueError("missing query invalidation route")
    previous = 0
    for index, (edit, expected, variant) in enumerate(zip(edits, ("bool", "value: i32", "value: bool", "value: i32"),
            ("body_error", "app", "signature_error", "api"))):
        name = schedule["edit_queries"][index]
        version = 22 + index if index < 2 else index
        app_version = version if index < 2 else 23
        if (edit.get("file") != (APP if index < 2 else API) or edit.get("version") != version or edit.get("variant") != variant
                or edit.get("text_sha256") != object_digest(variants()[variant]) or edit.get("started_ns", 0) <= previous
                or edit.get("affected_received_ns", 0) <= edit["started_ns"]
                or edit["affected_received_ns"] - edit["started_ns"] != sample.get(f"{name}_query_ns")
                or not 0 < edit.get("affected_round_trip_ns", 0) <= sample[f"{name}_query_ns"]
                or edit.get("unaffected_round_trip_ns", 0) <= 0 or edit.get("definition_round_trip_ns", 0) <= 0):
            raise ValueError("query invalidation timing/input evidence changed")
        hover_type(edit.get("affected"), expected)
        hover_type(edit.get("unaffected"), "i32")
        assert_answer("definition", edit.get("definition"), root)
        records = [SENTINEL] + ([ERROR] if index in (0, 2) else [])
        if not matches(edit.get("diagnostics"), (root / APP).as_uri(), {"version": app_version, "diagnostics": records}):
            raise ValueError("query invalidation did not reach its diagnostic state")
        if edit.get("diagnostic_received_ns", 0) <= edit["started_ns"]:
            raise ValueError("query edit diagnostic clock inversion")
        previous = max(edit["affected_received_ns"], edit["diagnostic_received_ns"])
