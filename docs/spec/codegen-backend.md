# Kagari Codegen Backend

Native compilation consumes verified Kagari MIR and explicit ABI/link descriptions.
Compilation, runtime installation and invocation are separate boundaries. The
[architecture](../architecture.md) defines crate ownership; the
[MIR refactor plan](../mir-architecture-refactor.md) records migration and validation.

## Ownership

```text
checked HIR --compiler source--> verified MIR --compiler core--> verified bytecode
                                      |
                               codegen interface
                                      |
                             Cranelift machine code
                                      |
                           runtime installation/invocation
```

- `kagari-mir` owns concrete typed CFGs, verification, sealed analyses, portable
  encoding and bounded public optimization passes. It does not depend on HIR.
- `kagari-compiler` owns source lowering, reachable monomorphization, MIR-to-bytecode
  emission, canonical artifact correspondence and native link construction.
- `kagari-abi` owns executable types/layouts, helper signatures, native calling
  representations, version constants, descriptors and code ownership contracts.
- `kagari-codegen` owns the compilation interface and diagnostics. It consumes MIR
  and ABI, without runtime, bytecode, frontend or concrete backend dependencies.
- `kagari-codegen-cranelift` owns MIR-to-CLIF legalization, host ISA selection,
  machine-code generation and executable page lifetime.
- `kagari-runtime` owns installation, generation/authority checks, sessions, frames,
  helper implementations and invocation. The VM consumes prepared native entries.
- `kagari-embed` orchestrates preparation, caching, linking and execution. Concrete
  backends are supplied by hosts; they are not SDK production dependencies.

No backend receives source text, a HIR arena, `Runtime`, `LoadedModule`, a Rust
`Value` or a `BytecodeFunction` to recover semantic facts. Cranelift IDs, builders,
registers and calling-convention objects remain private to its implementation.

## Verified Handoff

`BackendFunctionInput::new` selects an `InstanceId` from a `VerifiedMirModule` and
borrows a `NativeLinkDescription`. Its private fields prevent callers from pairing
an unrelated function with the module's seal. Accessors expose the function,
module, its `FunctionAnalysis` and the supplied links.

The verified handoff retains concrete executable identities, typed operations,
module/function links, origins, effects, definite initialization, liveness,
logical roots, safepoints and cancellation safepoints. Editing MIR requires consuming
the seal and re-verifying before lowering or compilation. Physical registers,
spills and backend stack-map locations remain backend responsibilities; the backend
must reject support when it cannot implement the verified root contract.

Portable MIR carries untrusted data, never serialized trust. A native-enabled SDK
decodes and verifies it, lowers it through compiler core and compares the canonical
bytecode program before installation. Missing native input permits an explicit
unsupported decision. Invalid or inconsistent input is an error, not permission
to execute either unvalidated representation.

## Compilation Interface and Trust

The current trait is defined in `crates/kagari-codegen/src/lib.rs`:

```text
unsafe trait CodegenBackend {
    configuration() -> BackendConfiguration
    compile_function(BackendFunctionInput) -> Result<NativeCompilationProduct, BackendCompileError>
}
```

There is no invocation method. `BackendConfiguration` includes the backend ID,
target triple/pointer width/ISA features and every option affecting code generation.
Equal configurations must permit reuse of compiled products. Unsupported input is
distinct from compiler or link errors and must be reported before script effects.

Implementing this trait is unsafe: successful products must implement the exact
verified function's semantics, logical charges, roots and host native ABI, retain
all referenced code/links, and never unwind through C frames. Native code is trusted
host code. Safe descriptor validation cannot prove arbitrary executable pointers
or machine instructions correct.

`NativeCompilationProduct` combines an `ExecutableFunctionArtifact` with an
`Rc<dyn NativeCodeOwner>`. Descriptors identify backend, target, function, entry
address, runtime/helper versions, safepoints, traps and debug metadata. The owner
keeps callable pages alive independently of backend destruction or later compilation.
The baseline is local to one host thread; the owner makes no Send/Sync promise.

