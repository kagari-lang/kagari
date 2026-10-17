# Source package workspace

This virtual workspace contains `gameplay` and `combat-rules`. It selects
`gameplay` by default, inherits the `rules` dependency alias explicitly and compiles
the selected root plus its source dependencies into one Program. `combat-rules`
needs no `main`; it can also be built as an independent root.

Run from the repository root:

```text
cargo run -p kagari-cli -- check --manifest-path examples/package-workspace/Kagari.toml --workspace
cargo run -p kagari-cli -- build --manifest-path examples/package-workspace/Kagari.toml --workspace --locked
cargo run -p kagari-cli -- run --manifest-path examples/package-workspace/Kagari.toml --frozen
cargo run -p kagari --example package_workspace
```

The first command maintains the checked-in shared `Kagari.lock`. Build outputs are under this
workspace's `target/kagari`; the script entry computes `42` and the run command
exits successfully (ordinary scalar returns are not printed). The SDK example loads
read-only, checks the host-supplied namespace against the manifest and invokes
`on_login`, `tick` and the re-exported `damage` from the same Program, producing
`on_login=40, tick=2, damage(21)=42`. It does not invoke `main` implicitly.

The root's private `events` module implements the public `on_login` re-export.
`rules::bonus<T>` specializes at the source call in `tick`; a generic template is
not a host-callable entry. Changing `rules` affects both selected roots, while
changing `gameplay` affects only that root. See [package inputs and updates](../../docs/spec/packages.md)
for workspace identity, locking and host-coordinated rebuild/reload.
