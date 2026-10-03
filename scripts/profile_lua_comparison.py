"""Sample only the benchmark's main thread, using local PDBs and no ETW privileges."""
import argparse
from collections import Counter
import ctypes as c
from ctypes import wintypes as w
import json
from pathlib import Path
import random
import hashlib
import os
import platform
import shutil
import subprocess
import threading
import time

from benchmark_lua import machine

if os.name != "nt" or c.sizeof(c.c_void_p) != 8 or platform.machine().lower() not in {"amd64", "x86_64"}:
    raise SystemExit("This sampler requires 64-bit Python on x64 Windows.")

K = c.WinDLL("kernel32", use_last_error=True)
D = c.WinDLL("dbghelp", use_last_error=True)
K.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
K.OpenProcess.restype = w.HANDLE
K.OpenThread.argtypes = [w.DWORD, w.BOOL, w.DWORD]
K.OpenThread.restype = w.HANDLE
K.CreateToolhelp32Snapshot.argtypes = [w.DWORD, w.DWORD]
K.CreateToolhelp32Snapshot.restype = w.HANDLE
K.SuspendThread.argtypes = [w.HANDLE]
K.SuspendThread.restype = w.DWORD
K.ResumeThread.argtypes = [w.HANDLE]
K.ResumeThread.restype = w.DWORD
K.GetThreadContext.argtypes = [w.HANDLE, c.c_void_p]
K.GetThreadContext.restype = w.BOOL
K.CloseHandle.argtypes = [w.HANDLE]
K.QueryThreadCycleTime.argtypes = [w.HANDLE, c.POINTER(c.c_uint64)]
K.QueryThreadCycleTime.restype = w.BOOL
D.SymSetOptions.argtypes = [w.DWORD]
D.SymInitialize.argtypes = [w.HANDLE, c.c_char_p, w.BOOL]
D.SymInitialize.restype = w.BOOL
D.SymCleanup.argtypes = [w.HANDLE]

class ThreadEntry(c.Structure):
    _fields_ = [("size", w.DWORD), ("usage", w.DWORD), ("tid", w.DWORD),
                ("pid", w.DWORD), ("base_priority", w.LONG), ("delta", w.LONG),
                ("flags", w.DWORD)]

K.Thread32First.argtypes = [w.HANDLE, c.POINTER(ThreadEntry)]
K.Thread32Next.argtypes = [w.HANDLE, c.POINTER(ThreadEntry)]

class Symbol(c.Structure):
    _fields_ = [("SizeOfStruct", w.ULONG), ("TypeIndex", w.ULONG),
                ("Reserved", c.c_uint64 * 2), ("Index", w.ULONG), ("Size", w.ULONG),
                ("ModBase", c.c_uint64), ("Flags", w.ULONG), ("Value", c.c_uint64),
                ("Address", c.c_uint64), ("Register", w.ULONG), ("Scope", w.ULONG),
                ("Tag", w.ULONG), ("NameLen", w.ULONG), ("MaxNameLen", w.ULONG),
                ("Name", c.c_char * 1)]

D.SymFromAddr.argtypes = [w.HANDLE, c.c_uint64, c.POINTER(c.c_uint64), c.POINTER(Symbol)]
D.SymFromAddr.restype = w.BOOL

def threads(pid):
    snapshot = K.CreateToolhelp32Snapshot(4, 0)
    entry = ThreadEntry()
    entry.size = c.sizeof(entry)
    result = []
    try:
        valid = K.Thread32First(snapshot, c.byref(entry))
        while valid:
            if entry.pid == pid:
                result.append(entry.tid)
            valid = K.Thread32Next(snapshot, c.byref(entry))
    finally:
        K.CloseHandle(snapshot)
    return result

def symbol(handle, address):
    buffer = c.create_string_buffer(c.sizeof(Symbol) + 2048)
    value = c.cast(buffer, c.POINTER(Symbol))
    value.contents.SizeOfStruct = c.sizeof(Symbol)
    value.contents.MaxNameLen = 2048
    displacement = c.c_uint64()
    if D.SymFromAddr(handle, address, c.byref(displacement), value):
        name = c.string_at(c.addressof(buffer) + Symbol.Name.offset, value.contents.NameLen).decode("utf-8", "replace")
        return name, displacement.value
    return "unresolved", None

