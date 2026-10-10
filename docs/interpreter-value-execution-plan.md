# Compact values and interpreter execution (VE00-VE09)

Status: implementation and local acceptance complete through VE09 (`9dfeba3c`).
Lua parity remains unmet; complete GitHub CI acceptance remains unrun. This record
retains implemented decisions, evidence and open gates, not instructions to replay
completed phases. The [roadmap](implementation-roadmap.md#interpreter-performance-follow-up)
owns future activation. Full phase contracts, intermediate failures and measurements
are recoverable with `git show 9dfeba3c:docs/interpreter-value-execution-plan.md`.

## Implemented representation and boundaries

- `Value` is 16 bytes and Rust Copy in debug/release on the measured 64-bit target,
  down from 32 bytes. Scalar banks remain untagged; copying a Value allocates no
  backing payload, increments no reference count and establishes no GC root.
- `HeapObjectId` uses checked owner/slot/generation indices (12 bytes), selected
  over direct pointers after VE00 probes. Exhaustion cannot wrap into a valid ID.
  Heap storage remains nonmoving mark-sweep; no moving/generational collector was added.
- Strings, immutable tuples, ranges and ephemeral host payloads use traced backing
  storage. Retained host handles remain owning/non-Copy. The smaller Value does
  not imply half the process memory or removal of backing allocations.
- Verified bytecode uses indexed constants. String constants materialize lazily
  per runtime/module generation and are traced through the executable version graph.
  Shared verified products contain no runtime-local heap IDs. Escaped strings can
  survive independently through roots; readonly native string access borrows storage,
  while conversion to an owned Rust String explicitly copies it.
- Concrete fields and script calls retain verified physical layout/transfer facts;
  call binding uses the caller's exact pinned program. Selected Vec/Map paths reuse
  scoped access. Native/closure/shared/interface paths retain their adaptation checks.
- Scalar execution segments borrow code, PCs and bounded scalar/init slices once.
  Logical instructions, debugger origins, slice accounting, checked arithmetic and
  cancellation boundaries are preserved. No unsafe pointer or instruction fusion
  was introduced.

| Owner | Current responsibility |
| --- | --- |
| [Values](../crates/kagari-runtime/src/value.rs), [GC](../crates/kagari-runtime/src/gc.rs) | Compact representation, checked references, traced storage and controlled mutation. |
| [Roots](../crates/kagari-runtime/src/gc/roots.rs), [frames](../crates/kagari-runtime/src/frame.rs) | Host retention, temporary/active/suspended values and cleanup. |
| [Prepared execution](../crates/kagari-runtime/src/module/execution.rs), [cursor](../crates/kagari-runtime/src/frame/cursor.rs), [VM](../crates/kagari-vm/src/executor) | Sealed operations, physical transfers and observable execution boundaries. |
| [Enum layouts](../crates/kagari-runtime/src/module/layouts.rs) | Prepared variant/payload scope and generation-aware compatibility. |
| [Benchmark driver](../scripts/benchmark_lua.py) | Unchanged workloads, paired results, checksums and measurement metadata. |

## Safety invariants

Runtime identity, slot generation, bounds, declared access, payload contracts and
host borrow validity remain checked wherever foreign, stale or unproven values can
enter. Verified facts may be reused only within their owning immutable program or
admitted execution scope. Rust locals holding Copy values are not implicit roots.

| Transition | Required invariant |
| --- | --- |
| Host entry | Validate identities/contracts and retain arguments/code before execution. |
| Allocation or callback | Publish live values/PC, protect temporaries and release incompatible borrows before collection or reentry. |
| Return or replacement | Protect outgoing/new references before retiring frames/roots; preserve mutation commit and failure order. |
| Suspend/resume | Park roots and exact dependencies without transient views/host borrows; re-admit on resume. |
| Trap/cancel/depth failure | Release frames, roots, borrows and code retention through the established cleanup boundary. |

Preserve left-to-right once-only evaluation, completed effects, generation-pinned
reload, candidate isolation and readonly alias behavior. Runtime transfer stays
exclusive and outside active scopes. Portable products never encode process pointers
or runtime-local handles. Ordinary tracing does not require a new generational
write barrier; controlled publication/mutation still accounts for every edge.

### Debug-only invariant diagnostics

The user explicitly permits extra debug-only fields and checks to support the
reference migration. Use `#[cfg(debug_assertions)]` for diagnostic fields or side
metadata, such as expected heap owner, allocation generation, object kind/layout
or execution-scope identity. Use debug assertions to cross-check facts already
guaranteed by validated construction, roots and the admitted execution context.
Gate on debug assertions explicitly rather than assuming an optimization level
or profile name determines instrumentation.

These diagnostics must not supply a missing release invariant. Foreign/stale host
handle rejection, artifact validation, dynamic bounds, borrow validity and required
failure/quarantine behavior remain enforced in release. Root publication, cleanup,
state updates and other required work must never occur only inside a debug assertion.
Debug checks must establish pointer validity before dereferencing; reading freed
memory to inspect its supposed generation is not a valid diagnostic.

Diagnostic metadata must not add strong roots, change collection eligibility or
retain old code/resources in a way that hides missing production retention. Keep
it separate from portable formats and native ABI layouts. Debug-only fields in a
Copy value must themselves be Copy. Measure the release size/performance contract
separately from diagnostic overhead, and exercise the affected correctness paths
with debug assertions both enabled and disabled.

## Completed checkpoints

| Phase | Result | Commit |
| --- | --- | --- |
| VE00 | Preserved baseline, consumer inventory and compact-index selection | `29a5df9b` |
| VE01 | Checked compact heap identities and ownership/roots | `b45b5dfb` |
| VE02 | 16-byte Copy Value and migrated consumers | `60f700a1` |
| VE03 | Indexed constants and shared string access | `511e40a2` |
| VE04 | Value/GC/host/async integration and allocation gate | `0f6252f8` |
| VE05 | Prepared fields and scoped collection operations | `263499b5` |
| VE06 | Prepared script calls, physical transfers and frame retirement | `13c151ef` |
| VE07 | Borrowed scalar execution segments | `0a6c34f0` |
| VE08 | Full local integration and preserved-baseline comparison | `06757739` |
| VE09 | Explicitly authorized native enum-result layout reuse | `9dfeba3c` |

### VE09 native result layout reuse

The Map::get/Option probe found 90 of 100 sampled requests in enum-layout graph
comparison. Native result caching already hit, but patterns consumed equal applied
layouts from different module slots. The existing EnumVariantRef owner now compares
complete layouts directly within one exact ProgramDescriptor, only when both
lexical environments are absent. Runtime owner and variant checks precede the
comparison. Different versions/scopes retain the general graph comparator.

The proof depends on equal entire layouts (declaration, arguments, all variants and
payload types) resolving nested nominal definitions through the same immutable
program. GC handles, dynamic payloads and publication remain checked; no new cache,
public API, layout field, unsafe code or semantic implementation was added.

## Local acceptance and measurements

[VE08 final evaluation](performance-baseline.md#compact-value-and-interpreter-final-local-evaluation-ve08-2026-10-10)
records a passing `cargo test --workspace --no-fail-fast`, workspace Clippy plus
post-fixture affected Clippy, format, structure and diff checks. Earlier failures
were old inline-string/current-constant-cache accounting assumptions; fixtures now
retire versions and retain exact final zero-root/object assertions. No production
semantics were changed for those repairs and no local errors remain.

[VE09](performance-baseline.md#native-enum-result-layout-reuse-ve09-2026-10-10)
passed 38 existing enum, source-free, generic reload, hash callback, ownership and
conversion contracts, runtime all-target Clippy and lightweight checks. It did not
repeat VE08's workspace suite. Map time is 0.489x VE08; 5,000 Map::get calls request
50,280 Rust allocations versus 500,280, with unchanged 5,001 script objects and ten
GC collections. Other workload ranges and small regressions remain in the report.

The performance report retains final tables, hashes, environment, timing scope and
reproduction. Temporary binaries/probes/logs live under ignored `target/ve00` through
`target/ve09`; they are not durable release assets. Full GitHub CI/backend acceptance
has not been established by these local runs.

## Open acceptance and future scope

- [ ] Complete GitHub CI feature/backend acceptance.
- [ ] Interpreter/Lua median <=1.0 for every frozen matched nontrivial workload.

All 16 matched workloads still fail parity (3.66-201.94x in VE09): original
arithmetic, arrays, branches, calls, fibonacci and maps; source-form direct, helper,
concrete generic, interface, shared generic, capture cell, field, byte state, string
constants and string calls. Entry and host adapters are separate; numeric diagnostics
use bounded domains/different loop constructs and cannot replace the matched gate.

Require repeated same-machine release measurements and uncertainty analysis near
parity. Do not change workload semantics, lower the target, substitute JIT results
or use an average to hide a failing workload. Implementation completion is not full
performance acceptance. Later work requires a finite activation with measured costs
and preserved correctness; VE09 is completed, not an unactivated proposal.

General enum unboxing, new collectors, global dynamic-string interning, escape
analysis, unrelated frontend/dispatch changes, JIT expansion and new library APIs
remain outside this completed track. Follow [AGENTS.md](../AGENTS.md) for any new
checkpoint; detailed historical command/failure narratives stay in Git history.
