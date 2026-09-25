# Module loading and activation

Import resolution is a compile-time and link-time operation. It does not execute script code. Cyclic imports are allowed when their declarations and linked contracts are valid.

Each runtime owns its mutable module instances, heap, permissions, budgets, and caches. Verified code and layouts may be shared. Instances have no initialization state or cached module result. A function executes only when the host or another script function calls it by a resolved entry or slot.

Loading validates the entire executable closure before publishing a handle. Reload prepares and stages a candidate, checks its schema, ABI, host bindings, capabilities, expected base version, and candidate-owned object references, then publishes. Ordinary reload does not run any candidate function. The current entry is unchanged if a check fails. Staged candidates are discarded when dropped.

An embedding host may explicitly call functions during a restricted candidate session. That session can mutate candidate-owned state, perform pure computation, and read explicitly immutable configuration; external modifications are denied. Termination and resource errors invalidate the candidate. Publication rechecks the base version and candidate references after the session ends. Publication is per runtime; coordination across actors belongs to the host.

The root call pins a dependency program. Nested calls and synchronous host reentry use that same version set, permissions, and budget. A later publication does not redirect existing calls. Old versions remain usable while reachable and are reclaimed after their remaining references and calls end.
