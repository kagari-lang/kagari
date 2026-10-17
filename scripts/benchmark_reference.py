"""Freeze and validate an independent Lua preparation control for paired subjects."""
from __future__ import annotations

from collections import defaultdict
import csv
import hashlib
import json
import os
from pathlib import Path, PurePosixPath, PureWindowsPath
import shutil
from statistics import median
import subprocess

SOURCE = "benchmarks/lua-comparison/examples/lua_setup_reference.rs"
PHASES = ("engine_init", "source_to_chunk", "module_init")
BATCH = 32
SAMPLES = 3


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(f"Lua setup reference: {message}")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def absolute(path: object) -> bool:
    return isinstance(path, str) and bool(path) and (
        PurePosixPath(path).is_absolute() or PureWindowsPath(path).is_absolute())


def is_setup(row: dict) -> bool:
    return row["engine"] == "lua54" and row["phase"] in PHASES


def freeze(root: Path, destination: Path) -> None:
    """Build only the reference example, preserving the ordinary subject image."""
    subprocess.run(["cargo", "build", "--release", "--locked", "-p", "kagari-lua-benchmark",
                    "--example", "lua_setup_reference"], cwd=root, check=True)
    executable = root / "target/release/examples/lua_setup_reference"
    if (executable.with_suffix(".exe")).is_file():
        executable = executable.with_suffix(".exe")
    destination = destination.resolve()
    require(destination != executable.resolve(), "freeze outside Cargo's output")
    binary_hash = digest(executable)
    require(not destination.exists() or digest(destination) == binary_hash,
            "destination contains another image; use a new path")
    command = lambda *args: subprocess.check_output(args, cwd=root, text=True, encoding="utf-8").strip()
    manifest = dict(executable=str(destination), binary_sha256=binary_hash,
                    source_sha256={SOURCE: digest(root / SOURCE)},
                    lock_sha256=digest(root / "Cargo.lock"), rustc=command("rustc", "-Vv"),
                    cargo=command("cargo", "-V"), profile="release (workspace defaults, opt-level=3)",
                    features="mlua lua54,vendored", batch=BATCH, samples=SAMPLES,
                    environment={key: os.environ[key] for key in (
                        "CARGO_BUILD_JOBS", "CARGO_TARGET_DIR", "RUSTFLAGS", "CFLAGS", "CC", "DEVELOPER_DIR")
                        if key in os.environ})
    manifest_path = Path(str(destination) + ".json")
    require(not manifest_path.exists()
            or json.loads(manifest_path.read_text(encoding="utf-8")) == manifest,
            "destination manifest has another contract")
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(executable, destination)
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"Frozen Lua setup reference: {destination}; SHA256={binary_hash}")


def load(root: Path, executable: Path, subject: dict) -> dict:
    executable = executable.resolve()
    manifest_path = Path(str(executable) + ".json")
    config = json.loads(manifest_path.read_text(encoding="utf-8"))
    require(config["executable"] == str(executable), "frozen path changed")
    require(config["binary_sha256"] == digest(executable), "frozen image changed")
    require(config["source_sha256"] == {SOURCE: digest(root / SOURCE)}, "runner source changed")
    for key in ("lock_sha256", "rustc", "cargo", "profile", "environment"):
        require(config[key] == subject[key], f"subject/reference {key} mismatch")
    require(config["features"] == "mlua lua54,vendored", "unexpected reference features")
    require(config["batch"] == BATCH and config["samples"] == SAMPLES, "changed sampling")
    config["manifest_sha256"] = digest(manifest_path)
    return config


def measure(root: Path, output: Path, metadata: dict, subject_rows: list[dict],
            run: int, pair: int, variant: str) -> tuple[list[dict], list[dict]]:
    """Run each fixed input after its subject process, outside every subject timer."""
    config = metadata["lua_setup_reference"]
    executable = Path(config["executable"])
    contracts = {}
    for row in subject_rows:
        if row["phase"] == "execute" and row["engine"] == "lua54":
            contract = row["size"], row["batch"], row["checksum"]
            require(contracts.setdefault(row["workload"], contract) == contract,
                    "subject execution contract changed")
    rows, processes = [], []
    for workload, (size, execution_batch, expected) in contracts.items():
        source_key = f"benchmarks/lua-comparison/workloads/{workload}.lua"
        source = (root / source_key).resolve()
        source_hash = metadata["source_sha256"][source_key]
        require(digest(source) == source_hash, "input source changed")
        require(digest(executable) == config["binary_sha256"], "image changed before process")
        args = [str(executable), "--source", str(source), "--workload", workload,
                "--size", str(size), "--execution-batch", str(execution_batch),
                "--expected-checksum", str(expected)]
        path = output / f"reference-{run}-{workload}.csv"
        with path.open("w", encoding="utf-8", newline="") as stdout, \
                (output / f"reference-{run}-{workload}.stderr.log").open("w", encoding="utf-8") as stderr:
            subprocess.run(args, cwd=root, stdout=stdout, stderr=stderr, check=True)
        require(digest(executable) == config["binary_sha256"], "image changed during process")
        require(digest(source) == source_hash, "input changed during process")
        with path.open(encoding="utf-8", newline="") as stream:
            measured = list(csv.DictReader(stream))
        for row in measured:
            row.update({key: int(row[key]) for key in ("size", "batch", "sample", "ns", "checksum")})
            row.update(run=run, pair=pair, variant=variant)
        rows.extend(measured)
        processes.append(dict(run=run, pair=pair, variant=variant, workload=workload,
                              executable=str(executable), binary_sha256=config["binary_sha256"],
                              source=str(source), source_key=source_key, source_sha256=source_hash,
                              execution_batch=execution_batch, expected_checksum=expected))
    return rows, processes


