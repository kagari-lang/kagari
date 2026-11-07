"""LSP process benchmark with protocol and exact diagnostic suites.

uv run python scripts/benchmark_lsp.py --check --transport stdio
uv run python scripts/benchmark_lsp.py --check --transport tcp
uv run python scripts/benchmark_lsp.py --output target/lsp-benchmark/BASELINE
uv run python scripts/benchmark_lsp.py --baseline target/lsp-benchmark/BASELINE --output target/lsp-benchmark/CANDIDATE

Build time is reported separately. Runs freeze the exact Cargo artifact (or an
explicit --candidate image), inputs, metadata and raw samples in a new directory.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import time
import uuid

from lsp_benchmark_inputs import ROOT, digest, materialize, object_digest
from lsp_benchmark_diagnostics import APP, API, UNAFFECTED, audit_notifications, exercise, frozen_scenario, variants
from lsp_benchmark_protocol import Peer
from lsp_benchmark_results import require, summarize, unavailable, validate_compatible, validate_report


def command(args, timeout=120):
    result = subprocess.run(args, cwd=ROOT, capture_output=True, text=True, encoding="utf-8", errors="strict", timeout=timeout)
    require(result.returncode == 0, f"command failed ({result.returncode}): {args!r}\n{result.stderr}\n{result.stdout}")
    return result.stdout.strip()


def save(path, value):
    with path.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, indent=2, sort_keys=True, allow_nan=False)
        stream.write("\n")


def decode(path):
    def unique(pairs):
        value = {}
        for key, item in pairs:
            require(key not in value, f"duplicate report key {key}")
            value[key] = item
        return value

    def invalid(value):
        raise ValueError(f"invalid JSON number: {value}")

    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique, parse_constant=invalid)


def build_candidate(profile):
    started = time.perf_counter_ns()
    output = command(["cargo", "build", "--locked", "-p", "kagari-analyzer", "--bin", "kagari-analyzer",
                      "--profile", profile, "--message-format=json"], timeout=1200)
    executables = []
    for line in output.splitlines():
        artifact = json.loads(line)
        if artifact.get("reason") == "compiler-artifact" and artifact.get("target", {}).get("name") == "kagari-analyzer":
            if artifact.get("executable"):
                executables.append(Path(artifact["executable"]).resolve())
    require(len(executables) == 1 and executables[0].is_file(), "Cargo did not identify exactly one analyzer executable")
    return executables[0], time.perf_counter_ns() - started


def source_state():
    names = command(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"]).split("\0")
    paths = sorted({name for name in names if name and (Path(name).suffix in (".rs", ".kgr") or
                    Path(name).name in ("Cargo.toml", "Cargo.lock") or name.startswith("scripts/lsp_benchmark") or
                    name in ("scripts/benchmark_lsp.py", "crates/kagari-analyzer/tests/fixtures/benchmark/manifest.json"))})
    hashes = {name: digest((ROOT / name).read_bytes()) if (ROOT / name).is_file() else "deleted" for name in paths}
    patch = command(["git", "diff", "--no-ext-diff", "--no-textconv", "--binary", "HEAD", "--",
                     "*.rs", "*.kgr", "Cargo.lock", "Cargo.toml", "**/Cargo.toml", "scripts/*lsp*"])
    return {"git_revision": command(["git", "rev-parse", "HEAD"]), "source_sha256": object_digest(hashes),
            "dirty_patch_sha256": digest(patch.encode()), "files": hashes}


def metadata(args, inputs):
    return {
        "rustc": command(["rustc", "-Vv"]), "cargo": command(["cargo", "-V"]),
        "platform": platform.platform(), "machine": platform.machine(), "cpu": platform.processor(),
        "logical_cpus": os.cpu_count(), "power_mode": args.power_mode,
        "profile": args.profile, "features": ["default"], "transport": args.transport,
        "suite": args.suite, "diagnostic_deadline_seconds": 30, "debounce_policy_ms": 150,
        "lock_sha256": digest((ROOT / "Cargo.lock").read_bytes()), "workload_sha256": object_digest(inputs),
        "protocol_schema": 1, "cache_state": "fresh process and empty isolated generated-document cache; OS filesystem caches uncontrolled",
        "parallelism": "serial benchmark processes; Cargo default build parallelism",
        "lattice_revision": "10df090d7912df233e28fbe2c2ef3f223dafcdbb",
        "harness_sha256": object_digest({path.name: digest(path.read_bytes()) for path in
                                        [Path(__file__), *(ROOT / "scripts").glob("lsp_benchmark_*.py")]}),
        "source": source_state(), "created_utc": datetime.now(timezone.utc).isoformat(),
    }


def freeze(source, output, name):
    destination = output / (name + source.suffix)
    expected = digest(source.read_bytes())
    shutil.copy2(source, destination)
    require(digest(source.read_bytes()) == digest(destination.read_bytes()) == expected,
            "executable changed while archiving")
    return {"path": destination.name, "sha256": expected, "bytes": destination.stat().st_size}


def archive_path(directory, name):
    path = (directory / name).resolve()
    require(path.is_relative_to(directory.resolve()), "archived path escaped its directory")
    return path


def verify_inputs(directory, inputs, suite):
    sources = inputs["sources"] if suite == "diagnostics" else inputs
    for name, expected in sources["file_sha256"].items():
        require(digest(archive_path(directory / "inputs", name).read_bytes()) == expected, "archived source input hash mismatch")
    if suite == "diagnostics":
        require(decode(directory / "edit-inputs.json") == variants(), "archived edit inputs changed")


def sample(executable, transport, root, image, group, suite):
    cache = root.parent / "caches" / f"{group}-{image}"
    with Peer(executable, transport, root, cache) as peer:
        result, duration, received = peer.request("initialize", "initialize", {
            "processId": os.getpid(), "rootUri": root.as_uri(),
            "workspaceFolders": [{"uri": root.as_uri(), "name": "benchmark"}],
            "capabilities": {"general": {"positionEncodings": ["utf-16"]},
                             "textDocument": {"publishDiagnostics": {"versionSupport": True}}},
        })
        capabilities = result.get("capabilities", {})
        sync = capabilities.get("textDocumentSync", {})
        server = result.get("serverInfo", {})
        evidence = {"positionEncoding": capabilities.get("positionEncoding"),
                    "openClose": sync.get("openClose"), "change": sync.get("change"),
                    "serverName": server.get("name"), "serverVersion": server.get("version")}
        startup = received - peer.spawn_ns
        peer.notify("initialized", {})
        metrics, operations = exercise(peer, root) if suite == "diagnostics" else ({}, [])
        shutdown, _, _ = peer.request("shutdown", "shutdown", None)
        evidence["shutdown_is_null"] = shutdown is None
        peer.notify("exit", None)
        peer.finish()
        evidence["exit_code"] = peer.process.returncode
        value = {"image": image, "group": group,
                 "workload": "B01+B03.diagnostics" if suite == "diagnostics" else "B01.protocol",
                 "errors": 0, "completed": 2 + len(operations),
                 "spawn_to_initialize_ns": startup, "initialize_round_trip_ns": duration, "evidence": evidence}
        if suite == "diagnostics":
            audit_notifications(peer.notification_history, root, operations[0]["started_ns"])
            value.update(metrics)
            value.update(operations=operations, document_uris={name: (root / name).as_uri() for name in (APP, API, UNAFFECTED)},
                         notification_history=[{"frame": frame, "received_ns": received} for frame, received in peer.notification_history])
        return value


def run(args):
    if args.validate:
        report = decode(args.validate / "report.json")
        validate_report(report)
        require(decode(args.validate / "summary.json") == summarize(report), "saved summary disagrees with raw samples")
        for image in report["images"].values():
            require(digest(archive_path(args.validate, image["path"]).read_bytes()) == image["sha256"], "archived image hash mismatch")
        verify_inputs(args.validate, report["inputs"], report["metadata"]["suite"])
        print(f"validated {args.validate}")
        return
    output = args.output or ROOT / "target/lsp-benchmark" / (datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid.uuid4().hex[:8])
    output = output.resolve()
    require(not output.exists(), "output must be a new directory; existing reports are immutable")
    require(output.is_relative_to((ROOT / "target").resolve()), "benchmark output must remain under ignored target/")
    output.mkdir(parents=True)
    inputs = materialize(args.workload, output / "inputs")
    if args.suite == "diagnostics":
        require(args.workload == "pilot", "diagnostics suite currently requires the frozen pilot")
        inputs = {"sources": inputs, "diagnostics": frozen_scenario()}
        save(output / "edit-inputs.json", variants())
    before = source_state()
    candidate, build_ns = (args.candidate.resolve(), None) if args.candidate else build_candidate(args.profile)
    require(candidate.is_file(), "candidate executable does not exist")
    current = metadata(args, inputs)
    require(current["source"] == before, "source changed during build; discard this campaign")
    current["build_ns"] = build_ns
    current["image_origin"] = "explicit --candidate; profile supplied by caller" if args.candidate else "Cargo compiler-artifact executable"
    images = {"candidate": freeze(candidate, output, "candidate")}
    if args.baseline:
        previous = decode(args.baseline / "report.json")
        validate_report(previous)
        require(not previous["check_only"], "a correctness-only report cannot be a timing baseline")
        validate_compatible(previous["metadata"], current)
        image = previous["images"]["candidate"]
        executable = (args.baseline / image["path"]).resolve()
        require(executable.is_relative_to(args.baseline.resolve()), "baseline image escaped its archive")
        require(digest(executable.read_bytes()) == image["sha256"], "baseline image changed")
        images["baseline"] = freeze(executable, output, "baseline")
        current["baseline_metadata"] = previous["metadata"]
    groups = 1 if args.check else 7
    report = {"schema": 1, "metadata": current, "groups": groups, "check_only": args.check,
              "images": images, "inputs": inputs, "source_before": before,
              "samples": [], "unavailable_rows": unavailable(args.suite)}
    save(output / "metadata.json", current)
    save(output / "inputs.json", inputs)
    for group in range(groups):
        order = ["baseline", "candidate"] if "baseline" in images else ["candidate"]
        if group % 2:
            order.reverse()
        for name in order:
            value = sample(output / images[name]["path"], args.transport, output / "inputs", name, group, args.suite)
            report["samples"].append(value)
            save(output / f"sample-{group}-{name}.json", value)
    report["source_after"] = source_state()
    require(report["source_after"] == before, "source changed during campaign; results are not accepted")
    verify_inputs(output, inputs, args.suite)
    summary = summarize(report)
    save(output / "report.json", report)
    save(output / "summary.json", summary)
    print(json.dumps({"output": str(output), "summary": summary}, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="one process; correctness evidence only")
    parser.add_argument("--suite", choices=("protocol", "diagnostics"), default="protocol")
    parser.add_argument("--workload", choices=("pilot", "workspace", "churn"), default="pilot")
    parser.add_argument("--transport", choices=("stdio", "tcp"), default="stdio")
    parser.add_argument("--candidate", type=Path)
    parser.add_argument("--baseline", type=Path, help="immutable prior report archive; replay its frozen executable")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--validate", type=Path, help="validate an existing report without launching a server")
    parser.add_argument("--profile", choices=("release", "dev"), default="release")
    parser.add_argument("--power-mode", default="unreported", help="record the machine power policy; no implicit performance claim")
    args = parser.parse_args()
    try:
        run(args)
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        print(f"LSP benchmark failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
