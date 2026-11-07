"""B07 separate allocation/source retention and externally sampled resident memory.

uv run python scripts/lsp_benchmark_memory.py --output target/lsp-benchmark/MEMORY
The benchmark pauses at owned checkpoints. Pauses and IO are not latency evidence.
"""
from __future__ import annotations

import argparse
import ctypes
import json
import os
from pathlib import Path
import platform
import queue
import subprocess
import sys
import threading
import time

from benchmark_lsp import command, freeze, save, source_state
from lsp_benchmark_inputs import ROOT, digest
from lsp_benchmark_results import require


def resident_memory(pid):
    if sys.platform == "win32":
        from ctypes import wintypes

        class Counters(ctypes.Structure):
            _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [
                (name, ctypes.c_size_t) for name in ("PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage",
                "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage", "PagefileUsage",
                "PeakPagefileUsage", "PrivateUsage")]

        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        psapi = ctypes.WinDLL("psapi", use_last_error=True)
        kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        kernel.OpenProcess.restype = wintypes.HANDLE
        kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        psapi.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(Counters), wintypes.DWORD]
        handle = kernel.OpenProcess(0x0400 | 0x0010, False, pid)
        if not handle:
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            counters = Counters()
            counters.cb = ctypes.sizeof(counters)
            if not psapi.GetProcessMemoryInfo(handle, ctypes.byref(counters), counters.cb):
                raise ctypes.WinError(ctypes.get_last_error())
            return {"rss_bytes": counters.WorkingSetSize, "os_peak_rss_bytes": counters.PeakWorkingSetSize,
                    "private_commit_bytes": counters.PrivateUsage, "api": "GetProcessMemoryInfo/PROCESS_MEMORY_COUNTERS_EX"}
        finally:
            kernel.CloseHandle(handle)
    if sys.platform.startswith("linux"):
        fields = {}
        for line in Path(f"/proc/{pid}/status").read_text().splitlines():
            key, _, value = line.partition(":")
            if key in ("VmRSS", "VmHWM"):
                fields[key] = int(value.strip().split()[0]) * 1024
        require(set(fields) == {"VmRSS", "VmHWM"}, "missing /proc resident-memory counters")
        return {"rss_bytes": fields["VmRSS"], "os_peak_rss_bytes": fields["VmHWM"], "api": "/proc/PID/status"}
    raise ValueError("resident memory sampling currently supports Windows and Linux")


def build():
    started = time.perf_counter_ns()
    output = command(["cargo", "build", "-p", "kagari-analyzer", "--bench", "lsp_memory", "--features", "verification",
                      "--profile", "release", "--message-format=json"], timeout=1200)
    paths = [Path(item["executable"]).resolve() for line in output.splitlines() if (item := json.loads(line)).get("reason") ==
             "compiler-artifact" and item.get("target", {}).get("name") == "lsp_memory" and item.get("executable")]
    require(len(paths) == 1, "Cargo did not identify exactly one memory benchmark artifact")
    return paths[0], time.perf_counter_ns() - started


def run_child(executable):
    child = subprocess.Popen([str(executable), "--external-checkpoints"], cwd=ROOT, stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    lines = queue.Queue(16)
    stderr = bytearray()

    def read():
        try:
            while line := child.stdout.readline(65537):
                require(len(line) <= 65536 and line.endswith(b"\n"), "oversized or truncated memory probe output")
                lines.put(line.decode("utf-8"), timeout=10)
            lines.put(None, timeout=10)
        except BaseException as error:
            lines.put(error, timeout=10)

    def drain():
        while data := child.stderr.read(4096):
            stderr.extend(data[:max(0, 65536 - len(stderr))])

    readers = [threading.Thread(target=read, daemon=True), threading.Thread(target=drain, daemon=True)]
    for reader in readers:
        reader.start()
    checkpoints = []
    result = None
    primed = False
    try:
        while True:
            line = lines.get(timeout=60)
            if isinstance(line, BaseException):
                raise line
            if line is None:
                break
            if line.strip() == "ready":
                require(not primed, "duplicate memory probe priming")
                primed = True
                child.stdin.write(b"\n")
                child.stdin.flush()
            elif line.startswith("checkpoint "):
                require(primed and result is None, "memory checkpoint outside probe window")
                _, phase, edits = line.split()
                checkpoints.append({"phase": phase, "edits": int(edits), "memory": resident_memory(child.pid)})
                child.stdin.write(b"\n")
                child.stdin.flush()
            else:
                require(result is None, "duplicate memory probe JSON report")
                result = json.loads(line)
        code = child.wait(timeout=10)
        require(code == 0 and result is not None, f"memory probe failed: code={code}; {stderr.decode('utf-8', 'replace')}")
        rows = result.get("checkpoints", [])
        require(len(rows) == len(checkpoints) == 6, "missing memory checkpoints")
        for owned, observed in zip(rows, checkpoints):
            require((owned["phase"], owned["edits"]) == (observed["phase"], observed["edits"]), "memory checkpoint order mismatch")
            owned["resident"] = observed["memory"]
        return result
    finally:
        if child.poll() is None:
            child.kill()
            child.wait(timeout=10)
        child.stdin.close()
        for reader in readers:
            reader.join(timeout=5)
        child.stdout.close()
        child.stderr.close()
        require(not any(reader.is_alive() for reader in readers), "memory sampler reader did not retire")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate", type=Path, help="explicit fresh release/verification lsp_memory artifact")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--power-mode", default="unreported")
    args = parser.parse_args()
    try:
        output = args.output.resolve()
        require(output.is_relative_to((ROOT / "target").resolve()) and not output.exists(), "output must be a new target/ directory")
        output.mkdir(parents=True)
        before = source_state()
        artifact, build_ns = (args.candidate.resolve(), None) if args.candidate else build()
        require(artifact.is_file(), "memory probe artifact is absent")
        require(source_state() == before, "source changed during memory probe build")
        image = freeze(artifact, output, "lsp_memory")
        metadata = {"rustc": command(["rustc", "-Vv"]), "cargo": command(["cargo", "-V"]),
            "platform": platform.platform(), "cpu": platform.processor(), "logical_cpus": os.cpu_count(),
            "profile": "release", "features": ["verification"], "power_mode": args.power_mode, "build_ns": build_ns,
            "parallelism": "one benchmark child plus an external sampler; Cargo default build parallelism",
            "cache_state": "fresh process; native baseline prepared before allocation peak window; OS caches uncontrolled",
            "source": before, "lock_sha256": digest((ROOT / "Cargo.lock").read_bytes()),
            "harness_sha256": digest(Path(__file__).read_bytes()), "sample_groups": 1,
            "scope": "requested/live System allocation layouts, exact source-content leases and compact index rows; externally sampled RSS/OS peak/private commit are separate; no speedup or paired regression claim"}
        result = run_child(output / image["path"])
        require(source_state() == before, "source changed during memory probe campaign")
        save(output / "report.json", {"schema": 1, "metadata": metadata, "image": image, "probe": result})
        print(json.dumps({"output": str(output), "checkpoints": result["checkpoints"]}, indent=2))
    except (ValueError, OSError, queue.Empty, subprocess.SubprocessError) as error:
        print(f"LSP memory benchmark failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
