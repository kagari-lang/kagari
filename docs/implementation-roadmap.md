# Kagari Implementation Roadmap

This roadmap is the execution document for bringing Kagari from the current repository snapshot to the production architecture described in `docs/architecture.md`.

Implementation work must follow the specifications, not legacy behavior in the current code.
If current code conflicts with the specs, change the code structurally.
Do not preserve compatibility with incorrect implementation behavior.

Each step is complete only when its scoped work is verified and committed with the step's required conventional commit message.
Each milestone is complete when all of its steps are committed and the milestone acceptance criteria pass.

## Global Execution Rules

- Use `docs/spec/` and `docs/kagari.ebnf` as the source of truth.
- Keep code structure clear and structural; do not patch around legacy forms.
- Remove incompatible legacy behavior instead of supporting both old and new models.
- Keep public Rust APIs narrow and named around Kagari concepts, not temporary implementation details.
- Add focused tests for accepted programs, rejected programs, lowering, runtime behavior, and reload behavior.
- Before committing a step, run the relevant package tests from that milestone plus `git diff --check`.
- Run the milestone verification commands when the final step in a milestone is complete.
- Commit every completed step immediately.
- Keep the working tree small; do not accumulate a whole milestone before committing.

## Milestone 1: Spec-Aligned Syntax and AST

Intent:

- Make source parsing match `docs/spec/syntax.md` and `docs/kagari.ebnf`.
- Remove old surface syntax from lexer, parser, AST, and parser tests.

Required code areas:

- `crates/kagari-syntax`
- syntax tests
- parser diagnostics
- README examples if they depend on old syntax

Spec references:

- `docs/spec/syntax.md`
- `docs/kagari.ebnf`
- `docs/spec/modules.md`
- `docs/spec/traits.md`

Implementation tasks:

- Add `val` and `var` tokens and binding AST nodes.
- Remove source-language `let`, `let mut`, `ref` parameters, `mut self`, `ref self`, script `static`, and script `dyn Trait`.
- Require struct fields to use `val` or `var`.
- Parse unit type/value as `()`.
- Parse trait names as ordinary type paths.
- Keep top-level `val` and `var` as module initialization statements, not public items.
- Update syntax tests to use spec-valid programs only.
- Add rejection tests for removed legacy forms.

Execution steps:

- M1.1 Add `val` / `var` lexer tokens, keyword classification, and token tests.
  Commit: `feat(syntax): add val var tokens`
- M1.2 Implement parser and AST support for `val` / `var` local bindings, top-level bindings, and struct fields.
  Commit: `feat(syntax): parse val var bindings`
- M1.3 Remove legacy parser support for source `let`, `let mut`, `ref` parameters, receiver modifiers, script `static`, and script `dyn Trait`.
  Commit: `fix(syntax): reject legacy source forms`
- M1.4 Update syntax fixtures, parser diagnostics, and conformance tests for spec-valid grammar.
  Commit: `test(syntax): cover spec grammar conformance`

Forbidden shortcuts:

- Do not parse old forms and lower them into new forms.
- Do not keep `static` grammar as hidden compatibility.
- Do not treat `dyn Trait` as a synonym for interface types.

Acceptance criteria:

- Valid examples from syntax, modules, traits, and typed path specs parse.
- Old `let mut`, `static mut`, `ref` parameter, `mut self`, and `dyn Trait` source forms are rejected.
- AST naming reflects `val` / `var` and field writeability.
- Parser diagnostics explain the supported syntax.

Verification:

```sh
cargo test -p kagari-syntax
cargo test --workspace
git diff --check
```

Milestone completion:

- All M1 step commits are present.
- The milestone verification commands pass on a clean working tree.

## Milestone 2: HIR, Resolver, and Type System Alignment

Intent:

- Make semantic analysis match the source language and type-system specs.
- Remove semantic concepts that only exist for legacy syntax.

Required code areas:

- `crates/kagari-hir`
- resolver and type checker tests
- builtin type definitions
- semantic diagnostics

Spec references:

- `docs/spec/syntax.md`
- `docs/spec/traits.md`
- `docs/spec/modules.md`
- `docs/spec/reflection.md`

Implementation tasks:

- Represent local bindings and fields with `val` / `var` writeability.
- Treat function parameters as non-rebindable ordinary bindings.
- Remove HIR static item support from the script language surface.
- Preserve `const` as compile-time value items.
- Implement field assignment checks: `val` fields reject assignment, `var` fields allow assignment subject to type and policy.
- Implement trait declarations, impls, bounds, and interface type use without script-level `dyn`.
- Implement interface compatibility validation for callable trait methods.
- Preserve concrete type identity for `is<T>` and `downcast<T>` planning.
- Update diagnostics and tests to reflect spec terms.

Execution steps:

- M2.1 Replace semantic binding and field mutability structures with `val` / `var` writeability.
  Commit: `feat(hir): model val var writeability`
- M2.2 Align resolver namespaces for locals, fields, consts, functions, traits, impls, modules, and top-level initialization bindings.
  Commit: `feat(hir): align resolver namespaces`
- M2.3 Enforce parameter, local, field, and const writeability rules in type checking.
  Commit: `feat(typeck): enforce writeability rules`
- M2.4 Implement trait, impl, bound, interface-type, and interface-compatibility validation without script-level `dyn`.
  Commit: `feat(typeck): add trait interface validation`
- M2.5 Update semantic diagnostics and negative tests for removed legacy constructs and invalid assignments.
  Commit: `test(hir): cover semantic conformance`

Forbidden shortcuts:

- Do not keep separate Rust-like mutability and Kagari writeability models.
- Do not allow function parameter rebinding.
- Do not type reflective field mutation as the ordinary assignment path.

Acceptance criteria:

- HIR contains no source-language `let mut`, script `static`, or script `dyn` concepts.
- Resolver correctly separates locals, fields, consts, functions, traits, impls, and modules.
- Type checker enforces binding, field, function, trait, interface, and const rules.
- Negative tests cover removed legacy constructs and invalid assignments.

Verification:

```sh
cargo test -p kagari-hir
cargo test --workspace
git diff --check
```

Milestone completion:

- All M2 step commits are present.
- The milestone verification commands pass on a clean working tree.

## Milestone 3: Typed IR and Bytecode Rebuild

Intent:

- Rebuild executable intermediate forms around spec-level operations.
- Make bytecode a register/local semantic contract for the interpreter and JIT.

Required code areas:

- `crates/kagari-ir`
- IR and bytecode tests
- lowering from analyzed HIR

Spec references:

- `docs/spec/bytecode.md`
- `docs/spec/typed-path-mutation.md`
- `docs/spec/codegen-backend.md`
- `docs/spec/jit.md`

Implementation tasks:

- Normalize typed IR around explicit control flow, typed values, and effect metadata.
- Keep bytecode register/local based.
- Add or finalize bytecode tables for constants, types, functions, public items, paths, and metadata.
- Lower ordinary script aggregate access separately from host-backed typed path access.
- Add typed path instructions or strongly typed helper calls for path read, set, modify, and view.
- Remove ordinary assignment lowering through reflection helpers.
- Add verification for local/field writeability, type consistency, path descriptors, helper calls, and control-flow targets.
- Preserve debug spans and source mapping.

Execution steps:

- M3.1 Normalize typed IR around explicit control flow, typed operands, values, locals, and effect metadata.
  Commit: `feat(ir): normalize typed control flow`
- M3.2 Add bytecode tables, metadata records, and verifier checks for register/local bytecode.
  Commit: `feat(bytecode): add verified metadata tables`
- M3.3 Separate ordinary aggregate field/index access from host-backed typed path access in IR and bytecode.
  Commit: `feat(ir): separate aggregate and path access`
- M3.4 Remove ordinary assignment lowering through reflection helpers.
  Commit: `fix(ir): remove reflection assignment lowering`
- M3.5 Add verifier, lowering, source-span, and effect metadata tests.
  Commit: `test(ir): cover bytecode verifier contracts`

Forbidden shortcuts:

- Do not use runtime string field lookup for ordinary compiled field access.
- Do not treat typed path mutation as reflection.
- Do not encode Cranelift-specific objects in Kagari IR.

Acceptance criteria:

