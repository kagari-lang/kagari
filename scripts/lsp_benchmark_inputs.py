"""Deterministic semantic scale inputs; no runtime-generated fixture language."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "crates/kagari-analyzer/tests/fixtures/benchmark"
SHAPES = {"pilot": (2, 10, 110), "workspace": (4, 50, 100), "churn": (2, 10, 110)}


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def object_digest(value) -> str:
    return digest(json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode())


def source_files(shape: str) -> dict[str, bytes]:
    packages, files_per_package, functions = SHAPES[shape]
    names = ["app", "api", "model", "helpers"][:packages]
    files = {}
    for package in names:
        directory = "" if package == "app" else f"{package}/"
        manifest = f'[package]\nname = "{package}"\nversion = "1.0.0"\n'
        if package == "app":
            manifest += '[workspace]\nnamespace = "benchmark/lsp"\nmembers = ['
            manifest += ", ".join(json.dumps(name) for name in names[1:]) + "]\n[dependencies]\n"
            manifest += "".join(f'{name} = {{ path = "{name}" }}\n' for name in names[1:])
        files[f"{directory}Kagari.toml"] = manifest.encode()
        root = "".join(f"pub mod scale{index};\n" for index in range(files_per_package - 1))
        root += "\n"
        if package in ("app", "api"):
            root += (FIXTURES / f"{package}.kgr").read_text(encoding="utf-8").replace("\r\n", "\n")
        root += "\npub fn scaled() -> i32 {\n    "
        root += " + ".join(f"scale{index}::f{functions - 1}(1)" for index in range(files_per_package - 1))
        root += "\n}\n"
        files[f"{directory}src/lib.kgr"] = root.encode()
        for index in range(files_per_package - 1):
            blocks = []
            for number in range(functions):
                previous = "value" if number == 0 else f"f{number - 1}(value)"
                blocks.append(f"pub fn f{number}(value: i32) -> i32 {{\n    val next = {previous} + 1;\n    next\n}}\n")
            text = "\n".join(blocks)
            # This one file deliberately exercises authored CRLF coordinates.
            if package == "app" and index == 0:
                text = text.replace("\n", "\r\n")
            files[f"{directory}src/scale{index}.kgr"] = text.encode()
    return files


def manifest(shape: str) -> dict:
    files = source_files(shape)
    sources = {name: data for name, data in files.items() if name.endswith(".kgr")}
    packages, files_per_package, functions = SHAPES[shape]
    edits = []
    if shape == "churn":
        # Alternate the body literal while leaving live source size unchanged.
        edits = [{"version": index + 2, "file": "src/scale0.kgr", "literal": 2 if index % 2 == 0 else 1}
                 for index in range(1000)]
    return {
        "schema": 1,
        "recipe": "semantic-function-chain-v1",
        "workload": shape,
        "packages": packages,
        "source_files": len(sources),
        "source_bytes": sum(map(len, sources.values())),
        "source_lines": sum(data.count(b"\n") for data in sources.values()),
        "generated_function_bodies": packages * (files_per_package - 1) * functions,
        "generated_call_references": packages * (files_per_package - 1) * (functions - 1),
        "file_sha256": {name: digest(data) for name, data in sorted(files.items())},
        "provider": "standard declaration catalog; immutable build identity recorded with executable",
        "expectations": {
            "B01.protocol": {"positionEncoding": "utf-16", "openClose": True, "change": 1,
                             "serverName": "kagari-analyzer", "shutdown": None},
            "semantic_sites": {"body_literal": 1, "default_value": 7,
                               "counter_member": "current", "trait": "api::Metric",
                               "incomplete_edit": "counter."},
        },
        "schedule": {"startup": ["spawn", "initialize", "initialized", "shutdown", "exit"],
                     "churn_edits": edits, "retention_checkpoints": [100, 500, 1000] if edits else []},
    }


def frozen(shape: str) -> dict:
    value = manifest(shape)
    frozen_values = json.loads((FIXTURES / "manifest.json").read_text(encoding="utf-8"))
    expected = frozen_values["workloads"][shape]
    if object_digest(value) != expected["manifest_sha256"]:
        raise ValueError(f"{shape} inputs changed; review/update the frozen workload manifest deliberately")
    return value


def materialize(shape: str, directory: Path) -> dict:
    value = frozen(shape)
    for name, data in source_files(shape).items():
        path = directory / name
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("xb") as stream:
            stream.write(data)
    return value
