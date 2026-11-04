# Candidate Native fold characterization

Run from the repository root:

```text
uv run python scripts/benchmark_managed_fold.py --output target/managed-fold/baseline
```

To compare a later implementation against that archived normal-release image:

```text
uv run python scripts/benchmark_managed_fold.py --diagnose --baseline target/managed-fold/diagnosis-before --output target/managed-fold/diagnosis-after
```

Create `diagnosis-before` with `--diagnose` before changing implementation code.
Comparison reruns both executables under current conditions, alternating old/new
and new/old by process index. It validates executable hashes and matching machine,
toolchain, build inputs, fixture Rust sources and matrix settings. Production Rust
may change. Each process index sees the same rotated recipe. Paired change is the
median of index-matched candidate/baseline time ratios minus one; it does not
divide new timings by historical estimates. Diagnostics/sampling images cannot
serve as throughput baselines.

The runner builds only `kagari-vm --example managed_fold` in the workspace release
profile, archives its executable and runs five sequential independent processes.
Each process rotates ten routes across three warmups and eleven measured samples.
It reports the median of process medians in ns/item and us/2048-item fold, with the
range of process medians. Ranges are not confidence intervals. `--runs 1` is a
fixture check, not performance acceptance. No external benchmark dependency is added.

| Route | Timed work |
| --- | --- |
| `rust` | Direct prebuilt Rust Vec iteration and scalar addition; black_box on each input prevents an arithmetic-series shortcut/vectorized sum |
| `managed_rust_scalar` | The actual candidate fold over unboxed u64 values with a Rust reducer |
| `managed_rust_references` | The same fold over NativeObject<Leaf<u64>> elements, projecting each object's scalar payload |
| `bridge` | Direct prebuilt Rust Vec loop calling the real script reducer through the existing typed Function API |
| `scalar` / `references` | The actual candidate scalar/reference fold calling the same script addition reducer through that API |
| `production` | The currently registered Vec<u64>/Iterable::fold with a script addition reducer |
| `bridge_gc` / `references_gc` / `production_gc` | Corresponding script callback allocates a two-element array per item; every 256th item calls a Native function that collects both heaps before reading the array again |

All routes sum 1 through 2048 and validate the result after timing. Native-only
routes batch 256 direct folds or 16 candidate folds; script routes time one fold.
Batch repetition remains in the timed interval and is normalized by actual work.
The candidate source vectors and their elements are created once before sampling,
with two immutable host roots retaining them throughout. Candidate folds still
enter a real Memory::call, admit the source and allocate/release three working
slots. They invoke the original algorithm without an optimized benchmark fork.
The candidate heap has 2050 objects and two root registrations after every batch.

Both heaps are explicitly collected before each batch outside timing; automatic
production collection is disabled. Pressure callbacks collect both heaps at the
same eight points per fold, including scanning the same prebuilt candidate data
even for the bridge/production routes. Managed reference folds additionally have
live call slots at those points. Timed pressure work includes collection; it does
not measure individual pauses, p99, sustained automatic pacing or new-heap
allocation throughput. Cleanup after the batch and final teardown are excluded.

The runner stores raw CSV, setup timings, compiler/machine/build metadata, source
and executable hashes, process estimates and paired ratios under the requested
new output directory. Compilation, installation/linking and data construction
are separately recorded, never divided into traversal time. Keep the machine
idle; no simultaneous build, test, profiler or benchmark. Frequency, affinity,
power state and background activity are uncontrolled. Warmups make this a warm
execution characterization, not a cold-cache result.

Interpret `scalar/bridge` as the additional candidate traversal/slot cost under
the existing callback machinery. The production comparison also changes iterator
dispatch, language Option materialization, roots, cancellation polling and
generic adaptation; it cannot isolate GC algorithm or cancellation costs.
Script execution remains the bytecode interpreter, and the legacy Native bridge
retains its current conversion/validation costs. These results do not establish
the final generic Native ABI's performance, a complete Iterable implementation or
production collector acceptance. The [active plan](../../../../docs/gc-redesign-plan.md)
owns findings and remaining work.

## Callback diagnosis

```text
uv run python scripts/benchmark_managed_fold.py --diagnose --output target/managed-fold/diagnosis
uv run python scripts/benchmark_managed_fold.py --counts --output target/managed-fold/counts
uv run python scripts/benchmark_managed_fold.py --sample bridge --sample bridge_raw --sample managed_rust_scalar --output target/managed-fold/profile
```

The diagnostic timing matrix keeps candidate Rust/scalar/reference rows and adds
five controls: `bridge_prepared` prepares the typed Function once per fold;
`bridge_raw` uses the existing borrowed CallableHandle/scalar argument API;
`bridge_raw_values` uses its general Value API; `script_loop` performs closure
calls inside a script loop, and `script_sum` performs addition without a closure.
The two script controls include their loop instructions. All return the same sum;
they are not instruction-identical subtractions for individual check costs.

Counting builds the same example with `--features diagnostics`, forwarding the
existing Runtime diagnostics feature and using its existing counting allocator.
It measures one complete warmed fold per route, not throughput. It also counts
2048 isolated scalar mapping checks and two-scalar argument encodings with their
setup outside the counting scope. Warm logical window admissions may reuse bank
capacity and must not be described as physical allocation counts. Counting
binaries reject throughput/sampling mode. The runner archives the instrumented
executable, metadata, setup log and counts separately from ordinary timing.

Sampling uses the existing x64 Windows main-thread RIP sampler with strict local
PDB symbol-range checks, three warmups and a ten-second interval per route. It
builds with `--release --config profile.release.debug=2`, preserving release
optimization while requesting full debug information, and archives the executable
and matching PDB. The user authorized this sampling-only symbol setting on
2026-11-04; ordinary throughput/counting commands keep their existing profiles.
See Cargo's [debug setting](https://doc.rust-lang.org/cargo/reference/profiles.html#debug).
Normal timing runs rebuild with release defaults before archiving their own image;
they do not reuse the sampling executable implicitly.
Sampling is independent of throughput and has no inclusive
stacks. Inlining/merged symbols, unresolved addresses and suspend/resume overhead
limit attribution; unresolved fractions remain in the raw report. The no-GC
sampling loop creates its outer script closure on each fold and does not perform
the timed matrix's pre-batch collections, so do not treat it as another throughput
sample. The [active diagnosis](../../../../docs/gc-redesign-plan.md#native-callback-architectural-diagnosis)
records evidence, limits and the intended ownership changes.
