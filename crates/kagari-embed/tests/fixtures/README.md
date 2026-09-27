# SDK feature-boundary artifact

`feature_artifact.kbc` is the current-format artifact emitted for
`feature_artifact.kgr` with logical source name `memory://feature-artifact.kgr`.
It contains bytecode and portable MIR, without source text. Tests consume these
same bytes with SDK features disabled, with `source`, with `native`, and with both.
The native test uses a static ABI fixture; it is not evidence of Cranelift codegen.

`artifact_features::portable_fixture_matches_source_emission` checks that the
fixture matches the current compiler. Regenerate it when the artifact contract
changes, using an SDK build with `source` enabled and this Rust code (paths are
relative to this directory):

```rust
use kagari_common::SourceFile;
use kagari_embed::KagariEngine;
use std::fs;

let source = SourceFile::new(
    "memory://feature-artifact.kgr",
    fs::read_to_string("feature_artifact.kgr").unwrap(),
);
let artifact = KagariEngine::default()
    .compile_to_artifact(source, Default::default(), Default::default())
    .unwrap();
fs::write("feature_artifact.kbc", artifact.to_bytes().unwrap()).unwrap();
```

Regeneration updates the current fixture, not a compatibility reader or old-format
acceptance policy. The test must continue checking canonical bytes and execution.
