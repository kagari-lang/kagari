# Native Collections Reset Plan

Status: replacement plan defined on 2026-10-02; implementation has not started.
The previous restoration goal remains paused. This documentation update does not
resume it or start implementation.

This is the active native-library plan. It replaces NR00-NR05, the full-library
restoration sequence and inherited ST06 acceptance obligations. Historical results
are evidence about the predecessor, not a requirement to restore its public
surface. See the [roadmap](implementation-roadmap.md) for other queued work.

## Objective and boundaries

Complete four ordered phases: remove the old library implementation, establish
compiler-owned language protocols, replace the costly native invocation path,
and prove the design with one collection package. Finish cleanup before building
its replacement; do not keep both architectures while migrating algorithms.

- Collection algorithms stay in Rust. Do not rewrite them in Kagari or introduce
  a separate standard-library crate.
- Language types and protocols required by syntax, static checking or implicit
  value semantics are always available, independently of installed libraries.
- Library and application native functions share registration, checked signatures,
  linking and execution. Native packages implement compiler-owned protocols
  without redeclaring their contracts.
- Native registrations own exported native declarations and bindings. Generated
  `.kgr` files provide tooling navigation, completion and documentation, not
  executable compiler input or a separate signature authority. Installation
  needs no declaration binary.
- Ordinary native functions and native-to-script calls execute synchronously.
  Callbacks alone do not require continuation state machines. Persistent state
  belongs to lazy iteration or actual asynchronous suspension.
- Avoid redundant allocation, rooting, metadata copying, validation and target
  resolution in hot loops. Performance is part of acceptance.
- Preserve static typing, checked arithmetic, alias identity, bounds, GC safety,
  failure semantics and generation-pinned calls. Budget schedules and permission
  matrices are not requirements or prerequisites of this reset and must not
  dictate the new native ABI or add per-step/per-call bookkeeping to it.

Out of scope: full-library restoration, Map/Set algorithms, arbitrary inline
script-struct layouts, a complete async executor, JIT/LLVM feature expansion,
new syntax, LSP transport, broad Rust interoperability and a separate execution-
policy migration. Necessary consumer changes belong to their owning phase.

## Phase 1 — Remove the previous library implementation

Task: establish a clean boundary without executable dependencies on the old
standard-library package or its restored native algorithms.

- Remove `kagari-stdlib`, its workspace/dependency/build integration, legacy source
  preparation and library declaration catalogs. Remove the retired handwritten
  and generated `stdlib/*.kgr` products.
- Remove the restored array/sorting, math/numeric, string, option/result, debug,
  cmp/hash/fmt/ops library registrations and business algorithms, their default
  installation wiring and per-standard-method selectors across all consumers.
- Remove the mixed `StandardTrait` catalog and associated library-owned type,
  method and blanket-implementation tables. Renaming that catalog is not cleanup.
- Retain independently justified language/runtime primitives: value layouts,
  GC storage, ordinary trait machinery, checked arithmetic, enum operations and
  generic host/native infrastructure. Move language-owned behavior to its actual
  owner rather than hiding a dependency on library registration. Reassess retained
  native infrastructure in phase 3.
- Retire examples, feature routes and tests tied solely to removed APIs or the old
  invocation model. Preserve meaningful language and boundary cases in focused
  tests and record intentionally withdrawn library APIs. Do not weaken assertions
  to hide failures in retained behavior.
- Remove tracked `.kbc` products and embedded-binary consumers. Generate required
  fixtures under ignored `target/` from reviewed source/API fixtures. Preserve
  independent source-free loading and rejection coverage without Git binaries.
- Reconcile the existing uncommitted implicit-protocol migration explicitly:
  remove library coupling and retain only justified language facts. Do not reset
  the whole worktree or discard unrelated changes.

Exit: no retired library crate, source loader, registration, algorithm or tracked
executable fixture remains on an execution path. Record the removal audit and test
disposition here. Independently retained units build and pass focused checks.
Any compiler gap caused by removing old declarations is recorded once with its
reproduction and phase 2 owner; do not reinstall the old package to bridge it.

## Phase 2 — Implement compiler-owned language protocols

Task: provide complete contracts and implementations without any installed native
library. Audit existing syntax/specifications first; do not copy all 38 old traits.

