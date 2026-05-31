# Kagari Embedding API Specification

This document defines the Rust-facing API shape for embedding Kagari.
It is a semantic API specification, not a commitment to exact Rust type names.

## Design Goals

- expose a small, stable host API for compiling, loading, running, and reloading scripts
- keep host state ownership explicit
- keep host registrations typed and capability-aware
- make hot reload and module epochs visible to the embedding layer
- keep interpreter and JIT selection behind runtime policy
- return structured diagnostics and runtime errors

## Core Host Objects

The embedding API is organized around these concepts:

```text
KagariEngine
  owns compiler services, runtime configuration, and shared registries

KagariRuntime
  owns loaded modules, heaps, host registries, security state, and execution policy

DebugController
  owns debugger sessions, breakpoint registries, pause state, and debugger events

HostRegistry
  owns registered host types, functions, interfaces, paths, and metadata

ModuleLoader
  resolves source modules, bytecode artifacts, package roots, and imports

LoadedModule
  identifies a successfully loaded module epoch

ExecutionContext
  carries capabilities, resource limits, host access policy, and tracing hooks
```

The actual Rust API may split these objects across crates, but the same ownership boundaries must be preserved.

## Compile and Load Flow

The host-visible pipeline is:

```text
compile source -> checked module -> bytecode artifact -> load module epoch -> execute entry
```

The embedding API must expose operations equivalent to:

```text
compile_source(source, compile_options) -> CompileResult<CheckedModule>
emit_bytecode(checked_module, artifact_options) -> CompileResult<BytecodeArtifact>
load_module(module_id, bytecode, load_options) -> LoadResult<LoadedModule>
execute(module, entry, args, execution_context) -> RunResult<Value>
reload_module(module_id, bytecode, reload_options) -> ReloadResult<LoadedModule>
```

Convenience functions may combine these operations for CLI use, but the underlying phases remain separate.

## Host Registry API

The host registry supports explicit registration of:

- host types
- host functions
- host constructors or factories, when allowed
- host-backed roots
- typed path descriptors
- trait/interface implementations for host values
- reflection metadata exposed to tooling or privileged profiles
- capability requirements

Registration must produce stable metadata identities used by type checking, bytecode validation, typed path mutation, hot reload, and optional JIT compilation.

## Type and Function Registration

Host function registration records:

- script-visible name
- parameter types
- return type
- passing style for each parameter
- capability requirements
- resource cost hints, if provided
- whether the call may allocate, trap, call host services, mutate host state, or suspend

Host type registration records:

- stable type identity
- script-visible name
- ownership model
- exposed fields and methods
- typed path access policy
- reflection exposure policy
- reload and layout fingerprint

Open Rust generics are not registered directly.
The host registers concrete instantiations or a separate factory model defined by host interop policy.

## Execution Context

Every host-initiated execution uses an execution context.

The context carries:

- language profile
- runtime capabilities
- resource limits
- host API exposure policy
- reflection policy
- JIT enablement policy
- tracing or audit hooks
- panic and engine-bug reporting policy

The execution context is not script-visible as an ordinary value.
It is an embedding boundary object used by runtime checks and host calls.

## Error and Diagnostic Model

The embedding API returns structured results.

Compile-time failures return diagnostics with:

- severity
- diagnostic code
- source span
- message
- optional notes and labels

Runtime failures return classified errors:

- script trap
- type or bytecode verification failure
- capability denial
- resource limit exceeded
- host call failure
- typed path validation failure
- stale module or host root
- reload validation failure
- engine invariant violation

Engine invariant violations may panic in debug builds, but production APIs still expose a controlled error boundary where practical.

## Hot Reload API

Reload is explicit.

The host supplies a candidate bytecode artifact or checked module for an existing module id.
The runtime validates:

- public ABI fingerprints
- type identities and layouts
- interface tables
- host registrations
- typed path descriptors
- bytecode verifier metadata
- JIT artifact dependencies, if JIT is enabled

If validation succeeds, the new epoch is published.
If validation fails, the existing active epoch remains active.

## Artifact API

The embedding API supports source execution and precompiled bytecode artifacts.

Artifact loading must:

- validate artifact header and format version
- validate target ABI and runtime-helper ABI versions
- validate module id and dependency fingerprints
- reject artifacts produced for incompatible language or runtime versions
- preserve debug metadata when available

The artifact format is specified in [artifacts.md](/Users/mikai/CLionProjects/kagari/docs/spec/artifacts.md).

## JIT Control

The host controls JIT policy through runtime or execution options.

Allowed policies include:

- disabled
- enabled for eligible functions
- compile on load
- compile on first call
- compile after threshold

JIT policy must not change script-visible behavior.
When JIT is disabled or unavailable, the interpreter remains the execution path.

## Debugger API

The embedding API exposes debugger control through host-created debug sessions.

Debugger operations include:

- attach session
- detach session
- set and clear breakpoints
- pause and continue
- step into, step over, and step out
- inspect call stack
- inspect frame variables
- inspect values according to host policy
- evaluate read-only watch expressions
- receive debugger events

The debugger API is not script-visible.
Debugger attachment and inspection require runtime capabilities and host policy.

Debug sessions use the model defined in [debugger.md](/Users/mikai/CLionProjects/kagari/docs/spec/debugger.md).

## Threading and Isolates

A single Kagari runtime isolate is single-threaded from the script perspective.
Hosts may run multiple isolates on different Rust threads.

Host APIs must not expose one mutable script heap concurrently to multiple script threads.
Cross-isolate value sharing requires explicit serialization, host handles, or embedding-defined transfer rules.

## Acceptance Criteria

The embedding API is complete when:

- host applications can compile, load, execute, and reload modules through stable entry points
- host registrations produce metadata used by checking, bytecode, runtime, reload, and JIT
- execution contexts enforce capabilities and resource limits
- errors and diagnostics are structured
- bytecode artifacts and source modules can be loaded through the same module identity model
- JIT can be enabled or disabled without changing semantics
