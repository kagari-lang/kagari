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
cargo test --locked -p kagari-stdlib --lib -- standard_documentation_covers_the_registered_api_and_examples_analyze
```

It checks the complete source/provider inventory, native analysis diagnostics and
examples from both registration documentation and the actual bundled sources.
Equivalent examples are analyzed once, each with a fresh user-source snapshot.
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

Document laziness, shared state, mutation and failure effects when they matter.
Distinguish requirements on a user implementation from guarantees made by built-in
implementations. Use `# Traps` for script traps, and describe the precise trigger
and what has already happened. Do not describe a trap as a Rust panic or expose
registration/binding machinery as if it were part of the caller's API.

Each reviewed public operation needs an example that exercises that operation and
makes the result or effect clear. Show meaningful boundaries, such as a consumed
match followed by a remaining cursor item, rather than reusing the trait overview
example for every member. Examples must be complete analyzable Kagari programs.
Do not add empty parameter/return/error headings merely to fill a template.

Separate sibling declarations with one blank line before the next item's attached
documentation or attributes. This also applies inside traits, implementations and
enums. Keep a documentation block contiguous with the declaration it describes.

`core::iter` is the first full content review. Other modules have normalized
declaration spacing, but their existing prose/examples still need module-specific
review; a nonempty comment or passing example is not a quality assessment.
The approach follows the
[rustdoc writing guide](https://doc.rust-lang.org/rustdoc/how-to-write-documentation.html)
and the operation-specific contracts/examples of
[Rust's Iterator documentation](https://doc.rust-lang.org/std/iter/trait.Iterator.html)
(std 1.99.1; guide consulted 2026-11-08). Kagari retains its own `u64` counters,
shared cursor semantics and script trap boundaries.
