# Prepared scalar execution and source generation

Runtime prepares verified scalar operations once, then executes specialized
instruction variants with one direct match. Canonical bytecode, checked numeric
semantics and external artifact/ABI identifiers are unchanged. The numeric payload
helpers remain in `kagari-types`; the generator implements no arithmetic.

## Source ownership

The [instruction catalog](../../../crates/kagari-runtime/codegen/scalar-instructions.toml)
is the single owner of specialized variant names, source patterns, immediate types
and helper selection. It contains six finite families: binary, shift, comparison,
simple comparison, unary and conversion. The current catalog yields 221 specialized
variants. Seventeen handwritten base/control/shared variants complete the existing
238-variant execution enum; this is an internal representation, not an opcode count
for the language. The existing physical instruction budget test still owns its
24-byte ceiling.

| Source | Handwritten responsibility | Generated region |
| --- | --- | --- |
| [instruction.rs](../../../crates/kagari-runtime/src/module/execution/instruction.rs) | Base/control/shared variants and type attributes | Specialized enum variants |
| [scalar.rs](../../../crates/kagari-runtime/src/module/execution/scalar.rs) | Operand classification and shared/uncommon preparation | Operation/operand-shape mapping arms |
| [scalars.rs](../../../crates/kagari-runtime/src/frame/cursor/scalars.rs) | Slice modes, payload access and cold failure conversion | None |
| [execute.rs](../../../crates/kagari-runtime/src/frame/cursor/scalars/execute.rs) | Complete loop, PC publication, control exits and result writeback | Direct specialized execution arms |

The surrounding [execution module](../../../crates/kagari-runtime/src/module/execution.rs)
owns function/frame preparation facts. Consumers import the instruction type from
its actual module; no old-path forwarding export is maintained.

An I32AddRR arm reads both scalar payloads, calls the existing checked i32 helper
and produces a destination/value pair for common writeback. I32AddRK retains the
existing signed immediate encoding. Comparison branches, shifts, floating-point
and conversion semantics use the same established helpers as before generation.
RR/RK, uncommon KR/KK and variant order are preserved.

Internal invariant failures use named ScalarFailure variants, with one static
message mapping and thiserror Display/Error support. Repeated operand accesses
construct InvalidOperandSlot rather than carrying a repeated string payload.
The existing quarantine API consumes that static message directly; no intermediate
String or formatting is added to instruction execution. Numeric helpers still
return their existing static trap reasons, carried by ScalarFailure::Trap, and
conversion to owned RuntimeError remains on the cold segment exit. Error kinds,
diagnostics and PC/effect ordering remain unchanged. No speedup is claimed.

The loop itself is ordinary Rust. It keeps one dispatch, local PC bookkeeping and
common PC publication on success/error. It is not split through handler tables,
trait objects or a second result dispatch. The small local `attempt!` macro solely
propagates failure to that common exit; it does not generate the loop or opcode
families. Polling/slicing and GC boundaries are unchanged by source generation.

## Updating instructions

Run from the repository root:

```text
uv run python scripts/generate_scalar_execution.py
uv run python scripts/generate_scalar_execution.py --check
```

The [generator](../../../scripts/generate_scalar_execution.py) uses standard-library
TOML parsing and fixed family renderers. Change the catalog for generated behavior;
edit the Rust skeleton for control flow. Review and submit catalog, generator and
resulting Rust changes together. Normal Cargo builds compile the checked-in source
without Python, build.rs generation or OUT_DIR includes.

Only exact named BEGIN/END GENERATED regions are replaced. The generator rejects
unknown fields/families, malformed rows, duplicate instruction names and ambiguous
markers. It renders all candidate files and formats them with workspace rustfmt
before writing; unchanged files retain their bytes and modification times.
Existing LF/CRLF style is preserved. Fixed input ordering produces fixed output,
without timestamps, absolute paths or target-dependent probing.

`--check` reports drift and exits 1 without rewriting any source. Input/formatting
errors exit 2. CI runs the generation tests and consistency check before Clippy
and Rust tests. Check mode may launch rustfmt, but neither mode runs Cargo or
stages files. Generation consistency verifies synchronization, not arithmetic;
the existing numeric and execution tests remain independent behavioral oracles.

## Bounded structural review

Generated regions count as ordinary Rust for imports, visibility, module layout and
effective LOC. There is no generated-source exclusion. The current formatted output
requires these exact, bounded exceptions in the
[structure policy](../../../scripts/structure-exceptions.toml):

| File | Effective LOC | Reviewed ceiling | Cohesion evidence |
| --- | ---: | ---: | --- |
| instruction.rs | 1267 | 1280 | One Rust enum with 238 ordered variants; enum variants cannot be separated into ordinary child modules. |
| scalar.rs | 2698 | 2800 | One typed operation/operand-shape preparation match, plus its small classification/shared fallback owner. Splitting by family adds nested selection or recreates macro assembly of match arms. |
| execute.rs | 2967 | 2980 | One hot instruction loop, common result writeback and common PC/error exit. Named invariant failures remove repeated diagnostic expressions. Splitting by family adds handler boundaries or another dispatch; source generation preserves the established control flow. |

The surrounding execution facts and scalar-access helpers already live separately.
These exceptions cover three exact files, not every generated source. Adding
instructions beyond the ceilings requires a fresh review. Standard rustfmt output
is retained so generated branches are readable and debuggable; line compression
or suppressing formatting is not used to hide the expanded size.

## Reference mechanisms

[Cargo generated-source guidance](https://doc.rust-lang.org/cargo/reference/build-script-examples.html#code-generation)
recommends explicit regeneration and consistency checks for committed output.
[CPython 3.14.0 case-generation tooling](https://github.com/python/cpython/blob/v3.14.0/Tools/cases_generator/README.md)
feeds execution code from instruction definitions. Kagari adopts explicit generated
source, with a finite family renderer; it does not adopt CPython's instruction DSL,
stack-effect analysis or dispatch mechanism.
