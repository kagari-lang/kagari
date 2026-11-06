# kagari-gc

An independent, explicitly rooted, nonmoving incremental managed heap. This first
implementation has no dependency on Kagari Runtime or language types.

```rust
use kagari_gc::{error::GcError, heap::Heap};

fn main() -> Result<(), GcError> {
    let mut heap = Heap::default();
    let root = heap.scope(|cx| {
        let value = cx.alloc_leaf(42u64)?;
        cx.retain(value) // only the escaping host value needs a Root
    })?;
    heap.scope(|cx| {
        let value = cx.get(&root)?;
        assert_eq!(cx.read(value, |value| *value), 42);
        Ok(())
    })?;
    heap.collect()?;
    drop(root);
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    Ok(())
}
```

Use `Heap::scope` for ordinary object operations. Its `Gc<'gc, T>` handles can be
copied, followed and used in successive reads/writes without registering roots or
holding payload borrows. A handle remains physically live throughout the scope,
even after its source edge/root slot is cleared. It cannot escape the callback or
be mixed with another scope. Reads use `&Scope` and can nest, including reads of
the same object through aliases. Mutable loans/writes, allocation and root
registration require `&mut Scope`; conflicting accesses fail at compile time.
`Scope::get/get_root/follow/downcast` and typed/dynamic field getters also accept
shared access. Read callback results still cannot contain borrowed payload data.

Use `Root<T>` for owning host retention across scopes, `RootSet` for fixed replaceable
root packs, and private `Edge<T>` fields for internal relationships. Root cloning shares
one registration. Last-handle drop only queues a pre-reserved notification; it
never recursively destroys objects or runs collection. Collection is explicitly
driven outside access scopes. Heap can transfer between threads; active access
and concurrent heap operations cannot.

Custom representations implement unsafe `GcObject` once for complete tracing,
pointer-free resumable state and constant-time owned-capacity accounting. Their
reviewed constructors may accept/return scoped Gc handles, importing stored edges
through `Init::edge_scoped`. Scoped allocation uses no temporary Root; explicit
`retain` creates an escaping host root. Raw `alloc_with`,
edge imports and general mutation are unsafe provider APIs; they are not an ordinary
safe object builder. Stored Edge is neither Copy nor Clone and grants no safe
independent dereference. `Access` retains low-level rooted/provider operations;
both surfaces share allocation, mutation and publication kernels.

Providers declare `ScalarField` and `EdgeField` once with unsafe verified
projections. Ordinary get/set/borrow_mut calls are safe: scalar fields preserve
tracing/charge, and optional fixed edge fields publish before replacing a target.
No arbitrary &mut T or Vec<Edge<T>> escapes. Structurally changing containers
still require their reviewed provider operations.

Typed scalar get/set/borrow_mut return `V`, `()` and `&mut V` directly. Typed edge
get/set return `Option<Gc<T>>` and `()`. `Scope::read` returns the callback's own
result, `Scope::follow` returns `Option<Gc<T>>`, and `View::follow` returns `&T`.
Invalid typed access/traversal is a contract violation and panics before the
rejected address is dereferenced. This does not remove ownership or bounds checks.
Fixed-edge publication failure faults the heap before panicking and leaves the
old field intact; catching the panic does not permit further use of that heap.
Foreign input rejected before publication and ordinary read-callback panics do
not by themselves fault a healthy heap; completed scalar edits remain committed.

Allocation/retention quotas, Root import, root-slot lookup, dynamic field layout
checks and downcasts retain `Result`. Collection/provider boundaries still report
failures explicitly: the host may shut down a faulted instance and report an
execution failure, but cannot repair/resume its collector. Panic isolation is
available only with unwinding; `panic=abort` does not offer recovery.

Runtime-defined fixed records use the safe `dynamic` module. Add fields with
`DynamicLayoutBuilder::scalar::<T>()` and `reference()`, which return typed keys
directly without a Result;
consume the builder with `Access::register_dynamic`, then bind each key with
`layout.scalar(key)` or `layout.reference(key)`. Keys from another builder are
rejected even when its schema is identical. The library packs fields by descending
alignment while preserving key identity, then scans immutable reference offsets.
Names stay in caller-owned structs or Runtime metadata; no numeric selectors or
per-access string lookup are needed. The [dynamic-layout example](examples/dynamic_layout.rs)
groups layout and accessors in `PlayerLayout`, used as `fields.hp` and `fields.target`.

Scalars start at zero/false/NUL and references at None. There are no arbitrary
offsets, custom scanners/drops or exposed byte slices on this surface. Keys keep
only their builder identity alive; bound accessors retain registered metadata.