| Family | Language-owned responsibility |
| --- | --- |
| PartialEq / Eq / Hash | Complete signatures, eligibility, scalar behavior, object identity, tuple/enum composition and explicit override precedence |
| PartialOrd / Ord | Signatures, Ordering, scalar implementations and floating-point partial ordering |
| Arithmetic, bitwise and unary operator traits | Existing signatures, associated outputs, checked primitives and ordinary user implementation selection |
| Index and existing writable indexing rules | Index/output contracts, read/write lowering and once-only evaluation; no new assignment syntax |
| Iterator / Iterable | Associated item/iterator contracts and ordinary selected implementations used by for loops |
| Fn | Function/closure contracts, argument tuples, associated outputs and normal callable implementations |
| RangeBounds and range forms | Contracts required by existing range/index syntax and bounds checks |
| Debug / Display | Existing implicit formatting contracts and implementations; public printing functions remain library/host APIs |

Primitive types, Option, Result, Ordering and syntax-required range forms also
belong to the language. Their declarations stay available with libraries disabled.
Option/Result convenience methods and collection algorithms remain library work.

List/MutableList, Map/Set interfaces, FromIterator, Sum/Product and conversion/
parsing helpers remain library declarations where existing syntax or implicit
semantics does not require them. Their presence in the predecessor catalog does
not make them compiler features or require their restoration in phase 4.

- HIR receives complete compiler-owned declarations, bounds and associated outputs.
  Native and script impls use ordinary trait checking and nominal identities;
  user traits named Eq or Iterator do not acquire language hooks.
- Specialization selects implementations once. Portable contracts retain checked
  signatures, layouts and concrete targets. Offline verification/runtime loading
  do not depend on HIR or source analysis, and generic ABI records do not recreate
  a complete library catalog.
- Make the same compiler-owned declarations available to tooling without a second
  signature authority or an optional native provider.

Exit: with libraries disabled, primitive operators, implicit value behavior,
user-defined operator/index impls, closures, Option/Result propagation and for
loops over user-defined iterators type check and execute. Invalid signatures,
bounds and associated outputs are rejected. Existing language semantics are
covered and phase 1 language-contract build gaps are resolved.

## Phase 3 — Optimize native calls and collection storage

Task: put a small prepared boundary around ordinary Rust functions, including
synchronous calls back into script code.

- Direct calls return results without allocating Box<Completed> or entering an
  advance/receive handshake. Separate them from genuinely suspended invocations.
- Use scoped contexts over already-rooted arguments. Do not rebuild argument
  vectors/root sets or clone complete signatures/selected-application lists merely
  to read arguments. Escaping values use explicit owning handles.
- Resolve generations and concrete callable slots at linking/preparation. Repeated
  Eq/Hash/Ord/Fn calls do not scan modules, function lists or native imports, infer
  types again or clone complete contract records.
- Provide synchronous typed callback invocation returning a result. Native authors
  write normal Rust loops and error propagation, not program counters for callbacks.
- Keep necessary handle/value checks and GC tracing at the correct boundary.
  Verified contracts eliminate redundant checks; unchecked casts and escaping
  unrestricted Rust references are not acceptable substitutes.
- Interpreter and future compiled callers share prepared contracts. Use matching
  generation-pinned compiled callback targets when actually available, without
  expanding the JIT backend as part of this phase.
- Choose storage from the declared element type at construction, including empty
  collections. ArrayList<i32> uses Vec<i32>; supported primitive types have compact
  contiguous buffers. Generic/reference-bearing values may use traced Vec<Value>.
  Explicitly define the specialization set and fallback; script types do not
  dynamically become Rust generic instantiations.
- Root the shared collection object rather than each primitive element. Scoped
  typed bulk access checks layout once; primitive inner loops do not construct
  GenericValue proxies, register roots or repeat element type checks.
- Use direct primitive comparison only when that exact implementation was selected;
  custom implementations use prepared callbacks. Do not hold a Rust buffer borrow
  across script reentry that can access/resize it. Callback algorithms use rooted
  working storage and preserve specified failure/commit semantics.
- Lazy iterators retain their cursor/captures and GC edges between next calls.
  Each next can execute synchronously. Continuations are reserved for actual
  suspension; a lazy iterator does not force every internal operation into a
  state machine. Full async implementation remains deferred.

Exit: application scalar/generic/callback natives work without a library. Reentry,
errors, GC and reload release roots and retain correct generations. A warmed
ordinary scalar adapter adds no heap allocation. An i32 bulk loop adds no
per-element allocation, rooting, metadata copying or callable lookup. Measure
these properties and record remaining unavoidable costs before acceptance.

