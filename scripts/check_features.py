"""Exercise SDK features in a standalone consumer, outside workspace dev unification.

Run with `uv run python scripts/check_features.py`. Generated manifests and logs
live under target/; Cargo uses the repository's normal target directory and O1.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "target" / "architecture-features"


def check_crate_boundaries(output: Path = OUTPUT) -> None:
    source = {"kagari-source", "kagari-hir", "kagari-syntax"}
    compiling = {"kagari-mir", "kagari-compiler", "kagari-codegen", "kagari-codegen-cranelift"}
    execution = {"kagari-runtime", "kagari-vm", "kagari-embed", "kagari-bytecode"}
    constraints = {
        "common": source | compiling | execution | {"kagari-types", "kagari-abi", "kagari-contract"},
        "types": source | compiling | execution | {"kagari-abi", "kagari-contract"},
        "abi": source | compiling | execution | {"kagari-contract", "kagari-common", "kagari-types"},
        "hir": compiling | execution | {"kagari-abi", "kagari-contract", "kagari-stdlib"},
        "contract": source | compiling | execution,
        "mir": source | execution | {"kagari-compiler", "kagari-codegen", "kagari-codegen-cranelift"},
        "bytecode": source | compiling,
        "runtime": source | compiling,
        "vm": source | compiling,
        "compiler": source | {"kagari-runtime", "kagari-vm", "kagari-embed", "kagari-codegen-cranelift"},
        "codegen": source | execution | {"kagari-compiler", "kagari-codegen-cranelift"},
        "codegen-cranelift": source | execution | {"kagari-compiler"},
    }
    for forbidden in constraints.values():
        forbidden.add("kagari-stdlib")
    constraints["stdlib"] = source | compiling | {"kagari-vm", "kagari-embed"}
    for crate, forbidden in constraints.items():
        graph = subprocess.check_output([
            "cargo", "tree", "--locked", "--offline", "-p", f"kagari-{crate}",
            "--no-default-features", "--edges", "normal", "--prefix", "none",
        ], cwd=ROOT, text=True)
        (output / f"{crate}-production-graph.log").write_text(graph)
        packages = {line.split()[0] for line in graph.splitlines() if line}
        assert not packages & forbidden, (crate, packages & forbidden)
    for crate in ["abi", "contract"]:
        graph = subprocess.check_output([
            "cargo", "tree", "--locked", "--offline", "-p", f"kagari-{crate}",
            "--no-default-features", "--edges", "normal,build", "--prefix", "none",
        ], cwd=ROOT, text=True)
        (output / f"{crate}-build-graph.log").write_text(graph)
        packages = {line.split()[0] for line in graph.splitlines() if line}
        assert not packages & constraints[crate], (f"{crate} build", packages & constraints[crate])
    print(f"{len(constraints)} production crate boundaries pass", flush=True)
    print("ABI/contract build graphs are independent of source analysis and execution", flush=True)


def run(async_only: bool = False) -> None:
    output = OUTPUT
    output.mkdir(parents=True, exist_ok=True)
    check_crate_boundaries(output)
    lines = [
        '[workspace]', '[package]', 'name = "kagari-feature-consumer"',
        'version = "0.0.0"', 'edition = "2024"', '[features]', 'default = []',
        'source = ["kagari-embed/source", "dep:kagari-source"]',
        'native = ["kagari-embed/native", "dep:kagari-codegen", "dep:kagari-mir", "dep:kagari-codegen-cranelift"]',
        '[dependencies]',
    ]
    for name in ["source", "abi", "contract", "types", "stdlib", "bytecode", "common", "runtime", "vm", "embed", "codegen", "mir", "codegen-cranelift"]:
        options = [f'path = {json.dumps(str(ROOT / "crates" / f"kagari-{name}"))}']
        if name == "embed":
            options.append('default-features = false')
        if name in {"source", "codegen", "mir", "codegen-cranelift"}:
            options.append('optional = true')
        lines.append(f'kagari-{name} = {{ {", ".join(options)} }}')
    targets = ["artifact_features", "async_execution"]
    for target in targets:
        lines += ['[[test]]', f'name = "{target}"',
                  f'path = {json.dumps(str(ROOT / "crates/kagari-embed/tests" / f"{target}.rs"))}']
    lines += ['[profile.dev]', 'opt-level = 1']
    manifest = output / "Cargo.toml"
    manifest.write_text("\n".join(lines) + "\n")
    # Keep the repository's external dependency resolutions. Cargo may prune unused
    # packages and add the local consumer, but must not upgrade external packages.
    (output / "Cargo.lock").write_bytes((ROOT / "Cargo.lock").read_bytes())
    locked = tomllib.loads((ROOT / "Cargo.lock").read_text())["package"]
    external = {(item["name"], item["version"]) for item in locked if "source" in item}
    if not async_only:
        subprocess.run([
            "cargo", "run", "--locked", "--offline", "-p", "kagari-embed",
            "--no-default-features", "--features", "source",
            "--example", "regenerate_feature_artifact",
        ], cwd=ROOT, check=True)
    subprocess.run([
        "cargo", "run", "--locked", "--offline", "-p", "kagari-embed",
        "--no-default-features", "--features", "source", "--example", "async_tasks",
        "--", str(ROOT / "target/fixtures/async_tasks.kbc"),
    ], cwd=ROOT, check=True)
    for features in ([""] if async_only else ["", "source", "native", "source,native"]):
        label = features.replace(",", "-") or "artifact-only"
        args = ["--manifest-path", str(manifest), "--no-default-features"]
        if features:
            args += ["--features", features]
        graph = subprocess.check_output(
            ["cargo", "tree", "--offline", *args, "--edges", "normal", "--prefix", "none"], cwd=ROOT, text=True,
        )
        resolved = tomllib.loads((output / "Cargo.lock").read_text())["package"]
        actual = {(item["name"], item["version"]) for item in resolved if "source" in item}
        assert actual <= external, (label, actual - external)
        (output / f"{label}-graph.log").write_text(graph)
        packages = {line.split()[0] for line in graph.splitlines() if line}
        forbidden = set()
        if "source" not in features:
            forbidden |= {"kagari-source", "kagari-hir", "kagari-syntax"}
        if not features:
            forbidden |= {"kagari-compiler", "kagari-mir", "kagari-codegen", "kagari-codegen-cranelift"}
        if features == "source":
            forbidden |= {"kagari-codegen", "kagari-codegen-cranelift"}
        assert not packages & forbidden, (label, packages & forbidden)
        if "source" in features:
            assert {"kagari-source", "kagari-hir", "kagari-syntax"} <= packages, label
        selected = ["--test", "artifact_features", "source_free_async_execution_contract"] if async_only else []
        log_label = f"{label}-async" if async_only else label
        with (output / f"{log_label}-tests.log").open("w") as log:
            subprocess.run(
                ["cargo", "test", "--locked", "--offline", *args, "--target-dir", str(ROOT / "target"), *selected],
                cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True,
            )
        print(f"{log_label}: production graph and standalone artifact tests pass", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--async-only", action="store_true",
                        help="run only the source-free async consumer contract, not the CI feature matrix")
    run(parser.parse_args().async_only)
