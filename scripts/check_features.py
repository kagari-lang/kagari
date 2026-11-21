"""Exercise SDK features in a standalone consumer, outside workspace dev unification.

Run with `uv run python scripts/check_features.py`. Generated manifests and logs
live under target/; Cargo uses the repository's normal target directory and O1.
Use --numeric-only for one source-free numeric workload without the feature matrix.
The full CI invocation also runs this bounded numeric acceptance once.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "target" / "architecture-features"


def check_crate_boundaries(output: Path = OUTPUT) -> None:
    source = {"kagari-source", "kagari-hir", "kagari-syntax", "kagari-package"}
    compiling = {"kagari-mir", "kagari-compiler", "kagari-codegen", "kagari-codegen-cranelift"}
    execution = {"kagari-runtime", "kagari", "kagari-bytecode"}
    constraints = {
        "common": source | compiling | execution | {"kagari-types", "kagari-abi", "kagari-contract"},
        "types": source | compiling | execution | {"kagari-abi", "kagari-contract"},
        "abi": source | compiling | execution | {"kagari-contract", "kagari-common", "kagari-types"},
        "hir": compiling | execution | {"kagari-abi", "kagari-contract", "kagari-stdlib"},
        "contract": source | compiling | execution,
        "mir": source | execution | {"kagari-compiler", "kagari-codegen", "kagari-codegen-cranelift"},
        "bytecode": source | compiling,
        "runtime": source | compiling,
        "compiler": source | {"kagari-runtime", "kagari", "kagari-codegen-cranelift"},
        "codegen": source | execution | {"kagari-compiler", "kagari-codegen-cranelift"},
        "codegen-cranelift": source | execution | {"kagari-compiler"},
    }
    tools = {"kagari-format", "kagari-lint"}
    for forbidden in constraints.values():
        forbidden.update(tools | {"kagari-stdlib", "kagari-vm"})
    constraints["stdlib"] = source | compiling | {"kagari"}
    constraints["stdlib"].update(tools)
    constraints["format"] = (source - {"kagari-source", "kagari-syntax"}) | compiling | execution | {"kagari-lint", "kagari-stdlib"}
    constraints["lint"] = compiling | execution | {"kagari-format", "kagari-stdlib"}
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
    host = next(line.removeprefix("host: ") for line in subprocess.check_output(
        ["rustc", "-vV"], cwd=ROOT, text=True,
    ).splitlines() if line.startswith("host: "))
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--locked", "--offline", "--format-version", "1",
        "--filter-platform", host,
    ], cwd=ROOT, text=True))
    packages = {item["id"]: item for item in metadata["packages"]}
    nodes = {item["id"]: item for item in metadata["resolve"]["nodes"]}
    runtime = next(key for key, item in packages.items() if item["name"] == "kagari-runtime")
    assert all(dependency["name"] != "kagari-stdlib" for dependency in packages[runtime]["dependencies"])
    # Check the host test graph, including the root's dev dependencies, but not
    # their own dev dependencies. Other target/backend matrices remain CI scope.
    pending = [runtime]
    seen = set()
    while pending:
        package = pending.pop()
        if package in seen:
            continue
        seen.add(package)
        assert packages[package]["name"] != "kagari-stdlib", "Runtime test/build graph reaches stdlib"
        for dependency in nodes[package]["deps"]:
            if package == runtime or any(kind["kind"] != "dev" for kind in dependency["dep_kinds"]):
                pending.append(dependency["pkg"])
    (output / "runtime-test-graph.json").write_text(json.dumps(sorted(
        packages[package]["name"] for package in seen
    ), indent=2))
    print("Runtime production, build and test dependencies exclude stdlib", flush=True)
    print(f"{len(constraints)} production crate boundaries pass", flush=True)
    print("ABI/contract build graphs are independent of source analysis and execution", flush=True)


def run(async_only: bool = False, numeric_only: bool = False, conversions_only: bool = False) -> None:
    output = OUTPUT / "numeric" if numeric_only else OUTPUT
    output.mkdir(parents=True, exist_ok=True)
    if not numeric_only and not conversions_only:
        check_crate_boundaries(output)
    lines = [
        '[workspace]', '[package]', 'name = "kagari-feature-consumer"',
        'version = "0.0.0"', 'edition = "2024"', '[features]', 'default = []',
    ]
    if not numeric_only:
        lines += [
            'source = ["kagari/source", "dep:kagari-source"]',
            'native = ["kagari/native", "dep:kagari-codegen", "dep:kagari-mir", "dep:kagari-codegen-cranelift"]',
        ]
    lines.append('[dependencies]')
    dependencies = ["kagari", "kagari-runtime"] if numeric_only else [
        "kagari", "kagari-source", "kagari-abi", "kagari-contract", "kagari-types",
        "kagari-stdlib", "kagari-bytecode", "kagari-common", "kagari-runtime", "kagari-gc",
        "kagari-codegen", "kagari-mir", "kagari-codegen-cranelift",
    ]
    for name in dependencies:
        options = [f'path = {json.dumps(str(ROOT / "crates" / name))}']
        if name == "kagari":
            options.append('default-features = false')
        if name in {"kagari-source", "kagari-codegen", "kagari-mir", "kagari-codegen-cranelift"}:
            options.append('optional = true')
        lines.append(f'{name} = {{ {", ".join(options)} }}')
    if not numeric_only:
        lines += ['[dev-dependencies]', 'tokio = { version = "1", features = ["rt"] }']
    targets = {"numeric_artifact": "artifact_features/numeric.rs"} if numeric_only else {
        "artifact_features": "artifact_features.rs", "async_execution": "async_execution.rs",
    }
    for target, path in targets.items():
        lines += ['[[test]]', f'name = "{target}"',
                  f'path = {json.dumps(str(ROOT / "crates/kagari/tests" / path))}']
    lines += ['[profile.dev]', 'opt-level = 1']
    manifest = output / "Cargo.toml"
    manifest.write_text("\n".join(lines) + "\n")
    # Keep the repository's external dependency resolutions. Cargo may prune unused
    # packages and add the local consumer, but must not upgrade external packages.
    (output / "Cargo.lock").write_bytes((ROOT / "Cargo.lock").read_bytes())
    locked = tomllib.loads((ROOT / "Cargo.lock").read_text())["package"]
    external = {(item["name"], item["version"]) for item in locked if "source" in item}
    def emit_fixture(source: Path, name: str) -> Path:
        # Never accept a stale product after a failed producer invocation.
        artifact = ROOT / "target/fixtures" / name
        artifact.unlink(missing_ok=True)
        subprocess.run([
            "cargo", "run", "--locked", "--offline", "-p", "kagari",
            "--no-default-features", "--features", "source",
            "--example", "regenerate_feature_artifact", "--",
            str(source), str(artifact),
        ], cwd=ROOT, check=True)
        return artifact

    test_env = os.environ.copy()
    if numeric_only:
        test_env["KAGARI_NUMERIC_ARTIFACT"] = str(emit_fixture(
            ROOT / "examples/numeric-sampling.kgr", "numeric_sampling.kbc"))
    elif conversions_only:
        emit_fixture(ROOT / "crates/kagari/tests/fixtures/language/conversion_traits"
                     / "reverse-conversions-call-the-destination-implementation.kgr", "reverse_conversions.kbc")
    elif not async_only:
        subprocess.run([
            "cargo", "run", "--locked", "--offline", "-p", "kagari",
            "--no-default-features", "--features", "source",
            "--example", "regenerate_feature_artifact",
        ], cwd=ROOT, check=True)
        emit_fixture(ROOT / "crates/kagari/tests/fixtures/async_pairs.kgr", "async_pairs.kbc")
    if not numeric_only and not conversions_only:
        subprocess.run([
            "cargo", "run", "--locked", "--offline", "-p", "kagari",
            "--no-default-features", "--features", "source", "--example", "async_tasks",
            "--", str(ROOT / "target/fixtures/async_tasks.kbc"),
        ], cwd=ROOT, check=True)
    focused = async_only or numeric_only or conversions_only
    edges = "normal,build,dev" if conversions_only else "normal,build" if numeric_only else "normal"
    for features in ([""] if focused else ["", "source", "native", "source,native"]):
        label = features.replace(",", "-") or "artifact-only"
        args = ["--manifest-path", str(manifest), "--no-default-features"]
        if features:
            args += ["--features", features]
        graph = subprocess.check_output(
            ["cargo", "tree", "--offline", *args, "--edges",
                         edges, "--prefix", "none"], cwd=ROOT, text=True,
        )
        resolved = tomllib.loads((output / "Cargo.lock").read_text())["package"]
        actual = {(item["name"], item["version"]) for item in resolved if "source" in item}
        assert actual <= external, (label, actual - external)
        (output / f"{label}-graph.log").write_text(graph)
        packages = {line.split()[0] for line in graph.splitlines() if line}
        forbidden = {"kagari-format", "kagari-lint"}
        if "source" not in features:
            forbidden |= {"kagari-source", "kagari-hir", "kagari-syntax", "kagari-package"}
        if not features:
            forbidden |= {"kagari-compiler", "kagari-mir", "kagari-codegen", "kagari-codegen-cranelift"}
        if features == "source":
            forbidden |= {"kagari-codegen", "kagari-codegen-cranelift"}
        assert not packages & forbidden, (label, packages & forbidden)
        if "source" in features:
            assert {"kagari-source", "kagari-hir", "kagari-syntax", "kagari-package"} <= packages, label
        log_label = label
        if numeric_only:
            log_label += "-numeric"
            selected = ["--test", "numeric_artifact"]
        elif conversions_only:
            log_label += "-conversions"
            selected = ["--test", "artifact_features", "reverse_conversions::"]
        elif async_only:
            log_label += "-async"
            selected = ["--test", "artifact_features", "source_free_async_execution_contract"]
        else:
            selected = []

        log_path = output / f"{log_label}-tests.log"
        try:
            with log_path.open("w") as log:
                subprocess.run(
                    ["cargo", "test", "--locked", "--offline", *args, "--target-dir", str(ROOT / "target"), *selected],
                    cwd=ROOT, env=test_env, stdout=log, stderr=subprocess.STDOUT, check=True,
                )
        except subprocess.CalledProcessError:
            print(log_path.read_text(), file=sys.stderr, flush=True)
            raise
        print(f"{log_label}: production graph and standalone artifact tests pass", flush=True)
    if not focused:
        run(numeric_only=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--async-only", action="store_true",
                      help="run only the source-free async consumer contract, not the CI feature matrix")
    mode.add_argument("--numeric-only", action="store_true",
                      help="emit and execute only the numeric example in a source-free consumer")
    mode.add_argument("--conversions-only", action="store_true",
                      help="emit and verify the reverse conversion fixture in the frontend-free consumer")
    args = parser.parse_args()
    run(async_only=args.async_only, numeric_only=args.numeric_only, conversions_only=args.conversions_only)
