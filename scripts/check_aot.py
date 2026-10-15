"""Windows deployment proof: separate producer and compiler-free consumer processes."""
from pathlib import Path
import json
import os
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent


def run(*args: str, **kwargs: object) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(args, cwd=ROOT, check=True, text=True, **kwargs)
    except subprocess.CalledProcessError as error:
        if error.stdout:
            print(error.stdout, file=sys.stderr)
        if error.stderr:
            print(error.stderr, file=sys.stderr)
        raise


def main() -> None:
    if sys.platform != "win32":
        raise SystemExit("AOT deployment acceptance currently requires Windows x64 MSVC")
    run("cargo", "build", "--locked", "-p", "kagari-cli", "--bin", "kg", "--features", "aot")
    output = ROOT / "target" / "aot-consumer"
    output.mkdir(parents=True, exist_ok=True)
    manifest = ["[workspace]", "[package]", 'name = "kagari-aot-consumer"',
                'version = "0.0.0"', 'edition = "2024"', "[dependencies]"]
    for name in ["kagari", "kagari-runtime", "kagari-types", "kagari-vm"]:
        options = f'path = {json.dumps(str(ROOT / "crates" / name))}'
        if name == "kagari":
            options += ', default-features = false, features = ["aot-load"]'
        manifest.append(f'{name} = {{ {options} }}')
    manifest += ['[[bin]]', 'name = "kagari-aot-consumer"',
                 f'path = {json.dumps(str(ROOT / "crates/kagari/examples/aot_host.rs"))}',
                 "[profile.dev]", "opt-level = 1"]
    (output / "Cargo.toml").write_text("\n".join(manifest) + "\n", encoding="utf-8")
    (output / "Cargo.lock").write_bytes((ROOT / "Cargo.lock").read_bytes())
    args = ["--manifest-path", str(output / "Cargo.toml")]
    graph = run("cargo", "tree", "--offline", *args, "--edges", "normal,build", "--prefix", "none", capture_output=True).stdout
    names = {line.split()[0] for line in graph.splitlines() if line}
    forbidden = {"kagari-source", "kagari-syntax", "kagari-hir", "kagari-mir", "kagari-compiler", "kagari-codegen", "kagari-codegen-cranelift"}
    assert not names & forbidden, names & forbidden
    assert not any(name.startswith("cranelift-") for name in names), names
    external = lambda path: {(p["name"], p["version"]) for p in tomllib.loads(path.read_text())["package"] if "source" in p}
    assert external(output / "Cargo.lock") <= external(ROOT / "Cargo.lock")
    (output / "dependency-graph.log").write_text(graph, encoding="utf-8")
    # Share the repository's normal target/profile and default build parallelism.
    run("cargo", "build", "--locked", "--offline", *args, "--target-dir", str(ROOT / "target"))
    with tempfile.TemporaryDirectory(prefix="aot-deployment-", dir=ROOT / "target") as temporary:
        work = Path(temporary)
        source = work / "answer.kgr"
        source.write_text("fn main() -> i32 { 6 * 7 }", encoding="utf-8")
        bundle = work / "bundle"
        run(str(ROOT / "target/debug/kg.exe"), "build-aot", str(source), "-o", str(bundle))
        source.unlink()
        # The launched host searches no build tool directory; its graph contains
        # neither compiler libraries nor the source frontend. Only OS tools remain.
        environment = os.environ.copy()
        environment["PATH"] = str(Path(os.environ["SystemRoot"]) / "System32")
        result = run(str(ROOT / "target/debug/kagari-aot-consumer.exe"), str(bundle), env=environment, capture_output=True)
        assert "Native: 42 (compiler-free host)" in result.stdout, result.stdout
        print(result.stdout.strip())
    print("AOT producer/consumer separation and dependency boundaries passed")


if __name__ == "__main__":
    main()