- IR and bytecode can represent all accepted milestone 1-2 syntax.
- Bytecode verifier rejects malformed control flow, type mismatches, invalid writes, and unresolved paths.
- Typed path descriptors are resolved before execution.
- Interpreter and future JIT have enough metadata for effects, safepoints, and reload validation.

Verification:

```sh
cargo test -p kagari-ir
cargo test --workspace
git diff --check
```

Milestone completion:

- All M3 step commits are present.
- The milestone verification commands pass on a clean working tree.

## Milestone 4: Runtime, Value Model, and GC Foundation

Intent:

- Establish the production runtime substrate shared by the interpreter and JIT.

Required code areas:

- `crates/kagari-runtime`
- runtime-facing value APIs
- runtime tests

Spec references:

- `docs/spec/runtime.md`
- `docs/spec/reflection.md`
- `docs/spec/modules.md`
- `docs/spec/security.md`

Implementation tasks:

- Define storable values, ephemeral values, host handles, interface values, path views, and unit.
- Implement explicit roots for host-retained Kagari values.
- Implement GC object identity and root scanning boundaries.
- Add module store with module id, epoch, module instances, and initialization result state.
- Add runtime metadata registries for types, fields, methods, interfaces, and public ABI fingerprints.
- Add resource accounting and runtime error categories.
- Ensure Rust host objects are never traced as Kagari heap objects.

Execution steps:

- M4.1 Define production runtime value categories: storable values, ephemeral values, host handles, interface values, path views, and `()`.
  Commit: `feat(runtime): define production value categories`
- M4.2 Implement explicit roots, GC object identity, and root scanning boundaries.
  Commit: `feat(runtime): add explicit gc roots`
- M4.3 Add module store structures for module ids, epochs, instances, initialization state, and initialization results.
  Commit: `feat(runtime): model module epochs`
- M4.4 Add runtime metadata registries, resource accounting, and runtime error categories.
  Commit: `feat(runtime): add metadata and resource state`
- M4.5 Add runtime tests for value categories, roots, module epochs, metadata, and host-object GC boundaries.
  Commit: `test(runtime): cover runtime substrate`

Forbidden shortcuts:

- Do not let host references become ordinary storable GC values.
- Do not make `const` heap-backed frozen objects.
- Do not couple runtime values to AST nodes.

Acceptance criteria:

- Runtime value categories match the specs.
- GC roots are explicit and testable.
- Module epochs and initialization state are represented independently from parser/HIR structures.
- Runtime metadata can serve reflection, reload validation, typed path validation, and JIT metadata.

Verification:

```sh
cargo test -p kagari-runtime
cargo test --workspace
git diff --check
```

Milestone completion:

- All M4 step commits are present.
- The milestone verification commands pass on a clean working tree.

## Milestone 5: Host Interop and Typed Path Mutation

Intent:

- Implement safe Rust host integration and ergonomic typed path mutation.

Required code areas:

- `crates/kagari-runtime`
- `crates/kagari-ir`
- `crates/kagari-vm`
- host interop tests

Spec references:

- `docs/spec/host-interop.md`
- `docs/spec/typed-path-mutation.md`
- `docs/spec/security.md`
- `docs/spec/reflection.md`

Implementation tasks:

- Implement host type and function registration metadata.
- Implement frame-scoped host borrow tokens for temporary `&T` and `&mut T` host calls.
- Enforce no-escape rules for host borrows.
- Implement typed host roots, typed path descriptors, host path views, and dynamic path arguments.
- Implement path read, set, modify, and view operations.
- Add host-side validation, dirty tracking hooks, and failure classification.
- Make typed path mutation reload-aware through descriptor fingerprints.
- Keep reflection separate from ordinary host-backed mutation.

Execution steps:

- M5.1 Implement host type and function registration metadata.
  Commit: `feat(host): add host registration metadata`
- M5.2 Implement frame-scoped host borrow tokens and no-escape validation.
  Commit: `feat(host): enforce frame scoped borrows`
- M5.3 Implement typed host roots, typed path descriptors, host path views, and dynamic path arguments.
  Commit: `feat(host): add typed path views`
- M5.4 Execute path read, set, modify, and view operations with validation, dirty hooks, and failure classification.
  Commit: `feat(host): execute typed path mutations`
- M5.5 Add host interop tests for path policy, stale roots, dynamic indexes, denied capabilities, and reload fingerprints.
  Commit: `test(host): cover path policy and reload guards`

