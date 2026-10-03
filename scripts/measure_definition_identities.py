"""Measure the same identity workload before and after the scoped-ID migration.

Run with `uv run python scripts/measure_definition_identities.py`.
Builds and all execution processes run sequentially. Output stays under target/.
"""
from __future__ import annotations

import hashlib
import json
import platform
from pathlib import Path
import shutil
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "target" / "identity-measurements"
BASELINE = ROOT / "target" / "identity-baseline"
BASELINE_REVISION = "7857fd8a"
EXAMPLE = Path("crates/kagari-embed/examples/definition_pipeline.rs")


def command(args: list[str], cwd: Path = ROOT) -> str:
    return subprocess.check_output(args, cwd=cwd, text=True, encoding="utf-8").strip()


def build(cwd: Path, label: str, package: str, example: str) -> float:
    start = time.perf_counter()
    with (OUTPUT / f"{label}-build.log").open("w", encoding="utf-8") as log:
        subprocess.run(
            ["cargo", "build", "--release", "--locked", "-p", package, "--example", example],
            cwd=cwd, stdout=log, stderr=subprocess.STDOUT, check=True,
        )
    seconds = time.perf_counter() - start
    print(f"{label}: build finished in {seconds:.2f}s (excluded from execution)", flush=True)
    return seconds


def run() -> None:
    OUTPUT.mkdir(parents=True, exist_ok=True)
    if not BASELINE.exists():
        subprocess.run(["git", "worktree", "add", "--detach", str(BASELINE), BASELINE_REVISION], cwd=ROOT, check=True)
    expected = command(["git", "rev-parse", BASELINE_REVISION])
    assert command(["git", "rev-parse", "HEAD"], BASELINE) == expected
    # The baseline gets exactly the same measurement source, without its production migration.
    shutil.copyfile(ROOT / EXAMPLE, BASELINE / EXAMPLE)
    metadata = {
        "baseline": expected,
        "candidate": command(["git", "rev-parse", "HEAD"]),
        "probe_sha256": hashlib.sha256((ROOT / EXAMPLE).read_bytes()).hexdigest(),
        "toolchain": command(["rustc", "-Vv"]),
        "os": platform.platform(),
        "profile": "workspace release, default Cargo parallelism and target directories",
        "features": "SDK default source/native",
        "cache": "build cache reported separately; execution warmed once per operation",
        "timing": "7 samples per process, destruction outside timed interval; inactive counting allocator remains installed",
        "allocations": "separate warmed pass: allocation/reallocation calls, gross requested bytes, live/peak deltas; excludes allocator bookkeeping",
        "workload": "32 nominal Player functions, generic identity and checked main returning 31",
        "order": ["baseline", "candidate", "candidate", "baseline"],
    }
    if platform.system() == "Windows":
        metadata["cpu"] = json.loads(command(["powershell", "-NoProfile", "-Command", "Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors | ConvertTo-Json -Compress"]))
    suffix = ".exe" if platform.system() == "Windows" else ""
    metadata["build_seconds"] = {
        "baseline": build(BASELINE, "baseline", "kagari-embed", "definition_pipeline"),
        "candidate": build(ROOT, "candidate", "kagari-embed", "definition_pipeline"),
        "table_probe": build(ROOT, "table-probe", "kagari-common", "definition_identity"),
    }
    for index, label in enumerate(metadata["order"], 1):
        cwd = BASELINE if label == "baseline" else ROOT
        binary = cwd / "target" / "release" / "examples" / f"definition_pipeline{suffix}"
        start = time.perf_counter()
        result = command([str(binary)], cwd)
        (OUTPUT / f"{index}-{label}.log").write_text(result + "\n", encoding="utf-8")
        print(f"{index}-{label}: checked probe finished in {time.perf_counter() - start:.2f}s", flush=True)
    table_binary = ROOT / "target" / "release" / "examples" / f"definition_identity{suffix}"
    (OUTPUT / "table-probe.log").write_text(command([str(table_binary)]) + "\n", encoding="utf-8")
    (OUTPUT / "metadata.json").write_text(json.dumps(metadata, indent=2), encoding="utf-8")


if __name__ == "__main__":
    run()