## Links and Runtime ABI

The compiler constructs an immutable `NativeLinkDescription` from explicitly
supplied helper symbols. Generated code must use the declared helper identity,
signature and address. Missing, duplicate or malformed required links are compiler
errors. Backends do not inspect runtime internals to resolve them.

The current zero-argument scalar entry uses the C ABI:

```text
(runtime: *const c_void, result: *mut JitValue) -> i32 status
```

`kagari-abi::native_call` defines tagged Unit/Bool/i32 results and status codes.
Runtime translates them into values or structured failures. Cranelift stores fields
at ABI-defined offsets rather than assuming the layout of runtime Rust `Value`.

At each executable point, the scalar backend calls the execution-poll helper using
its sealed logical offset. Runtime validates and records the location, checks
sticky termination/cancellation and visits a GC safepoint before checked arithmetic.
Errors retain their correct source origin. There are no logical charges or
charge-only checkpoints; optimizers retain actual effects and required safepoints.

Allocation, host access, borrows, declared interface checks, reflection, paths, calls and
future GC-bearing native operations must use the shared runtime contracts. Their
presence in MIR does not imply support by the current scalar backend.

## Preparation, Installation and Execution

`PreparedProgram` retains a shared `VerifiedProgram` and, with `native`, optional
verified MIR and a bounded native product cache. Cache keys include program-local
function identity, complete backend configuration and runtime/helper ABI versions.
Successful products and unsupported decisions are reusable; cancellation and
compiler errors are not cached. Runtime-specific handles are never cached there.

`KagariRuntime::prepare_native` checks the exact prepared/loaded version identity,
entry and runtime policy, compiles or reuses the product and installs it for this
runtime. `Runtime::install_native_function` is unsafe for direct callers; it checks
the descriptor, host ABI, declared access, entry/function/signature and metadata bounds,
while the caller proves code correctness and lifetime. SDK orchestration can satisfy
that contract using the unsafe backend guarantee and verified correspondence.

`InstalledNativeFunction` retains its code owner, loaded version and dependency
closure. Runtime invocation creates ordinary sessions/frames, checks authority and
ownership and shares cancellation, GC, cancellation and trace behavior with the VM.
`Vm::execute_prepared` accepts an installed entry or an explicit unsupported decision.
It never compiles. Only a pre-entry unsupported/policy decision may fall back;
a native failure after entry is returned with its trace and never restarted.

The baseline has no native observer callbacks. An attached execution observer
forces pre-entry interpreter fallback even if descriptor flags claim debug support.
Flags alone cannot provide debugger semantics.

Reload publishes a new immutable version. Existing native entries retain the exact
old version and pages while reachable; they cannot masquerade as entries for the
new version. Failed publication leaves current entrypoints intact. Interpreter cache
records are separate from native code ownership. Machine-code persistence across
process restarts is not supported.

## Current Cranelift Subset

Supported functions have no arguments, one straight-line return block, and only
Unit/Bool/i32 values. Supported instructions are constants, moves, checked i32 add/subtract/multiply/negation, boolean not, supported
scalar equality/order comparisons and return. GC-bearing values, locals, branches,
loops, calls, remainder/division and other numeric families select pre-entry fallback.
Each scalar helper safepoint has an empty root map. The backend allocates one JIT
module per successful product and frees its pages on final code-owner drop.

Native/interpreter tests must distinguish Native from InterpreterFallback, including
source and encoded artifacts, overflow and cancellation behavior, exact origins, retained code,
reload and GC boundaries. Compiler errors must not be mislabeled unsupported.

## Future Backends

Expand Cranelift coverage as a separate track before introducing LLVM. New support
must consume the same concrete MIR facts and explicit link/ABI descriptions, retain
correct roots and versions and satisfy the behavior matrix. No placeholder LLVM
crate, duplicate runtime or second implementation of source semantics is required.
Module/AOT compilation, physical GC maps, native debugger callbacks and cross-thread
execution need their own explicit contracts and acceptance tests; they are not
implicitly provided by the current function compiler.