Forbidden shortcuts:

- Do not store Rust `&mut` references in script values.
- Do not model local nested host values as detached Rust references.
- Do not let reflective writes bypass typed path policy.

Acceptance criteria:

- Scripts can use field/index syntax over host-backed roots through typed path operations.
- Multiple local nested views can coexist without Rust borrow-checker semantics leaking into scripts.
- Host borrow tokens cannot escape calls or suspension boundaries.
- Dirty tracking and validation hooks run in the defined order.
- Invalid roots, stale epochs, invalid dynamic indexes, and denied capabilities produce runtime errors.

Verification:

```sh
cargo test -p kagari-runtime
cargo test -p kagari-vm
cargo test --workspace
git diff --check
```

Milestone completion:

- All M5 step commits are present.
- The milestone verification commands pass on a clean working tree.

## Milestone 6: Production Interpreter

Intent:

- Make the bytecode VM a production semantic baseline.

Required code areas:

- `crates/kagari-vm`
- `crates/kagari-runtime`
- integration tests

Spec references:

- `docs/spec/execution.md`
- `docs/spec/bytecode.md`
- `docs/spec/runtime.md`
- `docs/spec/modules.md`

Implementation tasks:

- Execute verified bytecode only.
- Implement deterministic call frames, local/register storage, control flow, returns, traps, and helper calls.
- Enforce resource accounting and security context at runtime boundaries.
- Execute module initialization once per epoch and cache success or failure according to spec.
- Preserve stack/root metadata for GC.
- Add interpreter tests for functions, modules, loops, match, arrays, structs, interface calls, path mutation, errors, and reload boundaries.

Execution steps:

- M6.1 Require verified bytecode before VM execution and reject unsupported bytecode.
  Commit: `feat(vm): require verified bytecode`
- M6.2 Implement deterministic frames, local/register storage, control flow, calls, returns, and traps.
  Commit: `feat(vm): implement deterministic frames`
- M6.3 Enforce runtime helper, resource accounting, security, host, reflection, and path boundaries.
  Commit: `feat(vm): enforce runtime boundaries`
- M6.4 Implement module initialization and epoch-visible execution behavior.
  Commit: `feat(vm): execute module epochs`
- M6.5 Add interpreter integration tests for successful execution and classified failure paths.
  Commit: `test(vm): cover interpreter conformance`

Forbidden shortcuts:

- Do not execute unverified bytecode.
- Do not special-case source-level constructs in the VM.
- Do not ignore resource or capability checks for tests.

Acceptance criteria:

- VM executes the full spec-aligned bytecode subset.
- Runtime errors are stable and classified.
- Module initialization and reload-visible behavior match module specs.
- Interpreter tests cover success and failure paths.

Verification:

```sh
cargo test -p kagari-vm
cargo test --workspace
git diff --check
```

Milestone completion:

- All M6 step commits are present.
- The milestone verification commands pass on a clean working tree.

## Milestone 7: Hot Reload Production Pass

Intent:

- Make reload validation and publication safe enough for long-running host applications.

Required code areas:

- `crates/kagari-runtime`
- `crates/kagari-ir`
- `crates/kagari-vm`
- reload tests

Spec references:

- `docs/spec/modules.md`
- `docs/spec/runtime.md`
- `docs/spec/bytecode.md`
- `docs/spec/typed-path-mutation.md`
- `docs/spec/jit.md`

Implementation tasks:

- Add ABI fingerprints for public functions, consts, types, traits, interface tables, and typed path descriptors.
- Validate new module epochs before publication.
- Preserve active module on validation failure.
- Keep old metadata reachable while old values, calls, or compiled artifacts need it.
- Make new calls use the latest successfully published epoch.
- Invalidate interpreter caches and JIT artifacts when dependency fingerprints change.
- Add tests for compatible reloads, incompatible reloads, active-call stability, path descriptor changes, and failure rollback.

Execution steps:

- M7.1 Add ABI fingerprints for public functions, consts, types, traits, interface tables, and typed path descriptors.
  Commit: `feat(reload): add abi fingerprints`
- M7.2 Validate new module epochs before publication and preserve the active module on failure.
  Commit: `feat(reload): validate module publication`