## Phase 4 — Verify one representative library

Task: implement one optional ArrayList package using the completed boundaries.
The fixed surface is ArrayList<T> with new, len, push, get, set, sort, sort_by and
iter, plus one lazy map adapter and next. Implement compiler-owned Index, Iterable
and Iterator as applicable. Reuse Rust storage and established Rust algorithms;
do not restart full-library restoration or write a parallel Kagari algorithm.

- Prove compact i32 buffers and a traced GC-reference fallback, preserving shared
  identity and mutation behavior.
- Exercise built-in ordering, user-defined script Ord and a supplied comparator
  through synchronous invocation. Preserve sort stability and failure/commit rules.
  A comparator error stops further comparisons and leaves original element order
  unchanged while preserving completed effects on referenced payloads.
- Verify lazy consumption, retained captures, shared cursor behavior and cleanup
  on early exit/failure.
- Install the package by default through the ordinary engine mechanism. Disabling
  it removes its APIs while language protocols remain available.
- Add one application-owned native consumer using identical registration/callback
  APIs, without compiler/verifier/VM changes for its business algorithms.
- Generate .kgr views and check signatures/docs/navigation through existing tooling
  queries. Generate one target/ artifact and run an independent source-free
  consumer, including mismatched-contract rejection.

Measure small/large inputs, warmed calls, allocations and callback/lookup counts.
Compare direct Rust buffers, native collections and script algorithms over
equivalent representations and semantics. Separate compilation from execution;
record toolchain, machine, profile, features, parallelism and cache state. Unsupported
JIT cases must be labeled unsupported, not timed fallback reported as JIT. Backend
expansion and a fixed speedup claim are not required by this bounded proof.

Exit: the fixed surface passes behavioral, boundary and measured performance
checks without special library dispatch or algorithm changes in generic layers.
No build/test failure remains in retained workspace consumers.

## Execution and verification policy

- Finish phases in order. No compatibility wrappers, old artifact readers or
  parallel semantic implementations. A language-declaration gap after cleanup
  belongs to phase 2, not a temporary restoration exception.
- Keep the four checklist items fixed. Record discoveries briefly with their phase
  owner; substantial scope growth requires user direction.
- Use focused checks during implementation; do not repeat unchanged expensive
  suites or inherit the old full-library/budget matrix.
- No routine ABI/format bumps for unpublished changes. Regenerate only affected
  disposable artifacts under target/ when needed.
- Use coherent Conventional Commits. Implementation checkpoints carry
  Native-Reset-Phase: 1, 2, 3 or 4. Disclose intermediate build failures and owners
  in the commit and this ledger.
- Final integration runs structure checks, formatting, workspace Clippy/tests and
  git diff --check. Additional feature/backend routes must serve this fixed proof.
  Historical passes are not current acceptance evidence.

## Checklist

- [ ] Phase 1: old library implementation and tracked executable fixtures removed.
- [ ] Phase 2: compiler-owned language protocols implemented independently.
- [ ] Phase 3: efficient synchronous native calls and typed storage implemented.
- [ ] Phase 4: representative ArrayList package and measured proof accepted.

## Progress ledger

2026-10-02 — Planning reset only. The user replaced full-library restoration with
these four phases. No implementation phase has started or completed. The old goal
remains paused; budget/permission work is not a prerequisite.

Entry state: HEAD includes 158558ee (early development policy) and earlier native
restoration checkpoints. Uncommitted work includes implicit PartialEq/Hash/Debug/
Display metadata, compiler lowering, corruption tests, documentation and rebuilt
.kbc products. Phase 1 reconciles these changes without discarding unrelated work.

Previous verification is partial: focused scalar checks passed before the final
corruption case; structure/format checks passed; the route matrix was interrupted
at the user's pause. Old-model failures included fourteen ABI/seventeen HIR lib-
test diagnostics, three legacy source-package failures and an unavailable full-
feature generator. These are not new acceptance claims. Phase 1 owns retirement
or explicit disposition of obsolete targets; phase 2 owns retained language gaps;
final acceptance permits no carried failure in retained consumers. Historical
records remain in Git. The pre-reset worktree documents were copied to ignored
target/native-plan-reset/ for review, not as a second execution/progress ledger.
