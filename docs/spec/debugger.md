# Kagari Debugger Specification

This document defines Kagari debugger support.
The target user experience is close to debugging Kotlin in IntelliJ IDEA: source breakpoints, stepping, stack frames, variable inspection, watch expressions, and stable behavior across hot reload.

Debugger support is a host/tooling capability.
It is not an ordinary script API and it must not bypass runtime security, host exposure policy, typed path mutation policy, or hot reload validation.

## Design Goals

- support IDE-grade source debugging for scripts
- preserve interpreter semantics while debugging
- keep debugger control outside ordinary script code
- allow IntelliJ/DAP-style adapters without coupling the VM to one IDE protocol
- support breakpoints, stepping, call stacks, variable inspection, watch expressions, and trap breakpoints
- remain compatible with hot reload and module epochs
- provide a conservative JIT policy that falls back to the interpreter when needed

## Non-Goals

The debugger does not provide:

- script-visible debugger APIs
- unrestricted reflection or mutation
- native machine-code debugging as the baseline requirement
- arbitrary pause points inside every bytecode instruction
- debugger access to host-owned data that host policy does not expose
- time-travel debugging
- deterministic replay
- multi-threaded script debugging inside one isolate

## Core Architecture

The debugger layer is organized around:

```text
DebugController
  owns sessions, breakpoint registries, pause state, and event routing

DebugSession
  attaches to one runtime isolate or host-managed execution group

BreakpointRegistry
  stores source breakpoints and resolved breakpoint locations

SourceMap
  maps source spans to bytecode offsets and safe debug points

FrameInspector
  reads stack frames, locals, parameters, captured values, and receiver values

ValueInspector
  formats and expands script values, interface values, arrays, enums, and host-visible path values

ExpressionEvaluator
  evaluates watch expressions under debugger policy

DebugEventSink
  sends pause, resume, trap, step, and breakpoint events to tooling adapters
```

IDE-specific protocols such as DAP or IntelliJ plugin APIs live outside the core runtime.
They adapt tool requests into this debugger control surface.

## Debug Sessions

A debug session is created by the host.

The session records:

- runtime isolate id
- security profile
- debugger capabilities
- visible module set
- host object inspection policy
- watch/evaluate policy
- JIT policy while debugging
- event sink

Only one active controller may control pause/resume state for a given isolate at a time unless the host defines a coordination layer.

## Breakpoints

Source breakpoints are specified in source coordinates:

```text
SourceBreakpoint {
  source_uri: SourceUri,
  line: u32,
  column: optional u32,
  condition: optional DebugExpression,
  hit_count: optional HitCountRule,
  temporary: bool
}
```

Breakpoints resolve to module-epoch-specific execution locations:

```text
ResolvedBreakpoint {
  breakpoint_id: BreakpointId,
  module_id: ModuleId,
  epoch: ModuleEpoch,
  function_id: FunctionId,
  instruction_offset: InstructionOffset,
  source_span: SourceSpan,
  debug_point: DebugPointId
}
```

If a source breakpoint cannot be resolved for the current epoch, it remains pending.
Pending breakpoints are retried when modules load or reload.

## Supported Breakpoint Types

The baseline debugger supports:

- line breakpoints
- conditional breakpoints
- hit-count breakpoints
- temporary breakpoints
- trap breakpoints
- host call failure breakpoints
- capability denial breakpoints

Data breakpoints and watchpoint-style field mutation breakpoints are not baseline requirements.
They may be added later through typed path dirty hooks.

## Safe Debug Points

The VM pauses only at safe debug points.

Safe debug points include:

- statement boundaries
- line boundaries selected by source maps
- function entry
- function return
- call boundaries
- branch targets
- loop headers
- trap points
- host call entry and exit
- runtime helper boundaries marked as debuggable

The debugger must not require arbitrary pause at every VM instruction.
This keeps interpreter, GC, host borrow, and JIT support tractable.

## Stepping

The baseline step operations are:

- continue
- pause
- stop
- step into
- step over
- step out
- run to cursor

Stepping is defined over safe debug points and source spans.

`step over` runs through nested calls until the current frame reaches a later safe debug point or exits.
`step into` enters the next debuggable script function when possible.
`step out` runs until the current frame returns to its caller.
`run to cursor` installs a temporary source breakpoint and continues.

Host calls are stepped as opaque calls unless the host exposes a debugger integration for that API.

## Call Stack Inspection

The debugger exposes script call frames.

Each frame includes:

- frame id
- module id
- module epoch
- function id
- function name
- source span
- current instruction offset
- parameter bindings
- local bindings
- captured bindings
- receiver value, when present

Frame ids are debugger-session values and must not be script-visible.

## Variable Inspection

Variables are resolved through debug metadata:

- local names
- parameter names
- live ranges
- register/local locations
- captured environment slots
- source spans

The debugger may show a value as unavailable when:

