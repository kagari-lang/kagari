# Language conformance entry

Run `cargo test -p kagari-vm language_contract` for the shared observable suite.
Fixtures live in `crates/kagari-vm/src/tests/language_contract.rs` and contain:

- source text and an expected value, diagnostic code, or failure category;
- the ordered host calls and their argument values;
- committed host mutation records and expected host state;
- optional deterministic host rejection and repeated entry invocation.

Every executable fixture uses fresh runtimes for source bytecode, serialized
artifact loading, and the existing JIT. The JIT route permits its existing
interpreter fallback; it does not imply every fixture is native code. Diagnostic
fixtures must fail the checked-analysis boundary before any backend runs.

Selected scalar overflow fixtures require a recorded native invocation. They
cover add/subtract/multiply/negate, intermediate overflow, and budget exhaustion
before arithmetic. Division uses the existing interpreter fallback. Runtime
failures retain their structured category across the native ABI; they never
trigger a second execution through fallback. Path arithmetic has focused tests
for unchanged target state, zero write callbacks and zero dirty records on failure.

The recording host exposes `host.log`, as used by the source `print` builtin.
A call is recorded before its configured outcome. A successful append records
the target, previous length, and appended value. A rejected append produces no
mutation record. Calls after a trap must not occur, while earlier committed
appends remain. These records describe the test host, not a script-heap write
barrier or a transaction/replay facility.

The authority for expected behavior remains [value semantics](value-semantics.md),
[failure semantics](failure-semantics.md), and [module activation](module-activation.md).
New contract behavior extends this suite alongside focused subsystem tests.
Unimplemented contract cases are tracked in the foundation roadmap; no ignored
test is evidence of conformance.