Dynamic records use `Gc<AnyObject>` and `Root<AnyObject>` in the same scope/root
system. Rust handles can be erased with `Gc::erase`; `Scope::downcast` checks the
actual Rust descriptor before recovering a typed handle. AnyObject is only an
identity marker, never a fabricated Rust payload. `Edge<AnyObject>` lets reviewed
Rust providers reference either representation; mixed cycles use one collector.
Field tokens pin exact layout identities, including after the originating layout
handle is dropped. Same-size or same-shaped independent layouts are not interchangeable.

`get_root`/`set_root` operate on host RootSet slots without an intermediate Root.
`RootDriver<R: RootSource>` instead owns a heap plus directly scanned execution
storage, with incoming-reference publication and no mirrored per-frame RootSet.
Every step/collect includes the complete source and host root table. The
[root-source protocol](../../docs/gc-implementation-design.md#driver-owned-root-sources)
is implemented; plain Heap remains the host-root-only driver. The
[Frames example adapter](examples/root_access/frames.rs) offers safe frame/slot
operations around unsafe RootSource, RootAccess::source_mut and scoped source
import/publication primitives. Source loans cannot escape a driver scope, and a
scan failure faults collection before sweep. Run the
[reference-copy comparison](examples/root_access/main.rs) with
`cargo run -p kagari-gc --release --example root_access`; the
[plan](../../docs/gc-redesign-plan.md#remaining-decisions-and-work) tracks remaining
integration and performance work.
Runtime value/tag pairing, compact encodings and integration remain deferred;
physical reference fields do not enforce language nominal types.

The [linked-list example](examples/linked_list/main.rs) shows a safe business API
over that kernel. `values` is a lazy Iterator yielding `i64`, without a per-item
Result. `values_mut` is a lending iterator: next returns `Option<&mut i64>` to the actual
field, and that loan must end before advancing. It exposes no
mutable links or arbitrary Entry. Both keep an ordinary Gc handle as their current
position; the core has no linked-list cursor. Iteration uses safe field operations,
with no per-hop Root or GC protocol in the business loop. Typed and dynamic scalar
borrow_mut operations return an exclusive reference tied to the Scope loan.
Scoped static reads/loans reuse the admitted live T and compute its fixed payload
offset; dynamic fields check the pinned layout ID directly in the live header.
Release access avoids repeating page/slot resolution; debug access checks those
invariants. Heap/state checks, exclusive mutable loans and reference barriers remain.
Typed field declarations guarantee stable tracing/charge;
dynamic fields enforce this through their closed scalar set. No scalar guard,
finish call or destructor protocol is needed, including during unwind. General
ObjectMut and callback mutations still share accounting/finalization and preinstall
conservative Restart. Capacity-changing provider operations must commit before
exposing a safe result, including when a guard is forgotten.

View::follow remains useful for read-only traversal within a callback. Followed
references cannot escape or overlap mutation. Use View::retain when a target
must survive the interval. A scoped Gc already survives source changes until the
scope ends. The host-owned List registers one head slot at construction via
`List::new(&mut Access)`, including for an empty list. `push_front` replaces that
slot through publication without registering a new Root. `pop_front` first roots
its escaping result, then updates the same head slot and disconnects the removed
node. Failure to reserve the result root leaves the list unchanged; the returned
node requires a spare registration, while insertion and traversal do not. The [external provider contracts](tests/support.rs)
also implement Vec append and ordered removal with actual next/end cursor repair.

```text
cargo run -p kagari-gc --example linked_list
cargo run -p kagari-gc --example dynamic_layout
cargo run --release -p kagari-gc --example linked_list -- --measure
```

The optional measurement is characterization of owning-host allocation and full
collection, with three warmups and eleven samples in one process. It records no
comparative gate result or p99. Managed backing/indirect peaks exclude root-table,
Arc/control, workspace and allocator overhead; they are not whole-heap requested
bytes or RSS. Runtime field/script performance cannot be inferred from this example.

For repeatable measurements and before/after comparisons, use the
[durable benchmark suite](BENCHMARKS.md):

```text
uv run python scripts/benchmark_gc.py --output target/gc-benchmark/before
uv run python scripts/benchmark_gc.py --baseline target/gc-benchmark/before --output target/gc-benchmark/after
```

Run from the repository root. It builds once, archives executables and raw CSV,
runs seven sequential process repetitions, and writes Markdown/CSV/JSON reports.
Comparisons rerun archived old binaries paired with the candidate after checking
recipe/environment identity. `--suite` selects workloads; `--runs 1` is exploratory.
The guide defines timing/memory boundaries and explicitly unmeasured metrics.
Individual examples below remain available for focused diagnostics.

The [scalar field measurement](examples/field_access.rs) compares plain Rust,
typed GC fields, dynamic fields and per-operation versus batch scalar borrowing.
Build once with `cargo build --release -p kagari-gc --example field_access`, then
run `target/release/examples/field_access` (Windows: `.exe`) with process indices
0 through 6 sequentially. Each process prints CSV for three warmup and eleven
measured cycles of 50000 operations per case, rotating case order deterministically.
Setup and collection are outside timers; every sample verifies its result and
GC samples verify reclamation afterward. This is warm single-object, collector-idle
characterization, not allocation, reference barriers, GC pauses or script throughput.
The [measurement summary](../../docs/gc-redesign-plan.md#current-field-access)
records the environment, results and limitations. Pass `0 --dynamic-borrow`
(and subsequent process indices) for dynamic scalar loans; `0 --alternating`
visits 32 distinct objects round-robin and checks every final value.

The [gc-arena comparison](examples/arena_comparison/main.rs) pins gc-arena 0.7.0
as a development-only dependency and measures both collectors in the same binary.
Build with `cargo build --release -p kagari-gc --example arena_comparison`, then run
`target/release/examples/arena_comparison` (Windows: `.exe`) with indices 0 through
6 sequentially. CSV contains three warmup and eleven measured batches per case;
take each process's median, then the median across processes. Cases cover static
scalar access/loans, idle direct-root copies, and allocation/live collection/root
clearing/dead collection of 512 and 8192 scalar leaves. Scope entry and root-bank
creation are untimed; allocation includes storing into roots. Each batch checks
values and reclamation. These are safe-API throughput comparisons, not identical
collector contracts, pause distributions, dynamic script fields or Runtime results.
The [method and results](../../docs/gc-redesign-plan.md#performance-evidence)
record ownership differences and the remaining performance gaps.

Repeated typed allocation can reuse `let descriptor = heap.access()?.register::<T>()?`
and call `descriptor.alloc_leaf(cx, value)` inside `Heap::scope`, or the unsafe
provider constructor `descriptor.alloc_with(cx, initializer)`. This avoids repeated
TypeId lookup; per-allocation admission and rollback remain. Run the arena fixture
with `0 --prepared` to measure this path separately; registration is outside the
prepared timer, while the ordinary case includes first registration.

The [reference field fixture](examples/reference_access.rs) measures typed and
dynamic fixed-edge writes in Idle, Mark and Sweep. Run
`cargo run --release -p kagari-gc --example reference_access`; discard samples 0-2
and take the median of samples 3-13, then repeat seven sequential processes.
Targets alternate between two retained objects; this measures repeated publication,
not a fresh-white-target barrier on every iteration. Collection/result checks are
outside the timed loops. First-white-target latency and integrated barriers
remain [open measurements](../../docs/gc-redesign-plan.md#remaining-decisions-and-work).

When driving small incremental steps, inspect `StepReport::required_budget`.
`Some(units)` requests a larger next allowance for a bounded scanner; it is
recoverable and does not fault the heap. `collect()` already supplies enough.
Root/descriptor scans enumerate current registrations, so released historical
slots do not permanently inflate empty-collection work.

Automatic scheduling is available at `Heap::safepoint()` and
`RootDriver::safepoint()`. Call after ending a scope and installing escaping values
in host/execution roots. The method chooses sleep, one normal step or a synchronous
current-cycle assist from allocation growth and configured limits. It never
collects inside an access scope. `HeapConfig::pacing` configures thresholds; the
default minimum growth is 16 KiB and the relative threshold is 50% of post-cycle
managed bytes. Assists can exceed normal step work/retirement caps in aggregate
and have no pause deadline. Explicit `step` remains available for caller-controlled
slicing. See the [policy contract](../../docs/gc-implementation-design.md#automatic-service-at-driver-safepoints).

The [stage fixture](examples/collection_profile/main.rs) distinguishes root,
object-mark, sweep and transition slices for 8192 unique leaves, duplicate roots
and a one-root dynamic chain. It separately times full collection; slice timings
include per-step overhead and should not be summed as exact full-collect costs.
The [sustained fixture](examples/sustained_allocation/main.rs) allocates 128 batches
of 256 leaf/self-cyclic objects into a 512-slot live ring and invokes one safepoint
per batch. `--burst` replaces every sixteenth batch with 4096 allocations. Both
fixtures use three warmups and eleven measured runs; build before timing and run
seven sequential processes. CSV records operation/service times, work, cycles,
normal/assist counts, object populations and managed peak. The stage fixture
also emits individual step rows. The sustained fixture separately records
maintenance and the post-root-release final collect; validation is outside the
timers. CSV output is buffered until measurements and checks finish. Commands:

```text
cargo build -p kagari-gc --release --example collection_profile --example sustained_allocation
target/release/examples/collection_profile
target/release/examples/sustained_allocation
target/release/examples/sustained_allocation --burst
```

Use the `.exe` suffix on Windows. Results, tail aggregation and remaining limits
are in the [active plan](../../docs/gc-redesign-plan.md#collection-and-retained-capacity).

Stable-ID page/root/descriptor tables now release empty blocks, including holes
below a high live ID. Full collection and completing safepoints also shrink excess
auxiliary buffers, with `maintenance_elapsed` included in report `elapsed` but
outside collector work units. This suffix is not time-bounded. Explicit `step`
keeps large buffer copying separate; call `trim_metadata()` at a chosen idle
boundary. It returns `None` during a cycle and errors on a closed/faulted heap.
`metadata_stats()` reports requested directory/buffer capacity separately from
object backing and RSS; it walks allocated blocks and is a diagnostic operation.

The [capacity fixture](examples/metadata_capacity/main.rs) exercises a peak of
8192 pages with one host root, 8192 independent roots, and 8192 descriptors, keeping
one high-ID survivor in each case. Run
`cargo run -p kagari-gc --release --example metadata_capacity`. The [recorded results](../../docs/gc-redesign-plan.md#collection-and-retained-capacity)
include memory exclusions, retained partial blocks and measured throughput costs.

The library provides typed Rust and fixed dynamic allocation/access, roots,
mutation, incremental tracing, sweep and contained disposal. Runtime cutover,
arbitrary erased providers, native take/tombstones, integrated pressure/cancellation
policy, full performance instrumentation and comparative acceptance remain at their
[plan](../../docs/gc-redesign-plan.md) owners. There are no placeholder production
entrypoints for deferred capabilities. Existing Runtime behavior is unchanged.

## Graph and memory-safety validation

The [graph model](tests/graph_model.rs) compares real heap operations with an
independent ID graph. It mixes cyclic/shared nodes, fixed root-slot replacement,
dynamic reference/scalar fields and resumable vector insertion/removal with small
collection slices. The model stores no GC handles. Each operation checks live
values/edges and per-object destructor counters; a fresh full collection must
reclaim exactly the unreachable objects. Dropping all anchors must release object
backing and indirect storage. These are correctness checks, not benchmarks.

```text
cargo test -p kagari-gc --test graph_model
```

The default uses eight fixed seeds and 1024 generated operations per seed, plus a
cyclic-vector prelude and service slices. Set `KAGARI_GC_SEED` to a decimal `u64`
and `KAGARI_GC_OPERATIONS` to select another deterministic run. On a caught panic the test
prints the executed operation trace and attempts to save it below workspace
`target/gc-graph-failures/`. Set `KAGARI_GC_REPLAY` to its absolute path to replay
the sequence with the same test command. Replay overrides seed/count selection;
operations reference allocation-order IDs and must preserve model reachability.
This is not yet a coverage-guided fuzzer or automatic trace reducer.
Interpreter-detected undefined behavior can abort before trace persistence; the
fixed Miri seed remains the reproduction input in that case.

The `gc-safety` [CI job](../../.github/workflows/ci.yml) pins Miri to
`nightly-2026-10-22` and interprets the Linux x86-64 target with
`MIRIFLAGS=-Zmiri-strict-provenance` and the default aliasing model. It runs a bounded
one-seed/64-operation graph, existing layout/failure tests, and selected shared
access, erased-layout, thread-transfer and execution-root cases. Miri ignores the
seed/replay environment overrides so isolated CI runs remain deterministic.
It also checks typed-access panic quarantine and foreign-edge rejection.

```text
rustup toolchain install nightly-2026-10-22 --profile minimal --component miri --component rust-src
cargo +nightly-2026-10-22 miri setup --target x86_64-unknown-linux-gnu
cargo +nightly-2026-10-22 miri test --locked -p kagari-gc --target x86_64-unknown-linux-gnu --test graph_model
```

Set `MIRIFLAGS` in the calling shell before the Miri commands. Passing these
executed paths is not proof of soundness or coverage of arbitrary unsafe providers.
The [plan](../../docs/gc-redesign-plan.md#current-validation-evidence)
records actual local evidence and remaining fuzzing, sanitizer and Runtime work.
