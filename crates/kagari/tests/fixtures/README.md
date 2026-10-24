# SDK feature-boundary artifact

Ordinary file-backed language inputs live under [`language`](language), grouped
by their owning language contract (types/dispatch, collections/iteration, operators,
traits, Result/Try, strings and numerics).
Their directory-discovered cases keep route/result/GC configuration in the existing
Rust owner; boundary scenarios retain explicit assertions. The default-method
cursor input reuses `iterable-default-cursor.kgr` rather than copying it. Try
execution directly selects the canonical public example. The `language_contract`
subtree holds source data for explicit effect/trap/cancellation scenarios, not a
second runner. Numeric predicates stay generated from bounded Rust tables; only
useful fixed programs are extracted.
Host/async source data lives under [`embedding`](embedding), with fresh provider
installation, scheduling/borrow/reentry and cleanup assertions retained in Rust.
Syntax-example and stateful scenario inputs also live under
[`test-fixtures/language`](test-fixtures/language) and
[`test-fixtures/embedding`](test-fixtures/embedding). They are source data loaded
by the existing library test harness, not additional integration targets. Rust
owns expected values, route/GC configuration, traps, publication, alias checks and
resource cleanup. Preserve logical source names and exact fixture bytes, including
leading newlines and intentionally missing final newlines. Local attributes keep
these `.kgr` inputs LF-only on Windows. See the [testing guide](../../../../docs/testing.md#existing-file-backed-patterns).

## Feature-boundary artifact

`target/fixtures/feature_artifact.kbc` is the disposable current-format artifact emitted from the exact
`feature_artifact.kgr` text with source name `memory://feature-artifact.kgr`.
It contains bytecode, carried declarations/dependencies and portable MIR, without
source text. All four standalone SDK feature routes consume the same bytes:
artifact-only, `source`, `native`, and `source,native`.

`main` remains the two-step scalar function used by both the trusted static ABI
fixture and real Cranelift compilation. Keep heap operations and calls out of this
entry: the static backend accepts only its exact constant/return contract.
`declaration_patterns` separately exercises nested generic struct/tuple bindings,
explicit rest omission and captured mutable leaves.
`default_update_values` separately exercises ordinary core scalar, Vec and Option
Default calls (including Vec/Option elements without Default), a scripted generic
Default implementation, and contextual Default in a base-only struct update.
An overriding update checks independent outer scalar slots, shared inherited Vec
storage, continued base usability and fresh storage from separate Default calls.
`transparent_alias_values` imports renamed reexports of closed generic struct and
enum aliases. Construction through the alias and target-typed function calls preserve
one shared mutable object, including through an enum payload; payload and empty
variants retain the target enum identity. Aliases need no source frontend at load
or execution time.
The portable execution test calls these entries and requires 42 from each,
including in builds without source support. It forces GC at allocation and checks
root/call-depth cleanup and non-quarantined runtime state after each entry.
`library_and_object` exercises trait sorting,
an external native object retaining a callback, and lazy map.
`required_methods` exercises checked MutableList methods, Vec FromIterator,
numeric Sum, FromStr, derived TryInto and primitive From. It also checks all twelve
String methods and String-key sorting through a List result. Source-free execution
forces GC and checks root/object/depth cleanup.
`native_enum_values` exercises nested native-authored enums and pinned payload
layouts. `native_propagation_values` exercises a registered Carrier<T>'s Try and
FromResidual callbacks with a source-defined nested payload. `standard_residual_values`
exercises Option, converted Result errors and ControlFlow breaks. Their already-checked
selected calls execute in every feature combination, including consumers with no
frontend dependency.
Native preparation explicitly falls back before entry for unsupported library calls;
the scalar entry must still execute actual native code.

`artifact_features::portable_fixture_matches_source_emission` checks canonical
bytes against current compilation. Regenerate the fixture after source or contract
changes from the repository root:

```text
cargo run -p kagari --example regenerate_feature_artifact
cargo test -p kagari --test artifact_features portable_fixture_matches_source_emission
cargo test -p kagari --no-default-features --test artifact_features portable_artifact_executes_without_source_compilation
cargo test -p kagari --no-default-features --features native --test artifact_features native_artifact_preparation_and_execution_need_no_frontend
cargo test -p kagari --no-default-features --features native --test artifact_features real_cranelift_compiles_portable_artifact_without_source
```

The focused commands preserve both native scalar-entry contracts without running
the complete feature matrix, which remains CI-owned.
Regeneration replaces the current fixture; it does not add a compatibility reader
or relax version checks. Keep canonical-byte, rejection and execution assertions.
