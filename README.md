# Kagari Language

Kagari is an early-stage strongly typed scripting language. It adopts a Rust-inspired syntax style, while deliberately avoiding Rust's native lifetime and borrow-checking model as a language feature that script authors must work with directly.

The current goal of the project is to build a language system suitable for embedding into host applications: one with clear type-system boundaries, stable runtime abstractions, strong hot-reload potential, and low-friction interoperability with Rust.

## Project Status

Kagari is currently in the research and foundation-building stage. The repository primarily provides an early skeleton for the frontend, semantic analysis, IR, runtime, and virtual machine so that future language and runtime work can evolve on top of clear engineering boundaries.

This currently means:

- The syntax and standard library are not finalized
- The type system is still expected to evolve
- The runtime, GC, hot reload, and host ABI are still represented mostly by early abstractions and extension points
- AOT and JIT are future directions rather than near-term delivery targets

## Design Direction

Kagari is currently being shaped around the following principles:

- A strongly typed scripting language rather than a weakly typed dynamic one
- A syntax style that remains close to Rust in order to reduce context switching for Rust users
- No direct reproduction of Rust's lifetime and borrow system at the script language level
- A GC-backed runtime responsible for script-owned memory
- Hot reload as a first-class concern, with module loading and version evolution treated as core capabilities
- Natural interoperability with Rust hosts, especially around controlled access to borrowed and mutably borrowed host data
- A clean separation between frontend, intermediate representation, and execution backends so the project can grow toward interpretation, AOT, and JIT without rewriting the whole stack

## Runtime and Host Interoperability Principles

One of Kagari's intended roles is to serve as an embeddable scripting layer for Rust applications. To support that goal, the repository currently follows these principles:

- Script-owned objects and host-borrowed objects should remain explicitly distinct
- GC is responsible for script-owned data, not for the borrowed lifetime of host-side references
- Host references and mutable references passed into scripts should be governed through call-frame-scoped handles or equivalent boundary rules
- The language frontend should not depend directly on runtime implementation details
- Interpreter, AOT, and JIT backends should share the same semantic-analysis and IR boundary

The aim is to keep the scripting model ergonomic without giving up the host application's control over data validity and call-time constraints.

## Repository Layout

The repository is organized as a Rust workspace so that major responsibilities are separated early:

- `kagari-common`: shared foundational types such as source files, spans, and diagnostics
- `kagari-syntax`: lexer, parser, and AST
- `kagari-sema`: name resolution, builtin types, semantic analysis, and type-checking scaffolding
- `kagari-ir`: lowering from typed semantics into IR and bytecode-oriented forms
- `kagari-runtime`: runtime abstractions, GC placeholders, host ABI boundaries, and hot-reload metadata
- `kagari-vm`: the initial interpreter layer
- `kagari-cli`: a thin command-line entry point for driving the pipeline

This structure is not meant to make the project unnecessarily complex early on. Its purpose is to prevent syntax, semantics, runtime logic, and execution backends from becoming tightly coupled as the project grows.

## Naming Conventions

The project currently uses the following naming conventions:

- Source file extension: `.kgr`
- Package manager name: `kg`
- Bytecode artifact extension: `.kbc`

These names are intended to serve as the baseline vocabulary for the future toolchain, module system, and build outputs.

## Engineering Priorities

At the implementation level, Kagari currently prioritizes the following:

- Stabilize the frontend, semantic layer, and IR boundary before expanding backend complexity
- Establish a verifiable interpreter pipeline before pursuing more aggressive optimization paths
- Design hot reload into the module system and runtime rather than adding it later as an afterthought
- Define host ABI safety boundaries before adding convenience-oriented syntax
- Keep the runtime independent from AST details

## What the Repository Already Provides

The current codebase already includes the following minimal building blocks:

- A runnable workspace structure
- Basic source and diagnostic types
- A small lexer and parser skeleton
- An initial name-resolution and builtin-type-checking flow
- A lowering path from semantic results into an initial IR
- Basic abstractions for the runtime, hot-reload epochs, and host function registration
- A minimal runnable VM entry example

These pieces exist primarily to validate architectural boundaries. They should not be interpreted as evidence that the language itself is already feature-complete.

## Expected Areas of Growth

In later stages, the project will likely continue to focus on:

- Full expression, statement, module, and type-annotation syntax
- A stricter and more extensible type system
- Host type registration, reflection, and function binding mechanisms
- A clearer bytecode format and module loading protocol
- A maintainable GC object model
- Module version management and state migration strategies for hot reload
- AOT and JIT backend experiments aimed at performance-oriented use cases

## Note

Kagari is still an early project. Many modules in this repository exist to stabilize system boundaries as early as possible rather than to provide a polished language experience today. The intention is to ensure that later work on language design, runtime behavior, and host integration can proceed on top of a clear and durable structure.