def summaries(report: dict) -> list[dict]:
    """Require independent provenance and every validation row before using timings."""
    metadata = report["metadata"]
    config = metadata.get("lua_setup_reference")
    require(isinstance(config, dict), "missing frozen reference metadata")
    require(absolute(config.get("executable")), "missing absolute image path")
    require(config["executable"] not in (metadata.get("candidate_executable"), metadata.get("baseline_executable"))
            and config.get("binary_sha256") not in (metadata.get("binary_sha256"), metadata.get("baseline_binary_sha256")),
            "reference must be independent of both subject images")
    require(config.get("batch") == BATCH and config.get("samples") == SAMPLES,
            "changed sampling")
    require(config.get("features") == "mlua lua54,vendored", "unexpected reference features")
    for key in ("lock_sha256", "rustc", "cargo", "profile", "environment"):
        require(config.get(key) == metadata[key], f"subject/reference {key} mismatch")
    source_hashes = config.get("source_sha256")
    require(isinstance(source_hashes, dict) and set(source_hashes) == {SOURCE},
            "missing runner source identity")
    for value in (config.get("binary_sha256"), config.get("manifest_sha256"), *source_hashes.values()):
        require(isinstance(value, str) and len(value) == 64
                and all(char in "0123456789abcdef" for char in value), "invalid image/source hash")
    contracts = {}
    for row in report["raw"]:
        if row["phase"] == "execute" and row["engine"] == "lua54":
            contracts[row["run"], row["workload"]] = row
    processes = report.get("reference_processes")
    require(isinstance(processes, list), "missing process provenance")
    seen = set()
    for process in processes:
        for field in ("run", "pair", "execution_batch", "expected_checksum"):
            require(type(process[field]) is int, f"non-integer process {field}")
        key = process["run"], process["workload"]
        require(key in contracts and key not in seen, "unexpected/duplicate reference process")
        seen.add(key)
        subject = contracts[key]
        require(process["pair"] == subject["pair"] and process["variant"] == subject["variant"],
                "process pair/order mismatch")
        require(process["executable"] == config["executable"]
                and process["binary_sha256"] == config["binary_sha256"], "process image/path changed")
        source_key = f"benchmarks/lua-comparison/workloads/{subject['workload']}.lua"
        require(process["source_key"] == source_key and absolute(process["source"])
                and process["source"].replace("\\", "/").endswith("/" + source_key),
                "unexpected source path")
        require(process["source_sha256"] == metadata["source_sha256"][source_key], "source changed")
        require(process["execution_batch"] == subject["batch"]
                and process["expected_checksum"] == subject["checksum"], "validation contract changed")
    require(seen == set(contracts), "missing reference process")
    raw = report.get("reference_raw")
    require(isinstance(raw, list), "missing reference samples")
    grouped = defaultdict(list)
    for row in raw:
        for field in ("size", "batch", "sample", "ns", "checksum", "run", "pair"):
            require(type(row[field]) is int, f"non-integer {field}")
        key = row["run"], row["workload"]
        require(key in contracts and row["engine"] == "lua54", "unexpected reference row")
        subject = contracts[key]
        require(row["pair"] == subject["pair"] and row["variant"] == subject["variant"],
                "sample pair/order mismatch")
        require(row["phase"] in (*PHASES, "validate"), "unexpected phase")
        require(row["size"] == subject["size"] and row["batch"] == BATCH and row["ns"] >= 0,
                "invalid size/batch/timing")
        expected = subject["checksum"] if row["phase"] == "validate" else 0
        require(row["checksum"] == expected, "checksum mismatch")
        require(row["phase"] != "validate" or row["ns"] == 0, "validation entered setup timing")
        grouped[*key, row["phase"]].append(row)
    result = []
    for (run, workload), subject in sorted(contracts.items()):
        for phase in (*PHASES, "validate"):
            rows = grouped.get((run, workload, phase), [])
            require(len(rows) == SAMPLES and sorted(r["sample"] for r in rows) == list(range(SAMPLES)),
                    f"missing/duplicate samples: {run}/{workload}/{phase}")
            if phase == "validate":
                continue
            values = [r["ns"] / BATCH for r in rows]
            result.append(dict(run=run, pair=subject["pair"], variant=subject["variant"],
                               phase=phase, workload=workload, engine="lua54", samples=SAMPLES,
                               median_ns=median(values), min_ns=min(values), max_ns=max(values),
                               origin="independent_lua_setup"))
    return result
