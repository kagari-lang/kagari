# Kagari Security and Sandboxing Draft

This document describes a proposed security model for Kagari.
It is a design draft, not a finalized implementation contract.

The main goal is to let host applications safely embed Kagari while retaining fine-grained control over language features, runtime capabilities, and exposed host APIs.

Host interop direction is drafted separately in [host-interop.md](/Users/mikai/CLionProjects/kagari/docs/spec/host-interop.md).
Runtime model direction is drafted separately in [runtime.md](/Users/mikai/CLionProjects/kagari/docs/spec/runtime.md).

## Design Goals

- allow hosts to disable high-risk language capabilities
- allow hosts to restrict runtime powers such as IO and networking
- keep baseline language syntax stable across embeddings
- separate compile-time feature gating from runtime permission checks
- support resource limits suitable for untrusted or semi-trusted scripts

## Non-Goals

The first security version should not attempt to provide:

- OS-level isolation by itself
- a guarantee that front-end checks alone are sufficient
- arbitrary per-host rewrites of core language grammar
- a replacement for process sandboxing when hostile native code is involved

Kagari should complement host-side and OS-side defenses, not replace them.

## Core Principle

Security control should be split into four layers:

1. language profile
2. runtime capabilities
3. host exports
4. resource policy

This separation is important.

If all control is pushed into one mechanism, the result becomes hard to reason about.
For example, "this syntax is forbidden" and "this API exists but is denied at runtime" are different kinds of restrictions and should be modeled separately.

## Layer 1: Language Profile

The language profile determines which higher-level language features are allowed in a given embedding context.

Examples of profile-controlled features:

- reflection support
- reflection write access
- dynamic trait objects
- dynamic module loading
- `eval`
- async or concurrency features
- host escape hatches such as unsafe host interop

This layer should generally not be used to disable basic core syntax such as:

- `fn`
- `if`
- `match`
- `loop`
- `struct`
- `enum`
- ordinary closures

Those features should remain part of the stable language.

### Example Profile

```text
LanguageProfile {
  allow_reflection: true,
  allow_reflection_write: false,
  allow_dyn_trait: true,
  allow_dynamic_load: false,
  allow_eval: false,
  allow_async: false
}
```

### Enforcement Guidance

Language profile checks should happen after parsing in a dedicated validation pass.

Recommended pipeline:

1. parse source into AST
2. resolve names and basic semantics
3. run feature validation against the active profile
4. reject unsupported language constructs with diagnostics

This is preferable to making the parser itself host-specific.

## Layer 2: Runtime Capabilities

Runtime capabilities determine what a script is allowed to do when it executes.

Examples:

- file read
- file write
- networking
- clock access
- randomness
- module loading
- host reflection read
- host reflection write
- dynamic invocation

### Example Capability Set

```text
CapabilitySet {
  fs_read: false,
  fs_write: false,
  net: false,
  clock: true,
  random: true,
  reflection_read: true,
  reflection_write: false,
  dynamic_load: false
}
```

### Enforcement Guidance

Capability checks must happen at runtime entry points, not only in the front end.

For example:

- file APIs check `fs_read` or `fs_write`
- reflective field writes check `reflection_write`
- dynamic loading checks `dynamic_load`

The front end may reject obviously forbidden operations when the profile is known ahead of time, but runtime checks remain necessary because capability state is part of the execution environment.

## Layer 3: Host Exports

The most effective security control is often to avoid exposing dangerous capabilities in the first place.

Kagari should not assume that IO, networking, process control, or reflection over host objects are built-in universal powers.

Instead, the host should explicitly export what scripts may access.

### Example Host Export Model

```text
HostExports {
  modules: ["log", "ui"],
  functions: ["log.info", "ui.draw_text"],
  types: ["Player", "Vec2"]
}
```

Recommended rule:

- if the host does not export a capability-bearing API, script code cannot use it

This is usually safer than exposing an API and then denying it dynamically.

## Layer 4: Resource Policy

Sandboxing is not only about semantic permissions.
It also needs resource controls.

Recommended resource limits include:

