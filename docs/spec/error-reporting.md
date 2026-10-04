# Error origins and diagnostic stacks

This is the authoritative error-reporting contract. Errors remain ordinary
`Result<T, E>` values; no `throw`, `try` or `catch` syntax is introduced.

## Origin ownership and propagation

The ordinary core-library Result declaration marks its Err variant with the
checked `reports_failure` fact. A newly constructed `Result::Err(e)` captures the current source location
and synchronous script stack, innermost frame first. The metadata belongs to the
Result value, not to `e`. It records the original call stack, not a history of
later propagation. A stored error can therefore report callers that have already
returned when another caller inspects it.

| Operation | Origin behavior |
| --- | --- |
| New `Err(e)` | Capture a new origin and stack |
| `?`, ordinary return, assignment, container storage | Preserve the original metadata |
| `map`, `and_then` on Err | Preserve the original Err |
| `map_err` on Err | Change the payload and preserve the original metadata |
| `?` with a `From` error conversion | Convert the payload and preserve the original metadata |
| `match r { Err(e) => Err(e), ... }` | Construct a new Err and capture a new origin |
| `None`, propagation of None | No failure metadata |
| `None.ok_or(e)` / `ok_or_else(...)` | Capture at the conversion site |
| `Ok(Err(e))` | The inner Err has metadata; the outer Ok is successful |

Patterns expose only the payload. Equality, hashing and container key lookup ignore
this metadata. Tuple/enum composition does not turn nested failures into top-level
execution failures. `?` may change the payload through `From`; its preserved trace
does not acquire conversion frames. If the conversion itself traps, that trap has
its own execution stack, including the conversion call.

The heap stores optional origin beside ordinary nominal enum values. A generic
ForwardEnumOrigin operation copies this metadata after validating the target enum
construction; it does not reinterpret a payload or resolve a standard type name.
Unmarked source/native enum variants capture no origin and do not become CLI
failures automatically. Reporting policy is checked against carried declarations.

## Runtime and host boundaries

Traps, cancellation and resource termination capture their script stack before
frames unwind. A synchronous host reentry shares the suspended caller stack.
Sticky termination preserves the first captured origin even when a host callback
attempts to swallow the nested failure. Cleanup and completed side effects follow
[the failure contract](failure-semantics.md).

A host-created Err during a script call records that script call site. Native Rust
frames are not synthesized. An Err constructed outside execution has an explicitly
unavailable/incomplete trace. A host forwarding a nested ordinary trap can retain
its trace using `HostError::with_trace`; constructing a new HostError from text
alone cannot preserve an origin that was discarded by the host.

## Reporting

Returning Err is successful VM execution, with an ordinary `return_value` and an
optional `ExecutionReport::failure`. `Runtime::result_failure(&value)` inspects a
returned or rooted Err; `RuntimeError::trace`, `VmError::trace` and embedding
`EmbeddingError::error_trace` expose runtime-failure snapshots. These accessors do
not print, invoke script code, or require a third-party error library.

The CLI renders a top-level returned Err and its stack on stderr and exits with
status 1. Ok, None and internally handled Err values do not produce an error
report. Embedded hosts decide when and where to present reports.

`ResultFailure` is a detached diagnostic preview. String payloads are shown as
text; other payloads use the runtime's bounded structural/identity preview, never
arbitrary user Debug/Display code. The original typed payload remains in Result.
Try/FromResidual permit custom carriers without automatically assigning failure
reporting policy. A custom Error trait and message/cause protocol are
separate future features.

## Bounds, versions and source positions

Snapshots contain at most 128 innermost frames, an omitted-frame count and an
incomplete flag. Function names, source URIs and payload previews are limited to
4096 UTF-8 bytes each (a preview may append a truncation marker). Unavailable
frames/locations are explicit and do not replace the original failure.

Each frame copies the code fingerprint, module epoch, function identity/name,
logical instruction offset and available source range/position. Locations use
one-based lines and UTF-8 byte columns, including CRLF and non-ASCII source.
Source URIs and line tables survive artifact loading without optional debugger
metadata; stack reports do not consult current disk contents after hot reload.

Snapshots contain no local values, GC roots or owning execution-version handles.
Keeping a report does not retain script objects or old code. Hosts retaining the
Result value itself must still use a rooted handle. Error trace data uses bounded
host diagnostic memory outside the script heap-unit accounting. Native backends
publish logical instruction positions before resource checks; unsupported Result
operations use the existing interpreter fallback.
