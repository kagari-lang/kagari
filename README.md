# Kagari Language

Kagari is an early-stage strongly typed scripting language. It adopts a Rust-inspired syntax style, while deliberately avoiding Rust's native lifetime and borrow-checking model as a language feature that script authors must work with directly.

The current goal of the project is to build a language system suitable for embedding into host applications: one with clear type-system boundaries, stable runtime abstractions, first-class hot reload, and low-friction interoperability with Rust.

## Project Status

Kagari is still an early language, but the repository now contains an end-to-end implementation slice. The implemented pipeline includes parsing, semantic analysis, IR lowering, bytecode lowering and verification, `.kbc` artifact construction and validation, VM execution, hot-reload validation metadata, host registration boundaries, cooperative cancellation, debugger hooks, and an optional baseline Cranelift JIT backend.

This currently means:

- The specifications in `docs/spec/`, `docs/kagari.ebnf`, and `docs/architecture.md` are authoritative over legacy implementation behavior.
- The source frontend compiles [Kagari's language core](library/core/). Native library methods and host services require explicit installation.
- Script integers are fixed-width `i8`/`i16`/`i32`/`i64` and `u8`/`u16`/`u32`/`u64`, with no machine-width aliases. Standard lengths, counts and indices use `u64`; [memory and resource limits](docs/spec/runtime.md#machine-and-resource-boundaries) remain platform-dependent.
- Interpreter execution is the semantic foundation.
- The Cranelift JIT is optional and feature-gated; unsupported functions or debug-policy conflicts fall back to the interpreter.
- Runtime, host interop, reload, reflection, and debugger APIs expose structural boundaries, while production polish continues beyond the current implementation slice.

## Documentation

- [Agent and engineering guidelines](AGENTS.md)
- [Language guide source and local preview](site/README.md) — 25 guide/tool pages including a browser Playground; live Pages acceptance remains separate
- [Documentation index](docs/README.md)
- [Project goal](docs/project_goal.md)
- [Architecture](docs/architecture.md)
- [Implementation roadmap](docs/implementation-roadmap.md)
- [Performance measurements](docs/performance-baseline.md)
- [Runnable language examples](examples/README.md)
- [Syntax grammar](docs/kagari.ebnf)
- [Embedding API specification](docs/spec/embedding-api.md)
- [Module loading specification](docs/spec/module-loading.md)
- [Native declarations and explicit registration API](docs/spec/standard-declarations.md)
- [Builtins and standard library specification](docs/spec/builtins.md)
- [Bytecode artifact specification](docs/spec/artifacts.md)
- [Debugger specification](docs/spec/debugger.md)
- [Baseline JIT specification](docs/spec/jit.md)

## Design Direction

Kagari is currently being shaped around the following principles:

- A strongly typed scripting language rather than a weakly typed dynamic one
- A syntax style that remains close to Rust in order to reduce context switching for Rust users
- No direct reproduction of Rust's lifetime and borrow system at the script language level
- A GC-backed runtime responsible for script-owned memory
- Hot reload as a first-class concern, with module loading and version evolution treated as core capabilities
- Natural interoperability with Rust hosts through checked Host adapters, retained Opaque payloads, and typed path mutation
- A clean separation between frontend, intermediate representation, bytecode, interpreter execution, and optional machine-code backends

## Runtime and Host Interoperability Principles

One of Kagari's intended roles is to serve as an embeddable scripting layer for Rust applications. To support that goal, the repository currently follows these principles:

- Script-owned objects and host-borrowed objects should remain explicitly distinct
- GC is responsible for script-owned data, not for the borrowed lifetime of host-side references
- Host references and mutable references passed into scripts should be governed through call-frame-scoped handles or equivalent boundary rules
- The language frontend should not depend directly on runtime implementation details
- Interpreter and JIT backends should share the same semantic-analysis, typed IR, bytecode, runtime-helper, and metadata boundaries

The aim is to keep the scripting model ergonomic without giving up the host application's control over data validity and call-time constraints.

## Repository Layout

The repository is organized as a Rust workspace so that major responsibilities are separated:

The [architecture](docs/architecture.md) describes the workspace crates;
the [roadmap](docs/implementation-roadmap.md) records pending work.

- `kagari-common`: portable identities, numeric semantics, limits and host schemas
- `kagari-source`: source documents, revisions, diagnostics, literals and navigation provenance
- `kagari-syntax`: lexer, parser, CST and AST views
- `kagari-hir`: owned ID-linked HIR, recoverable resolution/typing and checked compiler admission
- `kagari-analyzer`: protocol-neutral `.queries()` views and demand-built IDE indexes; separate stdio LSP lifecycle
- `kagari-lint`: optional rules and private usage/attribute-policy projections
- `kagari-abi`: physical representations, helper calls and native code ownership
- `kagari-contract`: semantic declarations, checked calls/layouts and portable verification
- `kagari-mir`: concrete CFGs, verification, analyses, passes and portable encoding
- `kagari-compiler`: checked source-to-MIR and MIR-to-bytecode lowering, native links
- `kagari-bytecode`: interpreter model, validation, codec and `.kbc` envelope
- `kagari-codegen`: verified MIR compilation interface
- `kagari-codegen-cranelift`: optional scalar native compiler and code-memory ownership
- `kagari-runtime`: canonical Program/Runtime, interpreter/debugger, async tasks, GC, Host/Native APIs and reload
- `kagari`: host SDK with independent `source` and `native` features
- `kagari-cli`: the `kg` executable, arguments, filesystem IO and presentation

This structure prevents syntax, semantics, runtime logic, and execution backends from becoming tightly coupled as the project grows.

## Naming Conventions

The project currently uses the following naming conventions:

- Source file extension: `.kgr`
- Rust SDK package and import: `kagari`
- CLI executable: `kg` (Cargo package `kagari-cli`)
- Bytecode artifact extension: `.kbc`

These names are intended to serve as the baseline vocabulary for the future toolchain, module system, and build outputs.

## CLI

The `kg` CLI uses the same compilation/execution pipeline as hosts and also provides
offline artifact inspection. Build it with
`cargo build -p kagari-cli --bin kg`, or run it through Cargo:

```sh
cargo run -p kagari-cli -- parse path/to/main.kgr
cargo run -p kagari-cli -- fmt path/to/main.kgr --check
cargo run -p kagari-cli -- check path/to/main.kgr
cargo run -p kagari-cli -- lint path/to/main.kgr -D warnings
cargo run -p kagari-cli -- emit -o path/to/main.kbc path/to/main.kgr
cargo run -p kagari-cli -- run path/to/main.kgr
cargo run -p kagari-cli -- run-artifact path/to/main.kbc
```

For source-tool/project commands, omit the file to select project members;
`--workspace` selects the workspace.
`fmt` changes layout, `check` enforces language rules, and `lint` adds six optional
rules. See [source tools](docs/spec/toolchain-tools.md) for configuration, JSON,
suppressions, source acquisition and writeback guarantees.

An implicit path selects `run-artifact` for `.kbc` files and `run` for other paths.
Installed interfaces determine available APIs; execution supports cancellation and a runtime call-depth limit.
`--jit` requests JIT execution when the binary is built with the `jit` feature; `--no-jit` forces interpreter execution.

Inspect an existing artifact without source files, installed host providers or
execution:

```sh
kg disasm path/to/main.kbc --list-functions
kg disasm path/to/main.kbc --function '@0:3'
kg disasm path/to/main.kbc > main.disasm.txt
```

Choose a slot from the listing, or pass an exact raw qualified name to `--function`.
All modes validate the entire current-format artifact first. Instruction numbers
are zero-based indices, not byte offsets; the output is human-readable bytecode,
not reconstructed source or a stable machine protocol. See the
[disassembly contract](docs/spec/toolchain-tools.md#kbc-disassembly) for selection,
exit codes and output limits. With Cargo, use `cargo run -p kagari-cli -- disasm`
in place of `kg disasm`.

## Standard Library

Source compilation includes eleven authored modules. They define traits such as
`PartialEq` and `Iterator`, the `Option` and `Result` enums, and Script defaults.
A collection can supply its own cursor and call `bag.iter().map(f)` without a
bundled collection or any Rust registration. Compiled Script code runs without
core source files or stdlib at execution.

The optional `kagari-stdlib` supplies the `std` prelude and re-exports, six
collection interfaces, and native operations such as String's `trim`. Importing
`alloc::string::String` alone does not install that method. Bundled `Vec`,
`HashMap` and `HashSet` implementations are currently withdrawn. See the
[foundation inventory](docs/spec/builtins.md#foundation-trait-inventory) and
[native registration guide](docs/native-registration.md).
Host-sensitive capabilities such as I/O remain explicit embedding interfaces.

## Engineering Priorities

At the implementation level, Kagari currently prioritizes the following:

- Stabilize the frontend, semantic layer, and IR boundary before expanding backend complexity
- Establish a verifiable interpreter pipeline before pursuing more aggressive optimization paths
- Design hot reload into the module system and runtime rather than adding it later as an afterthought
- Define host ABI safety boundaries before adding convenience-oriented syntax
- Keep the runtime independent from AST details

## What the Repository Already Provides

The current codebase includes:

- A runnable workspace structure
- Source, span, and structured diagnostic types with stable diagnostic codes
- Lexer, parser, AST, HIR lowering, name resolution, builtin metadata, and semantic checks
- Bounded lowering from checked source to concrete verified MIR, sealed analyses and bytecode
- Source-defined core protocols, generic iteration and propagation, with optional native library implementations
- `.kbc` artifact metadata, validation, and current Rust serialization helpers
- Runtime values, host function/type registration, declared access checks, cancellation, module epochs, and reload validation
- A bytecode VM with debugger hooks, breakpoint resolution, stepping state, watch evaluation, and adapter-boundary request/event types
- An optional verified-MIR Cranelift compiler behind `CodegenBackend`, with runtime-owned invocation
- A CLI and embedding facade that share compile, artifact, load, execute, and reload boundaries

## Expected Areas of Growth

Near-term work continues to focus on:

- Expanding conformance coverage for syntax, semantics, host interop, reload, debugger, security, reflection, and JIT policy
- Filling out the remaining specified language and builtin surface
- Hardening artifact encoding, loader policy, GC behavior, host APIs, and production diagnostics
- Extending the baseline JIT while preserving interpreter semantics

## Note

Kagari is still an early project. The current implementation is useful for validating the architecture and exercising the language pipeline, but the specifications remain the source of truth while later roadmap work fills out production details.
