# Kagari standard library

The native-provider reset removes predecessor algorithms. The installed package
currently proves a small declaration-to-artifact slice:

- [array](array.kgr): ArrayList new/len/push/from_fn, List len/get and indexed views.
- Other modules retain required protocols and native representations; their old
  public implementations are temporarily unavailable.

HIR checks source signatures against source-free kagari-stdlib-provider descriptors.
Runtime registrations own handlers; MIR/bytecode carry checked imports and the
common frame driver owns invocation roots and callbacks. Source annotations never
grant native authority themselves.

The [active plan](../docs/native-provider-refactor.md#reset-execution-checkpoint-2026-10-01)
records the data model, runnable proof, API restoration and carried test errors.
Existing behavior tests remain required for final acceptance.