- M7.3 Preserve old epochs and metadata while old values, calls, or compiled artifacts need them.
  Commit: `feat(reload): preserve reachable epochs`
- M7.4 Invalidate interpreter caches and JIT artifacts when reload dependencies change.
  Commit: `feat(reload): invalidate stale artifacts`
- M7.5 Add reload tests for compatible reloads, incompatible reloads, active-call stability, path descriptor changes, and failure rollback.
  Commit: `test(reload): cover safe reload behavior`

Forbidden shortcuts:

- Do not silently migrate script-visible module storage.
- Do not publish partially validated modules.
- Do not let stale compiled artifacts run after invalidation.

Acceptance criteria:

- Reload success and failure behavior is deterministic.
- ABI incompatibilities are diagnosed.
- Existing calls and values remain tied to valid old epochs.
- Failed reload cannot corrupt active runtime state.

Verification:

```sh
cargo test --workspace reload
cargo test --workspace
git diff --check
```

Milestone completion:

- All M7 step commits are present.
- The milestone verification commands pass on a clean working tree.

## Milestone 8: Security Profiles and Reflection Gates

Intent:

- Enforce security policy as a layered runtime and compile-time system.

Required code areas:

- `crates/kagari-runtime`
- `crates/kagari-hir`
- `crates/kagari-vm`
- CLI profile plumbing if needed

Spec references:

- `docs/spec/security.md`
- `docs/spec/reflection.md`
- `docs/spec/host-interop.md`
- `docs/spec/traits.md`

Implementation tasks:

- Implement language profiles for feature availability.
- Implement runtime capabilities for reflection, host calls, path mutation, module loading, and JIT.
- Enforce host API exposure policy.
- Gate reflection metadata reads, reflective reads, reflective writes, dynamic invocation, and downcasts separately.
- Add resource limits for instruction count, call depth, allocation, host calls, and reflection operations.
- Ensure runtime checks remain active even when frontend checks reject obvious violations.

Execution steps:

- M8.1 Implement language profiles and runtime capabilities for reflection, host calls, path mutation, module loading, and JIT.
  Commit: `feat(security): add profiles and capabilities`
- M8.2 Enforce host API exposure policy at runtime entry points.
  Commit: `feat(security): enforce host exposure policy`
- M8.3 Gate reflection metadata reads, reflective reads, reflective writes, dynamic invocation, and downcasts separately.
  Commit: `feat(security): gate reflection operations`
- M8.4 Enforce runtime resource limits for instruction count, call depth, allocation, host calls, and reflection operations.
  Commit: `feat(security): enforce runtime resource limits`
- M8.5 Add tests for restricted profiles, denied operations, reflection gates, and resource limit failures.
  Commit: `test(security): cover denied operations`

Forbidden shortcuts:

- Do not let embeddings redefine core grammar.
- Do not make reflection a backdoor around host exposure or typed path policy.
- Do not rely only on compile-time checks for runtime capabilities.

Acceptance criteria:

- Restricted profiles can disable reflection, host calls, path mutation, module loading, or JIT independently.
- Denied operations fail with classified runtime errors.
- Reflection metadata access and reflective mutation are separately gated.
- Resource limits are enforced in interpreter execution.

Verification:

```sh
cargo test --workspace security
cargo test --workspace reflection
cargo test --workspace
git diff --check
```

Milestone completion:

- All M8 step commits are present.
- The milestone verification commands pass on a clean working tree.

## Milestone 9: Baseline Cranelift JIT

Intent:

- Add an optional baseline Cranelift backend that preserves interpreter semantics.

Required code areas:

- new backend crate or module for Cranelift integration
- `crates/kagari-ir`
- `crates/kagari-runtime`
- `crates/kagari-vm` dispatch integration
- JIT tests

Spec references:

- `docs/spec/jit.md`
- `docs/spec/codegen-backend.md`
- `docs/spec/execution.md`
- `docs/spec/bytecode.md`

Implementation tasks:

