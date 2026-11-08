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
registered documentation examples. These are declaration-mode sources, containing
native-only signatures and representations; they are not ordinary executable
program fixtures for `kg fmt`. Preserve LF and one final newline.

Installed files are read-only inputs. The analyzer verifies the complete installed
package against its embedded bytes before using its paths. A missing or mismatched
package falls back to embedded text and a named immutable cache snapshot. See the
[native source contract](../../../docs/lsp-design.md#native-declarations-and-source-tools)
for publication, navigation availability and lifetime rules.