def sample(workload, exe):
    root = Path(__file__).resolve().parents[1]
    output = root / "target/lua-comparison/profile" / workload
    output.mkdir(parents=True, exist_ok=True)
    lines = []
    with (output / "stderr.log").open("w") as error_log:
        process = subprocess.Popen([str(exe), f"--profile={workload}"], stdout=subprocess.PIPE,
                                   stderr=error_log, text=True, bufsize=1)
        try:
            for line in process.stdout:
                lines.append(line.rstrip())
                if line.startswith("PROFILE_READY,"):
                    break
            else:
                raise RuntimeError("No sampling window")
            done = threading.Event()
            def read_output():
                for line in process.stdout:
                    lines.append(line.rstrip())
                    if line.startswith("PROFILE_DONE,"):
                        done.set()
                done.set()
            reader = threading.Thread(target=read_output, daemon=True)
            reader.start()
            tids = threads(process.pid)
            cycles = {}
            for tid in tids:
                candidate = K.OpenThread(0x40, False, tid)
                try:
                    value = c.c_uint64()
                    assert candidate and K.QueryThreadCycleTime(candidate, c.byref(value)), c.get_last_error()
                    cycles[tid] = value.value
                finally:
                    if candidate:
                        K.CloseHandle(candidate)
            target_tid = max(cycles, key=cycles.get)
            handle = K.OpenProcess(0x400 | 0x10, False, process.pid)
            thread = K.OpenThread(0x2 | 0x8 | 0x40, False, target_tid)
            initialized = False
            try:
                assert handle and thread, c.get_last_error()
                D.SymSetOptions(0x2 | 0x4 | 0x200 | 0x40000)
                assert D.SymInitialize(handle, str(exe.parent).encode(), True), c.get_last_error()
                initialized = True
                # AMD64 CONTEXT is 16-byte aligned; RIP is at offset 248 and
                # ContextFlags at 48. Extra buffer space covers the complete context.
                buffer = c.create_string_buffer(4096 + 16)
                context = (c.addressof(buffer) + 15) & ~15
                c.c_uint32.from_address(context + 48).value = 0x100003
                addresses = Counter()
                errors = 0
                randomizer = random.Random(0x4B4752)
                started = time.perf_counter()
                while not done.is_set() and process.poll() is None:
                    suspended = K.SuspendThread(thread)
                    if suspended == 0xFFFFFFFF:
                        errors += 1
                        break
                    try:
                        if K.GetThreadContext(thread, context):
                            addresses[c.c_uint64.from_address(context + 248).value] += 1
                        else:
                            errors += 1
                    finally:
                        assert K.ResumeThread(thread) != 0xFFFFFFFF, c.get_last_error()
                    time.sleep(randomizer.uniform(0.0015, 0.0025))
                duration = time.perf_counter() - started
                names = Counter()
                resolved = []
                for address, count in addresses.items():
                    name, displacement = symbol(handle, address)
                    names[name] += count
                    resolved.append({"pc": hex(address), "count": count, "symbol": name,
                                     "displacement": displacement})
                assert names and names["unresolved"] < sum(names.values()) / 2, "Symbols unavailable"
                report = {"workload": workload, "pid": process.pid, "thread": target_tid,
                          "initial_thread_cycles": cycles,
                          "sample_wall_seconds": duration, "samples": sum(names.values()),
                          "errors": errors, "interval_seconds": [0.0015, 0.0025],
                          "method": "jittered target main-thread RIP samples; brief suspend/context/resume; local release PDB symbols",
                          "symbols": names.most_common(), "addresses": resolved}
                (output / "samples.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
                for name, count in names.most_common(25):
                    print(f"{count / sum(names.values()) * 100:6.2f}% {count:5d} {name}")
            finally:
                if initialized:
                    D.SymCleanup(handle)
                if thread:
                    K.CloseHandle(thread)
                if handle:
                    K.CloseHandle(handle)
            process.wait(timeout=30)
            reader.join(timeout=5)
            assert process.returncode == 0, process.returncode
            (output / "execution.log").write_text("\n".join(lines) + "\n", encoding="utf-8")
            for line in lines:
                if line.startswith(("PROFILE_DONE", "PROFILE_LAYOUT", "INSTRUCTION", "LUA_INSTRUCTION")):
                    print(line)
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=10)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("workloads", nargs="+", choices=["entry", "arithmetic", "branches", "calls", "fibonacci", "arrays", "maps"])
    parser.add_argument("--no-build", action="store_true", help="Use an already saved optimized symbol build")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    output = root / "target/lua-comparison/profile"
    output.mkdir(parents=True, exist_ok=True)
    binary = output / "symbol-bin"
    binary.mkdir(exist_ok=True)
    if not args.no_build:
        environment = dict(os.environ, CARGO_PROFILE_RELEASE_DEBUG="2")
        started = time.perf_counter()
        with (output / "symbol-build.log").open("w") as log:
            subprocess.run(["cargo", "build", "--release", "--locked", "-p", "kagari-lua-benchmark"], cwd=root, env=environment, stdout=log, stderr=subprocess.STDOUT, check=True)
        for name in ["kagari-lua-benchmark.exe", "kagari_lua_benchmark.pdb"]:
            shutil.copy2(root / "target/release" / name, binary / name)
        metadata = {
            "build_seconds": time.perf_counter() - started,
            "profile": "release opt-level=3; debug=2 for symbols; no throughput measurements",
            "rustc": subprocess.check_output(["rustc", "-Vv"], text=True),
            "cargo": subprocess.check_output(["cargo", "-V"], text=True),
            "machine": machine(),
            "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
            "git_status": subprocess.check_output(["git", "status", "--short"], cwd=root, text=True).strip(),
            "features": "kagari-embed source,native; mlua lua54,vendored",
            "parallelism": "Cargo default; one target thread; workload processes sequential",
            "cache": "reuse build products; fresh process and runtime per workload; three warmups",
            "environment": {key: os.environ[key] for key in (
                "CARGO_BUILD_JOBS", "CARGO_TARGET_DIR", "RUSTFLAGS", "CFLAGS", "CC"
            ) if key in os.environ},
            "driver_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            "lock_sha256": hashlib.sha256((root / "Cargo.lock").read_bytes()).hexdigest(),
            "source_sha256": {
                path.relative_to(root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
                for path in sorted((root / "benchmarks/lua-comparison").rglob("*"))
                if path.is_file() and path.suffix in {".rs", ".lua", ".kgr", ".toml"}
            },
            "binary_sha256": hashlib.sha256((binary / "kagari-lua-benchmark.exe").read_bytes()).hexdigest(),
            "pdb_sha256": hashlib.sha256((binary / "kagari_lua_benchmark.pdb").read_bytes()).hexdigest(),
        }
        (output / "metadata.json").write_text(json.dumps(metadata, indent=2), encoding="utf-8")
    for workload in args.workloads:
        print(f"Sampling {workload}", flush=True)
        sample(workload, binary / "kagari-lua-benchmark.exe")


if __name__ == "__main__":
    main()