- the variable is out of live range
- the value has been optimized away by an allowed execution tier
- the value is ephemeral and cannot be inspected safely
- host policy hides the value
- the module epoch metadata is unavailable

## Value Inspection

The value inspector supports:

- primitive values
- `()`
- strings
- arrays
- tuples
- structs
- enums and active variants
- trait/interface values
- closures as opaque values with optional metadata
- host handles as opaque values unless host policy exposes metadata
- host path views through typed path read policy

Value expansion must respect reflection and host exposure policy.
Debugger inspection must not become an unrestricted reflection backdoor.

## Watch Expressions

Watch expressions are evaluated in a selected frame.

The baseline watch evaluator is read-only.
It supports:

- local and parameter reads
- field reads allowed by language and host policy
- index reads
- pure builtin operations
- pure method calls only when explicitly marked safe for debug evaluation

The evaluator rejects:

- assignment
- path mutation
- host calls without debug-evaluation permission
- allocation-heavy operations beyond resource limits
- suspension
- operations requiring unavailable frame-scoped host borrows

## Evaluate Expression

Evaluate Expression is optional and profile-gated.

When enabled, it uses the same evaluator as watch expressions but may allow a larger expression subset according to host policy.
Side-effecting evaluation is disabled by default.
If an embedding enables side effects, they must run through ordinary runtime capability checks, host exposure checks, typed path policy, and resource limits.

## Trap and Exception Breakpoints

The debugger can pause on:

- script traps
- runtime type errors
- bytecode verification failures during load
- capability denials
- resource limit failures
- host call failures
- reload validation failures

Engine invariant violations are reported according to embedding policy.
They are not ordinary script exceptions.

## Hot Reload

Debugger state is epoch-aware.

Rules:

- existing frames keep their original module epoch and source map
- new calls use the latest published epoch
- source breakpoints are re-resolved after reload
- unresolved breakpoints remain pending
- removed or unmappable breakpoints are reported as unbound
- old source maps remain reachable while old frames need them
- watch expressions compile against the selected frame's epoch

Reload failure must not corrupt debugger state.

## JIT Interaction

The baseline debugger is interpreter-first.

When a debug session attaches, the runtime may:

- disable JIT for debugged modules
- fall back to interpreter for functions with active breakpoints
- continue running non-debugged modules through JIT
- allow JIT execution only at declared safe debug points

JIT code may participate in debugging only when it provides:

- line tables
- source span mapping
- stack maps
- live value location metadata
- safe debug point traps or callbacks
- fallback to interpreter when debug metadata is insufficient

The baseline Cranelift JIT does not require arbitrary native instruction debugging.

## Security

Debugger capabilities are separate from reflection capabilities.

Useful debugger capabilities include:

- attach debugger
- set breakpoints
- pause execution
- inspect stack
- inspect local values
- inspect host-exposed values
- evaluate read-only expressions
- evaluate side-effecting expressions

Restricted profiles may disable debugger attachment entirely.
Debugger operations must observe resource limits and host exposure policy.

## Debug Metadata Requirements

Bytecode and artifacts must preserve enough metadata for:

- source span mapping
- line breakpoint resolution
- safe debug point mapping
- local and parameter names
- live ranges
- frame layouts
- captured variable layouts
- function names and module names
- module epoch identity
- value formatting metadata

Debug metadata may be stripped only when the module is not intended to be debugged.
If debug metadata is stripped, breakpoints and variable inspection are unavailable for that artifact.

## Debug Adapter Boundary

The runtime exposes a debugger control surface.
IDE protocols are adapters.

An IntelliJ plugin or DAP adapter owns:

- IDE protocol messages
- source file presentation
- breakpoint UI state
- variable tree rendering
- watch expression UI
- user-facing session lifecycle

The adapter must not define language semantics.

The current VM exposes this boundary as `DebugProtocolAdapter`.
Adapters send `DebugAdapterRequest` values such as `Attach`, `SetBreakpoint`, `Continue`, `Pause`, stepping requests, `RunToCursor`, `EvaluateWatch`, and `FlushEvents`.
They receive `DebugAdapterResponse` values for request results and `DebugAdapterEvent` values for attachment, breakpoint, continue, pause-request, breakpoint-resolution, and pause notifications.
Tool-specific transports, message ids, and DAP or IDE object models remain outside the VM.

## Acceptance Criteria

Debugger support is complete when:

- a host can attach a debug session to a runtime isolate
- source breakpoints resolve to safe debug points
- continue, pause, stop, step into, step over, step out, and run to cursor work in the interpreter
- call stacks expose module epoch, function, source span, and frame identity
- locals, parameters, captured bindings, `self`, arrays, structs, enums, and interface values can be inspected according to policy
- host-owned values respect host metadata and typed path policy
- read-only watch expressions work in selected frames
- trap and host failure breakpoints pause execution
- hot reload re-resolves breakpoints and preserves old-frame source maps
- debug sessions can force interpreter fallback when JIT metadata is insufficient
