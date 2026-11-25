# HIR declaration inputs: implementation plan

**Status: Not started.** Planning baseline: `f8c3a20d6` on `design/hir-native-model`. The [design](hir-declaration-inputs-design.md) owns the target Rust types, field meanings and semantics. This plan owns the order of work, checkpoint evidence and remaining blockers. The [roadmap](implementation-roadmap.md#native-model-replacement) points here. Existing contracts apply until their replacement passes its gate.

## Goal handoff

> Implement the HIR declaration input design in phases 1–7 below. Start with the first unfinished phase. Preserve unrelated changes. Validate and record each checkpoint here, then continue without waiting for approval. Request overall acceptance only after all phases, or report a concrete blocker if progress would require changing semantics or weakening safety. Do not commit unless the Goal explicitly authorizes commits.

The compiler gets user and core `.kgr` plus scoped Host `ModuleDecl` records. The editor gets user, core and generated Host `.kgr`, with no registrations or semantic attachments. Each logical module supplies one `LoweredModule` to the same graph, resolver and type checker. A declaration need not have an installed Rust implementation to be analyzed. A source-free executable still needs independent validation to load.

Do not introduce a third offline frontend, binary declaration prebuild, discovery protocol, file watcher, new query framework or callback-bearing HIR. Do not change reload call selection, redesign general Host borrowing or bump an unpublished ABI as a migration shortcut. Invalidate affected cached products instead.

## 1. Establish the HIR join point — Not started

**Owners:** [declaration preparation](../crates/kagari-hir/src/analysis/declarations.rs), [structured lowering](../crates/kagari-hir/src/native/structured.rs) and [module scopes](../crates/kagari-hir/src/imports/environment.rs). No SDK, execution or live-editor cutover required yet.

- Keep `declarations(source_snapshot, cancel)` as the query. Gather bundled core once, lower real documents (including inline modules) with their syntax and ranges, and lower registrations directly after adopting their definition tables. A structured origin has a graph identity, not a fabricated source span. Use the existing single graph after collection; provenance must not select another resolver or checker.
- Normalize package aliases, explicit dependency edges and preludes per importer before graph construction. Preserve core ownership, rejection of competing library owners, project ambiguity diagnostics, cancellation, limits and immutable replay. Preserve the old public input routes for consumers not yet migrated; do not add another one.
- **Gate:** Analyze `score() -> i32` calling a registered `game::api::number() -> i32` in one request, then the same call using its generated declaration plus user text and **zero registrations** in another. Assert the selected canonical module/member and result type; `number(1)` must diagnose arity in both. Assert the editor definition range selects the exact `number` token and structured analysis does not invent one. Compare identities by owner and member, never session-local integers. This proves the join point, not yet parity for every Host API.

## 2. Separate SDK compilation from declaration publication — Not started

**Depends on 1. Owners:** [SDK assembly](../crates/kagari/src/engine/declarations.rs), declaration renderer, source capture and manifest consumers.

- Compile registrations structurally whether or not `declaration_cache` is enabled. Render only for publication or an explicit tooling request. Deliver generated text with its stable document key, logical module identity, display path and import configuration; do not ship HIR IDs or selectors. Keep real Script bodies/defaults complete; move an unrepresentable body to authored source rather than reading it back from a cache.
- Render names in the *importing module's* effective scope. Check both canonical target and namespace of re-exports. If there is no unambiguous legal spelling, report an export error; do not silently emit another declaration's name.
- **Gate:** Two packages export `Token`; the generated import/re-export selects the same `Token` as registration even when aliases differ by importer. Exercise a Script default through its generated body and structured counterpart. With cache off/on, compile the same registration with the same semantics. For a program containing a structured-only module, publication must not manufacture a complete source manifest: verify `MissingManifest` for the operation requiring it, and that a whole-file update supported without it still works.

## 3. Give foreign traits one owner and preserve load checks — Not started

**Depends on 1–2. Owners:** [`ModuleDecl`](../crates/kagari-types/src/declaration/module.rs), [builder](../crates/kagari-runtime/src/native/builder/implementation.rs), HIR impl checking, [compiler ABI emission](../crates/kagari-compiler/src/source/lower/abi.rs) and provider installation/linking. Migrate these consumers as one checkpoint.

- Record `ImplDecl.trait_type` and the impl's **own** `FnDecl`s (explicit or from a typed adapter). Once both modules are present, resolve the reachable, visible trait owner and compare its checked signature with the impl, including substituted bounds, associated members and defaults. Do not synthesize the impl signature from the expected trait.
- Emit portable interface requirements from checked owners and actual uses, including bounds and Native callable operations. Preserve independently established installed-provider requirements, including mandatory identities. Only then remove the builder's foreign `TraitDef` input, both `trait_references` fields and `registered_trait_access`.
- **Gate:** On both routes `impl Health for Player` agrees with a source-owned `Health.hp -> i32`; changing **only** the owner to return `String` yields a signature mismatch. An inaccessible trait fails visibility, not parsing. At [source-free linking](../crates/kagari/src/runtime/tests/embedding/offline_nominal/traits.rs), a matching installation loads; missing requirements, a mismatched owner, or an added default method against an unchanged installed expectation do not. Never derive both sides of that comparison from the artifact being tested. If independent evidence cannot be obtained, keep the load checks and report the blocker.

## 4. Separate field selection from Host execution — Not started

**Depends on 1–3. Owners:** [HIR field checking](../crates/kagari-hir/src/typeck/body/host_access.rs), HIR member signatures/type tables, compiler place lowering, Host registration, adapters and runtime.

- HIR selects a field by its declaration, visibility, type and `val`/`var`; it records field IDs for reads and writes. Method lookup sees Kagari `self`, not the Rust adapter's loan. Move schema, adapter compatibility, short loans and path operations to their registration/link/execution owners. Compile the checked field IDs; do not resolve names again. Virtual members/indexes need a visible declaration or an explicit method.
- **Gate:** On both routes, a `val baseline: Stats` allows writing `player.baseline.hp` when `Stats.hp` is `var`, not replacing `baseline`. A `val hp` is not writable via either `baseline` or `current`. Exercise a successful real write and reject an adapter that cannot supply its declaration; do not upgrade a read-only runtime view. Preserve the [mixed dynamic-index/virtual-member scenario](../crates/kagari/src/runtime/tests/embedding/offline_nominal/paths.rs), including source-free load, once-only left-to-right evaluation, trap order and previously committed effects. Retire HIR Host path outputs and receiver-loan policy only with their checked replacement and migrated consumers.

## 5. Prove async retention safe before relaxing HIR — Not started

**Depends on 4. Owners:** [Host registry](../crates/kagari-runtime/src/host/registry.rs), [value storage](../crates/kagari-runtime/src/value/properties.rs), async runtime, MIR/bytecode verifiers and HIR capture checks.

- Distinguish a GC-managed Native value or durable Host handle from an active Rust loan, mutation guard or restricted path view. Start with non-reused object IDs per registry. A retired ID never names another object; a schema epoch is not an object generation. On access, validate the handle and obtain a fresh short loan. Never park a Rust reference. Do not make all path views or tuples storable to implement this.
- **Gate:** Test cold Future capture and actual suspension/resumption: a rooted managed object survives, a live handle reads the intended player, and retiring A and creating B during the wait makes access to A fail before touching B. Reject an active loan before it escapes or parks; verify roots/guards are released on cancellation and trap. Keep independent MIR/bytecode and malformed source-free artifact checks. Only with that evidence remove `contains_host_value`-based blanket rejection and verify both frontend routes agree on the async script. Preserve normal capture and `await` typing.

## 6. Deliver snapshots without losing editor state — Not started

**Depends on 1–2; schedule after 5. Owners:** editor workspace/analyzer and [source database](../crates/kagari-source/src/source_database.rs), not HIR.

- Keep disk base, unsaved overlays and read-only generated documents separate. Merge them at the workspace boundary into one immutable `SourceSnapshot`; use no registrations in the analyzer. Do not refresh the live editor by calling the whole-batch `replace_captured` with the generated set.
- **Gate:** Edit a user file without saving, refresh changed Host exports, then close the user buffer. The edit survives refresh, the new snapshot navigates to the updated export, closing restores disk text, and a retained snapshot still navigates to its old file and range. No filesystem watcher or new protocol is required to exercise this sequence.

## 7. Retire old input routes and reconcile contracts — Not started

**Depends on 1–6. Owners:** HIR inputs/replay, core, SDK and tooling callers, affected docs and fixtures.

- Audit every row in the design's [field disposition table](hir-declaration-inputs-design.md#where-each-externalmodule-field-goes) against a replacement consumer and observable evidence. In particular: only canonical `Result::Err` creates the designated failure origin and forwarding still works; reserved constructors require the correct owner and shape; namespace re-exports, conversions, final defaults, callable requirements and concrete results retain caller-visible behavior on both routes. Keep execution-only evidence with the compiler/linker. If a distinction cannot yet be expressed in source, fix its owner or report a blocker **before** deleting its route.
- Remove `ExternalModule`, the mixed `LibrarySources` inventory, `native_sources`, `set_external_modules`, `set_native_sources` and obsolete paired-input tests/fingerprints. Replace them with the design's `SourceSnapshot` and `AnalysisInputs`, not renamed attachment maps. Derive replay/cache identity from actual core documents/owners, scoped declarations, import selections, limits and source revisions. Preserve independent registration, artifact and navigation checks.
- **Gate:** Trace one compiler request and one zero-registration editor request end to end. Verify no production callers of the old inputs remain and changed inputs invalidate affected products without changing retained snapshots. Bring the HIR guide, language/embedding specs, SDK examples and affected fixtures into line. At closure transfer lasting contracts to their owners, move genuine residual work to the roadmap/review, and retire the temporary planning documents per repository policy.

## Checkpoints and overall acceptance

At each phase inspect status and relevant diffs; prefer strengthening a test at the contract owner. Run focused package/target/name tests for the affected scenarios, plus `uv run --locked scripts/check_structure.py`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `git diff --check` for implementation checkpoints. Record the actual commands/results, any specific failure and its owner, and the next unfinished phase here. A phase is not done while its safety or integration gate is red. Do not repeat unrelated known failures as if they were new evidence; historical stdlib collection failures are not findings of this plan unless reproduced.

For final review, report the compiler/editor walkthrough, what each gate actually proved, local results, unresolved failures and pending CI separately. Full-workspace tests and exhaustive feature/backend/platform matrices belong to CI, not a local Goal run. The user reviews the integrated result once; passing the first HIR test does not establish Host, async, link or editor correctness.
