# Cranelift AOT deployment plan

Status: plan prepared; A01-A04 implementation has not started. The user selected
Cranelift AOT as the next native track and authorized this planning checkpoint.
The [roadmap](implementation-roadmap.md#cranelift-aot-deployment) owns sequencing.
This document owns scope, decisions, acceptance and the implementation ledger;
proposed contracts below are not existing APIs.

## Outcome and finite scope

Build native code ahead of deployment, then load and execute it in a separate
Kagari host without a source frontend, MIR compiler or Cranelift dependency.
Keep the existing JIT working. Both routes share verified MIR lowering and the
runtime's installation, invocation, cancellation and version-retention contracts.

The first implementation targets x86_64 Windows MSVC on the current development
platform. Build and execution target the same platform, with an explicit baseline
CPU feature set rather than the build machine's detected features. Linux, other
architectures and cross-compilation need later platform acceptance.

Native coverage stays at the existing zero-argument, single-return-block
Unit/Bool/i32 subset: constants, moves, supported checked arithmetic/comparisons,
boolean not and return. The first bundle contains one explicitly selected entry
and its complete bytecode Program, including dependency metadata. Unsupported
entries fail the explicit AOT build before publishing a bundle. A successful build
must contain native code; a bytecode-only output is not AOT success.

Exclude wider language coverage, script calls, parameters, loops, GC-bearing
values, async suspension, native debugging, automatic compilation, persistent
compilation caches, standalone executables, LLVM and package distribution.
The [synchronous expansion plan](cranelift-expansion-plan.md) and call-stack
decision remain deferred until the interpreter is stable. This plan introduces
no continuation protocol or general script-call ABI.

## Baseline and ownership

Baseline: `12eee4da`, inspected on 2026-10-14.

| Owner | Current boundary and required change |
| --- | --- |
| [Codegen interface](../crates/kagari-codegen/src/lib.rs) | Returns live executable products. Add a distinct object-emission contract consuming the same verified MIR; do not serialize a live product. |
| [Cranelift emission](../crates/kagari-codegen-cranelift/src/emit.rs) | Couples scalar lowering to JITModule and process helper addresses. Share lowering through Cranelift's Module abstraction; separate JIT finalization and object output. |
| [Physical ABI](../crates/kagari-abi/src/native.rs) | Owns target, helper declarations and code lifetime. Separate portable helper requirements from process bindings and define the narrow loading/invocation contract. |
| [Executable metadata](../crates/kagari-contract/src/native.rs) | Owns function, safepoint and debug descriptors. Own portable native metadata validation without MIR or source dependencies. |
| [Runtime installation](../crates/kagari-runtime/src/backend/native.rs) | Retains exact loaded versions and executable owners. Reuse this path after resolving a trusted native image; preserve shared session/frame cleanup. |
| [SDK preparation](../crates/kagari/src/program/native.rs) | Existing native preparation depends on compiler/MIR. Add separately gated AOT loading that needs neither, without moving compilation into runtime. |
| [CLI](../crates/kagari-cli/Cargo.toml) | Owns arguments, filesystem IO and diagnostics. Own external linker invocation; SDK owns reusable build/load orchestration. |

Cranelift's [ObjectModule](https://docs.wasmtime.dev/api/cranelift_object/index.html)
emits object files and implements Module. Use the repository's pinned Cranelift
version when implementing; do not assume the latest documentation's API matches.
An object file still requires linking. Use the platform linker and OS dynamic
loader rather than implementing relocation or an object loader in Kagari.

For example, build `fn main() -> i32 { 6 * 7 }` into a bundle. A separate host
verifies its bytecode and deployment metadata, loads its DLL, binds runtime
helpers, installs the exact entry and reports Native with result 42. Overflow
and cancellation still pass through the existing runtime failure/origin path.
This establishes deployment, not support for general business scripts or a
performance advantage over JIT/interpreter execution.

## Required contracts

### Build products and identity

Use a bundle directory containing bytecode (`.kbc`), a native DLL and a bounded
manifest. Object files are build intermediates. The manifest binds the exact
bytecode bytes, native image bytes, selected entry and metadata, target triple,
pointer width, required CPU features, runtime/helper ABI identifiers, backend
configuration and compiler/build-contract identity. Use existing portable identity
and codec conventions; never persist process-local DefinitionId or runtime handles.
Resolve portable module/function references against the loaded verified Program.

Verify MIR and canonical bytecode correspondence on the build machine before
emission. The loader independently verifies bytecode and manifest consistency;
it does not recompile MIR or claim to prove machine-code semantics. AOT bundles
need no portable MIR section for execution. Retain origins needed for failure
traces without requiring source text or source analysis on the server.

Keep format/ABI identifiers under the unpublished development policy. Do not
promise cross-revision compatibility or routinely bump versions. A01 defines an
exact build-contract compatibility check so stale development products are
rejected when unchanged ABI identifiers alone would be insufficient. Regenerate
affected disposable products; do not retain alternate readers or old entry ABIs.

Bound decoded lengths, function/metadata counts and paths before allocation or
loading. Reject missing/duplicate entries, digest mismatches, unsupported format
or build contracts, incompatible targets/CPU features and inconsistent metadata.
Hashes establish correspondence, not trust in the publisher.

### Helper binding and trust

No build-process helper, runtime or executable address may enter persistent
metadata or code as an assumed deployment address. Portable requirements name
the helper and its physical signature; actual bindings belong to execution.

Use an invocation-local ABI context containing an opaque runtime pointer and an
immutable helper table. Generated code calls helpers through that table. JIT and
AOT share the narrow scalar context layout and lowering, with runtime constructing
the context at invocation. This avoids executable-export lookup and mutable
DLL-global bindings when multiple runtimes share a loaded image. The context and
table remain valid through the call; generated code cannot retain their pointers.
A01 defines exact layouts, signatures and ownership before changing emission.
This small scalar transport change does not settle the deferred script-call model.

Loading native code is a trusted-host operation. The public file-loading boundary
must state the unsafe obligations for generated code, image dependencies and
initialization, consistent with the existing unsafe backend/install contracts.
Do not expose arbitrary native file loading as safe merely because a manifest
passes validation. The OS loader may execute initialization code before Kagari
entry resolution. Generated bundles have no script execution during loading;
metadata inspection is not a sandbox for arbitrary DLLs.

Use an explicit absolute bundle location and restricted dependency resolution;
do not discover libraries via the current directory. A01 defines the immutable
deployment-directory contract covering validation through loading so replacement
between checking and loading is not silently treated as safe. Application package
authentication and a general signing/update system remain outside this task.

### Installation, execution and lifetime

SDK loading owns filesystem/OS-loader work in an optional module; contract owns
portable validation; runtime receives resolved products and owns installation.
Keep compiler/backend dependencies out of the loader feature and runtime. Split
feature gates at the actual ownership boundary, without an empty new crate or a
second invocation implementation. The interpreter-only SDK remains usable.

The library owner implements the existing executable-owner lifetime contract.
Every installed/prepared native entry and active invocation retains the library
and its exact Program/dependency versions. Releasing build/preparation objects
cannot invalidate an installed entry. New deployments use distinct immutable
image paths, including on Windows where a loaded DLL cannot be overwritten.
Old entries execute old code after reload; unload only after the last owner drops.
Do not add a process-global native-image cache in this batch.

Validate the complete platform and CPU requirements before native entry; matching
pointer width alone is insufficient. Keep ABI, signature, metadata bounds,
runtime ownership and generation checks. Retain polling, empty scalar root maps,
checked arithmetic, trace origins and cleanup on all exits. Do not add charging.

Explicit AOT loading errors are errors, never silent interpreter fallback.
Existing observer/execution-policy decisions may select the interpreter before
entry, with that route reported explicitly and the bundled bytecode retained.
A failure after native entry never restarts the script. Separate code-generation
policy from permission to execute already installed native code where current
JIT-named policy conflates them; do not add a generic permission framework.

## Phases and acceptance

Keep the tree buildable at each phase. Each phase updates this ledger and records
focused checks; integration failures must be resolved before marking it complete.

### A01 - Portable contracts and scalar binding

- Define the bounded manifest/portable descriptors, exact compatibility rule,
  entry identity, target checks and trusted immutable loading boundary above.
- Separate emitted objects from live products and helper requirements from
  addresses. Define and implement the shared scalar invocation context.
- Adapt existing JIT callers coherently; preserve JIT results, overflow/origins,
  cancellation and code-owner lifetime with existing focused tests.
- Record concrete SDK feature and CLI entrypoint choices here before A02/A03.
  Keep explicit build and load operations; do not introduce compile-on-load.

### A02 - Cranelift object output and linked bundle

- Factor shared scalar lowering, add ObjectModule output, and export only the
  selected entry required by the bundle. Keep JIT finalization separate.
- Implement build orchestration and CLI linker invocation for Windows MSVC.
  Select an explicit linker tool/target; report missing tools and failed links
  with useful diagnostics. Compiler/linker tools are build-machine requirements.
- Produce the bytecode, DLL and manifest from one checked Program. Publish a
  complete immutable bundle only after successful emission/linking; partial
  files or unsupported input must not replace a usable bundle.
- Verify object/image format, exports, target and absent embedded process bindings;
  retain a JIT regression using the same lowered scalar case.

### A03 - Compiler-free loading and execution

- Implement bounded preflight checks, trusted OS loading, symbol resolution and
  executable-owner retention. Install through the existing runtime path.
- Add separately gated SDK loading and a CLI load/run path. The runtime consumer
  needs neither source, compiler/MIR, codegen nor Cranelift in production deps.
- Execute an A02-produced bundle in a distinct process, assert Native and 42,
  and check dependency isolation rather than inferring it from feature names.

### A04 - Deployment boundaries and documentation

- Extend existing contract tests for AOT overflow/origins, cancellation/cleanup,
  malformed/mismatched metadata, unsupported CPU/target/build contract and
  wrong Program/entry. Test rejection without executing incompatible native code.
- Verify old/new code across reload, failed candidate preservation, code surviving
  preparation/backend destruction, shared image use by separate runtimes and
  eventual owner release. Test observer/policy pre-entry routing separately.
- Provide a runnable build-machine/server example and document required build
  tools, trusted deployment inputs and unsupported language/platform coverage.
- Move lasting behavior to architecture and backend/artifact specifications on
  completion; remove this plan after transferring unresolved work to the roadmap.

## Validation policy and progress

Local iteration uses selected tests at the affected semantic owners, including
JIT preparation and native artifact/runtime boundaries. Add an independent AOT
producer/consumer integration case because process separation and a compiler-free
consumer are distinct contracts; avoid a duplicate language smoke suite.
Record exact target/test filters once implemented. Do not run the full workspace
or complete feature matrix locally. Structure, formatting and diff checks apply
at Rust checkpoints; documentation-only changes use content/link/diff checks.

CI owns exhaustive workspace/feature validation and Windows deployment coverage.
Linux AOT support is deferred, not an unrun acceptance gate for this Windows scope.
Existing non-Windows JIT behavior must remain covered by its normal CI checks.
No performance claim or benchmark campaign is required for deployment acceptance.

- [x] Planning: inspected current owners and recorded scope/contracts at `12eee4da`.
- [ ] A01: contracts and shared scalar binding.
- [ ] A02: object emission, linking and complete bundle publication.
- [ ] A03: compiler-free loading and independent native execution.
- [ ] A04: deployment boundary validation and current-contract documentation.

Known implementation failures: none recorded; no implementation checks have run.
Planning validation: relative document links and `git diff --check`; no Cargo
builds/tests are required for this documentation-only checkpoint.