- maximum instruction steps
- maximum recursion depth
- maximum heap size
- maximum allocation rate
- maximum number of loaded modules
- execution timeout budget

### Example Resource Policy

```text
ResourcePolicy {
  max_steps: 10_000_000,
  max_call_depth: 256,
  max_heap_bytes: 64_MB,
  max_modules: 128,
  max_wall_time_ms: 100
}
```

These checks should be enforced inside the VM or interpreter loop and allocator boundary rather than relying only on cooperative script behavior.

## Why Core Syntax Should Stay Stable

Hosts may want to remove dangerous features, but it is usually a mistake to let every embedding arbitrarily redefine the core grammar.

Problems caused by host-specific grammar subsets:

- tooling fragmentation
- confusing diagnostics
- poor script portability
- difficulty sharing libraries across hosts

Recommended rule:

- stable syntax for the core language
- feature-gating for high-risk semantic features
- runtime and host-export control for dangerous effects

## Reflection and Security

Reflection is powerful and should be controlled explicitly.

Recommended split:

- reflection metadata read may be allowed
- reflection-based mutation may be separately gated
- host objects should not automatically expose reflective write access

This works well with the reflection design in [reflection.md](/Users/mikai/CLionProjects/kagari/docs/spec/reflection.md).

### Recommended Reflection Gates

```text
allow_reflection
allow_reflection_write
host_reflection_read
host_reflection_write
```

## Traits and Security

Trait use by itself is not a security problem.
The security-relevant part is what dynamic behavior traits unlock.

Examples:

- `dyn Trait` may be disabled in a restricted profile
- downcast may be permitted while reflective mutation is denied
- trait-based host APIs still depend on host export policy

Trait-system direction is drafted in [traits.md](/Users/mikai/CLionProjects/kagari/docs/spec/traits.md).

## Modules and Loading

Module import and module loading should be treated separately.

- `use` and static module references are language structure features
- runtime module loading is a capability

Recommended control:

- keep static `mod` and `use` in the language
- gate dynamic loading through both the language profile and capability set
- optionally restrict imports with allowlists

### Example Module Policy

```text
ModulePolicy {
  allowed_import_roots: ["core", "game", "ui"],
  allow_dynamic_load: false
}
```

## Host Object Exposure

Host objects need especially clear rules because they cross the script/host trust boundary.

Recommended policy:

- host types are opaque by default
- host functions are unavailable by default
- host reflection is opt-in
- host mutation is opt-in
- host-side registrations should declare required capabilities

This should align with Kagari's existing distinction between script-owned and host-borrowed data.

## Compile-Time vs Runtime Enforcement

The recommended rule is:

- use the language profile for compile-time validation
- use capabilities and resource policy for runtime enforcement
- use host exports to shape what code can name at all

This keeps the architecture understandable.

### Example

Suppose a host disables reflection writes.

Possible behavior:

- if the host compiles the script with a profile that forbids reflection writes, reflective write syntax or APIs are rejected during validation
- if a script reaches a reflective write path at runtime without permission, the runtime still rejects it

Both checks are useful, but they solve different problems.

## Recommended Default Posture

The safest default embedding should look roughly like this:

```text
LanguageProfile {
  allow_reflection: true,
  allow_reflection_write: false,
  allow_dyn_trait: true,
  allow_dynamic_load: false,
  allow_eval: false,
  allow_async: false
}

CapabilitySet {
  fs_read: false,
  fs_write: false,
  net: false,
  clock: true,
  random: true,
  reflection_read: true,
  reflection_write: false,
  dynamic_load: false
}

HostExports {
  modules: ["core", "log"],
  functions: ["log.info"],
  types: []
}
```

This gives scripts useful language features without ambient authority.

## Recommended Implementation Order

If implemented incrementally, the recommended order is:

1. explicit host export registry
2. runtime capability checks for dangerous APIs
3. resource policy enforcement in the VM
4. language-profile validation pass
5. finer-grained reflection and module policies

This order provides practical safety early, without requiring the entire language front end to be feature-configurable from day one.
