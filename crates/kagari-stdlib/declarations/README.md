# Bundled standard declarations

These fixed declaration-mode `.kgr` sources belong to `kagari-stdlib`. The explicit
module inventory in [`src/sources.rs`](../src/sources.rs) binds each resource to its
registered package/module identity and embeds its exact bytes. There is no build
script, Host invocation or generated artifact prerequisite for obtaining them.
The analyzer embeds them; the VS Code build copies them into `dist/stdlib`.

Rust registration records remain authoritative for native signatures, roles,
bindings and execution. Update the corresponding resource when changing an API
or its documented behavior. HIR checks every bundled module against registration,
including complete non-trivia syntax, role/shape constraints and native defaults.
Whitespace and documentation do not define native ABI identity. Tooling reads
documentation and source ranges from these exact resources; keep registration
documentation consistent for native export consumers.

Run the existing owner contract after edits:

```sh
cargo test --locked -p kagari-stdlib --lib tests::documentation:: -- --nocapture
```

It checks the complete source/provider inventory, native analysis diagnostics and
exact Markdown correspondence by portable declaration identity, including overloads.
Examples are extracted from the displayed source comments. Equivalent programs run
once, retaining every API owner and source location for failure reporting. Each
ordinary `fn main() -> bool` program must compile and execute to true, with fresh
mutable runtime state. A deliberately incorrect String byte-offset expectation
proves this is an outcome check; the slice trap example checks IndexOutOfBounds.
Only the two Ref/RefMut visitor declarations are analysis-only: a target schema
cannot manufacture a scoped Host capability. TaskScope examples receive a real
scheduled scope from the test Host. Future and Task examples are actually driven.

For an affected-only iteration, select a named test case such as `case_01_strings`
or `case_08_numbers`. The inventory check remains cheap enough to run separately:
`documentation_inventory_matches_registration_and_schema_examples`. Compiler and
native preparation are reused within each bounded group; each displayed program
retains independent user sources and heap state. Tests report analysis,
compilation/load and execution/cleanup times separately, plus slow examples.
These are declaration-mode sources, containing
native-only signatures and representations; they are not ordinary executable
program fixtures for `kg fmt`. Preserve LF and one final newline.

Installed files are read-only inputs. The analyzer verifies the complete installed
package against its embedded bytes before using its paths. A missing or mismatched
package falls back to embedded text and a named immutable cache snapshot. See the
[native source contract](../../../docs/lsp-design.md#native-declarations-and-source-tools)
for publication, navigation availability and lifetime rules.

## Documentation style

Start each public item with a short sentence describing its behavior for a caller.
Follow it with the details needed to use the item correctly: argument meanings,
return values, empty-input behavior, evaluation order and stopping rules. Name
parameters with backticks. Use paragraphs and wrap prose to readable source lines;
do not put an entire API contract on one comment line.

Name public parameters for their roles in the Rust registration and the paired
source: map `key`/`value`, sequence `index`/`value`, comparison `other`, operator
`rhs`, task `factory` and callable `arguments`. Trait overrides inherit those
names from their contracts. Do not replace them with positional `arg1`/`arg2`
placeholders: hover and source navigation display these signatures to callers.

Document laziness, shared state, mutation and failure effects when they matter.
Distinguish requirements on a user implementation from guarantees made by built-in
implementations. Use `# Traps` for script traps, and describe the precise trigger
and what has already happened. Do not describe a trap as a Rust panic or expose
registration/binding machinery as if it were part of the caller's API.

Each public operation needs an example that exercises that operation and makes
the result or effect observable. Use concrete inputs and check values or effects
in `fn main() -> bool`; the documentation test requires true. Merely assigning a
call result does not demonstrate behavior. Show meaningful boundaries, such as a
consumed match followed by a remaining cursor item, rather than reusing the trait
overview for every member. Error enum examples match the exact variant instead of
assuming an Eq implementation. Explain any deliberately trapping public helper and
register its typed expected failure at the existing Rust documentation test owner.
Declaration-only examples require an explicit reviewed exception there.
Do not add empty parameter/return/error headings merely to fill a template.

Separate sibling declarations with one blank line before the next item's attached
documentation or attributes. This also applies inside traits, implementations and
enums. Keep a documentation block contiguous with the declaration it describes.

The review covers the complete bundled public inventory: strings, characters,
arrays, vectors, iteration, collection interfaces/hash storage, carriers,
conversion/default/operator/comparison/formatting traits, numbers and async tasks.
Numeric families share authored boundary witnesses, with concrete receiver widths
and expected values; no helper invents a discarded-result call from a signature.
A nonempty comment or passing example is not a quality assessment: inspect the
implementation and the caller questions when reviewing new or changed APIs.
The approach follows the
[rustdoc writing guide](https://doc.rust-lang.org/rustdoc/how-to-write-documentation.html)
and the operation-specific contracts/examples of
[Rust's Iterator documentation](https://doc.rust-lang.org/std/iter/trait.Iterator.html)
(std 1.99.1; guide consulted 2026-11-08). Kagari retains its own `u64` counters,
shared cursor semantics and script trap boundaries.
