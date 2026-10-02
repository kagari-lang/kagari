# Architecture review probes

Companion sources for the [2026-10-03 review](../../architecture-review-2026-10-03.md),
written against commit `9581f5ad2a754911d1edb92eb35e957a18d7b5df`.
They are a separate workspace, not production dependencies or correctness tests.
Run the commands below from the reviewed repository root. Build first; run
measurements serially, without competing build/test processes.

```sh
mkdir -p target/architecture-review
cp Cargo.lock docs/review-support/architecture-2026-10-03/Cargo.lock
cargo build --offline --manifest-path docs/review-support/architecture-2026-10-03/Cargo.toml --target-dir target
target/debug/kagari-interface-review-probe > target/architecture-review/interface-probe.log
target/debug/startup_probe > target/architecture-review/startup-probe.log
```

The copied root lockfile pins the repository dependency versions. The first build
must permit Cargo to add the standalone package to that copy, so it omits
`--locked`. `--offline` assumes the repository dependencies have already been
fetched; omit it if the local cache is empty. The copied lockfile is ignored.
The package uses the repository's dev profile optimization level (1), normal
target directory, default Cargo parallelism, and the SDK's default source/native
features. Build time is not a reported execution measurement. Repeat each
prebuilt binary in three separate processes when comparing with the report.

## Interface dispatch

`src/main.rs` generates traits of width 1, 8, and 32. Every method returns `1`.
Each root execution constructs one interface value and calls its `m0` method
1,000 times. Preparation checks that the bytecode contains one dynamic interface
call site. Every execution must report interpreter mode and return `1,000`.
The generated workloads are saved under ignored
`target/architecture-review/workloads/` before timing.

For each width, five warmups precede 21 measured samples. The measured region
includes SDK session entry, one interface construction, 1,000 calls, and return
report creation. Compilation, preparation, linking, printing, assertions and
report destruction are outside the region. Root/session cleanup performed by
`execute` is inside it. Counts cover the current thread only, using a calibrated
System allocator wrapper. Allocator instrumentation adds timing overhead.
Requested bytes are cumulative allocation/reallocation requests, not live bytes,
peak memory or RSS. Counts include fixed session and construction work, so the
probe does not isolate one raw dispatch instruction.

Compare allocation growth alongside time. Timings alone do not establish the
speedup of a proposed implementation. In particular, this is not a JIT benchmark.

## Runtime initialization versus linking

`src/bin/startup_probe.rs` compiles `fn main() -> i32 { 40 + 2 }` once, prepares
one shared program, and prints its module/declaration/import counts. One warmup
precedes eleven samples. Each sample creates a fresh runtime, then links the
shared program, with separate timers. Engine construction, source compilation,
artifact preparation, execution checks and every drop are outside both timers.
It asserts exact verified-program sharing and a result of `42`.

This binary uses the ordinary System allocator without counters. Its times must
not be subtracted from the differently instrumented and scoped
`architecture_baseline` measurements as an exact decomposition.

## Recording results

Record the commit, toolchain, machine, profile, features, cache state and other
machine activity. Preserve per-process medians and ranges; do not hide noisy
runs. Keep raw logs in `target/architecture-review/` and durable conclusions in
the review report. The original measurements used the same sources staged under
`target/architecture-review/interface_probe/`; the durable companion uses a
distinct package name, formatting and an ignored output directory for generated
workloads. The measured regions are unchanged.