- Add backend boundary trait for executable function artifacts.
- Add optional Cranelift dependency behind a feature or backend crate.
- Compile eligible functions from typed IR or verified bytecode-like IR.
- Call shared runtime helpers for allocation, host interop, path mutation, reflection, traps, security, and safepoints.
- Emit or derive stack maps and safepoint metadata.
- Fall back to interpreter for unsupported functions.
- Invalidate compiled artifacts on module epoch, ABI fingerprint, path descriptor, type layout, helper ABI, or policy changes.
- Add equivalence tests comparing interpreter and JIT results.

Execution steps:

- M9.1 Add the backend boundary trait and executable artifact registry without Cranelift-specific types in core IR.
  Commit: `feat(jit): add backend artifact boundary`
- M9.2 Add optional Cranelift integration behind a feature or backend crate.
  Commit: `feat(jit): wire optional cranelift backend`
- M9.3 Compile eligible baseline functions from typed IR or verified bytecode-like IR and call shared runtime helpers.
  Commit: `feat(jit): compile baseline functions`
- M9.4 Add safepoint metadata, stack-map handling, interpreter fallback, and unsupported-function diagnostics.
  Commit: `feat(jit): add safepoints and fallback`
- M9.5 Add JIT equivalence, policy disablement, fallback, and reload invalidation tests.
  Commit: `test(jit): cover equivalence and invalidation`

Forbidden shortcuts:

- Do not make Cranelift types part of Kagari IR.
- Do not bypass runtime helpers for host or security operations.
- Do not require deoptimization or optimizing-tier machinery.

Acceptance criteria:

- JIT can be enabled or disabled by runtime policy.
- Eligible functions run through compiled code and match interpreter results.
- Unsupported functions fall back transparently.
- Reload invalidation prevents stale machine code from running.
- GC safepoint/root metadata is tested.

Verification:

```sh
cargo test --workspace --features jit
cargo test --workspace
git diff --check
```

Milestone completion:

- All M9 step commits are present.
- The milestone verification commands pass on a clean working tree.

## Milestone 10: Production Hardening

Intent:

- Finish production-readiness work across tooling, tests, diagnostics, and documentation.

Required code areas:

- all crates
- README and docs
- test harnesses
- CLI

Spec references:

- all files under `docs/spec/`
- `docs/architecture.md`
- this roadmap

Implementation tasks:

- Add conformance tests organized by language feature and spec section.
- Add negative tests for removed legacy behavior.
- Add integration tests for host interop, reload, security, reflection, and JIT policy.
- Add fuzz or property tests for lexer/parser and bytecode verifier where practical.
- Improve diagnostics for parse, resolution, type, bytecode verification, runtime, host, reload, and security errors.
- Update CLI commands for parse/check/run and optional JIT execution.
- Update README and docs to match final behavior.
- Remove dead compatibility code and obsolete tests.

Execution steps:

- M10.1 Add conformance and negative tests organized by language feature and spec section.
  Commit: `test: add language conformance suite`
- M10.2 Improve parse, resolution, type, bytecode verification, runtime, host, reload, and security diagnostics.
  Commit: `feat(diagnostics): improve compiler runtime errors`
- M10.3 Update CLI commands for parse, check, run, profile selection, and optional JIT execution.
  Commit: `feat(cli): polish language pipeline commands`
- M10.4 Update README, architecture, roadmap, goal guide, and specs to match implemented behavior.
  Commit: `docs: align documentation with implementation`
- M10.5 Remove dead compatibility code, obsolete tests, and non-spec examples.
  Commit: `chore: remove legacy compatibility code`

Forbidden shortcuts:

- Do not leave old behavior documented as deprecated compatibility.
- Do not hide failing edge cases behind ignored tests.
- Do not ship examples that use non-spec syntax.

Acceptance criteria:

- `cargo test --workspace` passes.
- Feature-gated JIT tests pass when enabled.
- README examples use only spec-valid Kagari.
- Specs, architecture, and implementation roadmap agree with the code.
- Public APIs are documented enough for host embedding examples.
- No known legacy syntax remains accepted unless explicitly present in the specs.

Verification:

```sh
cargo test --workspace
cargo test --workspace --features jit
git diff --check
rg -n 'let mut|static mut|dyn Trait|ref self|mut self|HostBorrowHandle' crates README.md docs/spec docs/kagari.ebnf
```

Milestone completion:

- All M10 step commits are present.
- The milestone verification commands pass on a clean working tree.
