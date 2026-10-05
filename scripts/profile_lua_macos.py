"""Sample warmed interpreter execution on macOS, outside throughput measurements."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import threading
import time

from benchmark_lua import ROOT, command, machine

WORKLOADS = ("arithmetic", "branches", "calls", "fibonacci", "arrays", "maps")


def sample(name: str, executable: Path, output: Path) -> None:
    directory = output / name
    directory.mkdir()
    with (directory / "stderr.log").open("w") as errors, \
            (directory / "execution.log").open("w") as execution:
        process = subprocess.Popen(
            [str(executable), "--interpreter-only", f"--profile={name}"],
            cwd=ROOT, stdout=subprocess.PIPE, stderr=errors, text=True, bufsize=1,
        )
        lines: list[str] = []
        finished = threading.Event()
        reader = None
        try:
            for line in process.stdout:
                execution.write(line)
                lines.append(line)
                if line.startswith("PROFILE_READY,"):
                    break
            else:
                raise RuntimeError(f"{name}: missing sampling window")

            def drain() -> None:
                for line in process.stdout:
                    execution.write(line)
                    lines.append(line)
                    if line.startswith("PROFILE_DONE,"):
                        finished.set()

            reader = threading.Thread(target=drain)
            reader.start()
            with (directory / "sampler.log").open("w") as log:
                subprocess.run(
                    ["/usr/bin/sample", str(process.pid), "5", "1", "-file",
                     str(directory / "sample.txt")],
                    stdout=log, stderr=subprocess.STDOUT, check=True, timeout=30,
                )
            overlapped_counting = finished.is_set()
            code = process.wait(timeout=60)
            reader.join()
            if code or not finished.is_set():
                raise RuntimeError(f"{name}: incomplete profile (exit {code})")
            if overlapped_counting:
                raise RuntimeError(f"{name}: sampler may overlap the instruction-counting pass")
            if not any(line.startswith("INSTRUCTION_TOTAL,") for line in lines):
                raise RuntimeError(f"{name}: missing instruction counts")
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()
            if reader is not None:
                reader.join()
            process.stdout.close()
    print(f"{name}: sampled execution and checked instruction counts", flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("workloads", nargs="*", default=list(WORKLOADS))
    args = parser.parse_args()
    if platform.system() != "Darwin":
        parser.error("requires macOS /usr/bin/sample")
    if any(name not in WORKLOADS for name in args.workloads):
        parser.error(f"workloads must be selected from {WORKLOADS}")
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    output = ROOT / "target/lua-comparison" / f"{stamp}-macos-profile"
    output.mkdir(parents=True)
    print(f"Output: {output}", flush=True)
    start = time.perf_counter()
    with (output / "build.log").open("w") as log:
        subprocess.run(
            ["cargo", "build", "--release", "--locked", "-p", "kagari-lua-benchmark"],
            cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True,
        )
    build_seconds = time.perf_counter() - start
    executable = ROOT / "target/release/kagari-lua-benchmark"
    metadata = {
        "timestamp_utc": stamp, "revision": command("git", "rev-parse", "HEAD"),
        "git_status": command("git", "status", "--short"), "machine": machine(),
        "rustc": command("rustc", "-Vv"), "cargo": command("cargo", "-V"),
        "build_wall_seconds": build_seconds,
        "profile": "workspace release; default parallelism and target; no debug override",
        "features": "kagari-embed source,native; only interpreter executed",
        "sampling": "sample PID 5 1; all threads; three warmups; ten-second execution window",
        "limitations": "wall-clock stack samples, not CPU counters; optimized inline attribution is incomplete",
        "binary_sha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
        "driver_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "workloads": args.workloads,
        "environment": {key: os.environ[key] for key in (
            "CARGO_BUILD_JOBS", "CARGO_TARGET_DIR", "RUSTFLAGS", "CFLAGS", "CC"
        ) if key in os.environ},
        "lock_sha256": hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest(),
        "source_sha256": {
            path.relative_to(ROOT).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted((ROOT / "benchmarks/lua-comparison").rglob("*"))
            if path.is_file() and path.suffix in {".rs", ".kgr", ".lua", ".toml"}
        },
    }
    (output / "metadata.json").write_text(json.dumps(metadata, indent=2))
    for name in args.workloads:
        sample(name, executable, output)


if __name__ == "__main__":
    main()
