# Interface dispatch optimization

Status: active, 2026-10-03. The user authorized the current-implementation R3
proposal and execution with two coherent implementation commits.

## Scope and direction

Resolve the architecture review's R3 in the current FA01–FA05 implementation:
stop whole-interface descriptor copies and reuse receiver-dependent preparation.
Keep checked slots, explicit type/operation arguments, shared script/native
entries, source-free validation, scoped type origins and retained generations.
This is runtime/VM work, not a language, portable format or backend redesign.
Runtime initialization (R4), SDK preparation verification (R6), contract/common
renaming and unrelated interpreter allocations remain separate work.

Interface snapshots become immutable shared runtime records. A resolved call keeps
the receiver rooted and selects metadata by slot or by a shared bound operation.
Rc metadata ownership does not replace GC roots or executable-generation retention.
Call-specific signatures, type arguments and operation witnesses must not mutate
shared receiver metadata. Do not add a global specialization cache or use type
names as cache identities. Interface construction remains valid without optional
method bounds: List<NonOrd> can exist even though sorted requires Ord.

## ID01 Shared descriptors and method selection

- Store interface snapshots in Rc and make snapshot reads shallow clones.
- Resolve each interface call from one rooted snapshot and select its checked slot
  without copying all methods or the selected method's static metadata.
- Represent the existing bound-operation route using shared immutable metadata.
- Preserve argument/result checks, host ownership, trap ordering, roots, reload
  pinning and default/override behavior. No compatibility facade or ABI bump.
- Capture a current width baseline and rerun it after implementation; distinguish
  interface construction from repeated dispatch where the harness permits it.

Acceptance: relevant interface/default/native/generic/reload tests pass; width
measurements show whole-method-table copy allocation is removed from calls.
Commit with `Roadmap-Step: ID01`.

## ID02 Receiver preparation and final acceptance

- Share receiver-bound type/signature metadata and selected receiver operations.
  Prepare only demanded method information and reuse it for the retained receiver
  view; lazy preparation must not require unrelated optional method constraints.
- Pass method-local arguments and caller-selected constraints on each invocation.
  Keep origin/generation information for forwarded operations and captured closures.
- Ensure inherited dispatch does not recreate an entire table on each call.
  Explicit escaping parent views retain their normal value semantics.
- Keep preparation metadata receiver-independent and avoid Rc cycles or retained
  per-call arguments. Preserve cleanup on failure and collection.

Acceptance: calls do not incur table-copy allocations proportional to unused
method count; shared generic defaults/overrides and multiple type arguments,
inherited methods, custom native defaults, forged contracts and retained old
callbacks remain correct. Rerun bounded list measurements, final workspace Clippy
and tests, standalone feature routes, structure/format/diff and documentation links.
Use focused checks during development and rerun only affected failures after a
full sweep. Report actual allocation/time evidence without a zero-allocation or
universal speedup promise. Commit with `Roadmap-Step: ID02`.

## Verification

Use the existing VM interface tests, native_boundary, embedding default_methods,
generic_reload, collection/string and GC/reload consumers. Reuse the existing
allocation counter and sorting harness. Record machine, toolchain, O1 profile,
features, cache state and workload; separate compilation/construction/calls.
Temporary probes and logs belong under ignored target/interface-dispatch.

Each checkpoint runs structure, format and diff checks. Final acceptance includes:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
uv run python scripts/check_features.py
git diff --check
```

## Progress ledger

- [x] ID01 Shared descriptors and method selection.
- [ ] ID02 Receiver preparation and final acceptance.

2026-10-03: Activated from clean commit 0a7c0699. Current snapshots deep-copy the
method vector; RootedInterfaceMethod owns copied static metadata; native generic
defaults may traverse receiver/parent/result tables again. The approved two-step
scope is fixed. No implementation or acceptance claim has been made yet.

2026-10-03: ID01 accepted. Interface GC objects now store Rc snapshots; the rooted
call selection references that snapshot and its checked ordinal, or a shared
BoundOperation. Static signatures, targets and receiver metadata remain in their
owners rather than being copied into each call. Operation forwarding clones Rc
handles. Value semantics still clone a needed receiver value, not the whole table.
RootedInterfaceMethod remains the intentional public call handle; its metadata
access implementation lives in objects/method.rs. No facade/re-export was added.

Evidence: 23 VM interface cases, 71 native boundary cases, 32 embedding default
cases and the serialized generic reload case pass. A new allocation regression
creates the receiver outside measurement and performs 1,000 host-to-VM calls:
widths 1/8/32 each allocate exactly 16,000 times and request 4,888,000 bytes. Every
return is checked, the interface call instruction is verified, and dropping the
receiver root leaves zero roots/live heap objects. These totals include VM entry
work; they do not claim allocation-free calls.

The review's original interpreter-loop probe was rebuilt against 0a7c0699 and
rerun against ID01. Each sample includes one interface construction, 1,000 script
interface calls, SDK entry and report creation; compilation/linking/report drop
are excluded. Five warmups and 21 samples, warm caches, O1, default source/native
features, Cargo default parallelism, one measuring thread and no concurrent build
or test. Environment is rustc 1.98.1 (48a229cea, LLVM 22.1.8), macOS 26.6.2,
aarch64-apple-darwin, M1 Max (10 logical CPUs, 32 GiB). Per-thread System allocator
counts are cumulative requests, not RSS. Current baseline, not the older review
baseline, is used for the comparison:

| Interface methods | Baseline allocations / median | ID01 allocations / median |
| --- | --- | --- |
| 1 | 109,106 / 7.464 ms | 41,106 / 4.862 ms |
| 8 | 277,365 / 13.747 ms | 41,365 / 4.966 ms |
| 32 | 854,253 / 35.065 ms | 42,253 / 4.913 ms |

The remaining width difference includes construction; the separate existing-view
regression above has identical allocation counts and bytes. Logs/probe workloads
are under target/interface-dispatch. Runtime/VM all-target Clippy, format/diff and
structure (649 Rust files, no violations/exceptions) pass. An initial Clippy
large-enum warning was resolved by keeping bound receiver data separately from
the small selection enum; no extra Box or lint suppression was introduced.
No build/test error is carried. ID02 owns receiver/signature preparation reuse,
inherited views and final integration; its scope has not expanded.
