# Kagari standard library

The native-provider restoration starts with two optional registered modules:

- [array](array.kgr): ArrayList new/len/push/from_fn, List len/get, MutableList.set
  and indexed views.
- [math](math.kgr): checked floor/ceil/sqrt on f64 values.
- Other modules retain required protocols and native representations; their old
  public implementations are temporarily unavailable.

Rust `#[native_module]` registrations own declarations and handlers. HIR imports
registered records directly; generated `.kgr` files provide tooling syntax,
documentation and navigation. Array and math text is never lowered to establish
native signatures. Remaining modules still use the legacy source package during
migration. MIR/bytecode carry checked imports, and the common frame driver owns
invocation roots and callbacks. Source annotations do not grant native authority.

Engine installs the ordinary native packages by default; applications can disable
that selection and install their own packages through the same checked API.

The [active plan](../docs/native-provider-refactor.md#full-library-restoration-sequence-2026-10-01)
records restoration order, acceptance conditions and carried errors. Existing
behavior tests remain required for final acceptance.
