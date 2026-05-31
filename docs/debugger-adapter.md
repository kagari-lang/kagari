# Kagari Debugger Adapter Boundary

Kagari exposes debugger control through the VM debugger protocol boundary, not through script-visible APIs.
IDE integrations, DAP servers, and host tools should translate their own protocol messages into `DebugAdapterRequest` values and forward emitted `DebugAdapterEvent` values back to the tool.

## Session Flow

1. Create a runtime with debugger profile and capabilities enabled.
2. Create a `Vm` for that runtime.
3. Create a `DebugProtocolAdapter` with an event sink.
4. Send `DebugAdapterRequest::Attach`.
5. Send breakpoint, stepping, pause, continue, run-to-cursor, or watch-evaluation requests.
6. Execute the module through the VM.
7. Call `FlushEvents` or `flush_events` after execution steps to deliver resolved breakpoint and pause events to the host tool.

The adapter boundary owns protocol translation only.
Runtime semantics remain in the parser, checker, bytecode verifier, VM, runtime security checks, and debug session hooks.

## Supported Requests

- `Attach`
- `SetBreakpoint`
- `Continue`
- `Pause`
- `StepInto`
- `StepOver`
- `StepOut`
- `RunToCursor`
- `EvaluateWatch`
- `FlushEvents`

## Events

Adapters receive:

- session attachment
- breakpoint creation
- breakpoint resolution to safe debug points
- continue and pause requests
- pause events with inspected frames

Pause events carry debugger frame ids, module ids, module epochs, function ids, source spans, instruction offsets, and visible bindings.
Watch evaluation uses those debugger frame ids and remains subject to debugger capability and host visibility policy.

## Policy

Debugger attachment and operations require runtime debugger capabilities.
Restricted profiles should leave debugger attachment disabled.
Host-owned values are inspectable only when the runtime debug visibility policy exposes them.
JIT execution may fall back to the interpreter when active debug sessions require metadata that the compiled artifact cannot provide.

## Adapter Responsibilities

An IDE or DAP adapter is responsible for:

- converting IDE protocol requests into Kagari debug adapter requests
- mapping Kagari events back to IDE protocol events
- presenting source files and variable trees
- preserving user breakpoint UI state
- owning transport, message ids, and client-specific lifecycle

The adapter must not add script-visible debugger functions or reinterpret Kagari execution behavior.
