# SDK feature-boundary artifact

`target/fixtures/feature_artifact.kbc` is the disposable current-format artifact emitted from the exact
`feature_artifact.kgr` text with source name `memory://feature-artifact.kgr`.
It contains bytecode, carried declarations/dependencies and portable MIR, without
source text. All four standalone SDK feature routes consume the same bytes:
artifact-only, `source`, `native`, and `source,native`.

`main` remains the two-step scalar function used by both the trusted static ABI
fixture and real Cranelift compilation. `library_and_object` exercises trait sorting,
an external native object retaining a callback, and lazy map.
`required_methods` exercises checked MutableList methods, ArrayList FromIterator,
numeric Sum, FromStr, derived TryInto and primitive From. It also checks all twelve
String methods and String-key sorting through a List result. Source-free execution
forces GC and checks root/object/depth cleanup.
Native preparation explicitly falls back before entry for unsupported library calls;
the scalar entry must still execute actual native code.

`artifact_features::portable_fixture_matches_source_emission` checks canonical
bytes against current compilation. Regenerate the fixture after source or contract
changes from the repository root:

```text
cargo run -p kagari-embed --example regenerate_feature_artifact
cargo test -p kagari-embed --test artifact_features
uv run python scripts/check_features.py
```

Regeneration replaces the current fixture; it does not add a compatibility reader
or relax version checks. Keep canonical-byte, rejection and execution assertions.
