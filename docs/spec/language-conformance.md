# Language conformance entry

Run `cargo test -p kagari-vm language_contract` for the shared observable suite.
Fixtures live in `crates/kagari-vm/src/tests/language_contract.rs` and contain:

- source text and an expected value, diagnostic code, or failure category;
- the ordered host calls and their argument values;
- committed host mutation records and expected host state;
- optional deterministic host rejection/cancellation and repeated entry invocation;
- optional rooted array input and its expected contents after success or failure.

Every executable fixture uses four fresh runtimes: source/interpreter,
artifact/interpreter, source/JIT and artifact/JIT. Both artifact routes serialize,
decode and validate the artifact before runtime loading. Both JIT routes permit
the existing interpreter fallback; they do not imply every fixture is native code. Diagnostic
fixtures must fail the checked-analysis boundary before any backend runs.

Selected scalar overflow fixtures require a recorded native invocation on both
source/JIT and artifact/JIT routes. They
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
Assignment fixtures check target/index/RHS call order, reading RHS-updated values,
rejected removed locations, captured root identity, and tuple value updates. These
use the existing JIT fallback for local and aggregate operations. The optional array observer declares `observe.array` offline and binds it to an
explicit rooted handle in each runtime. Calls are recorded alongside ordinary host
calls. After execution the fixture checks that only the observer root remains,
collects garbage and compares the retained array contents. This covers completed
writes surviving overflow/index traps, host rejection, cancellation and instruction
budget exhaustion, removed targets staying removed, and compound
assignment reading a value changed by its RHS. It exercises JIT fallback for these
container operations. Heap mutation event records and observers for other object
kinds remain pending; final contents do not establish a full write-event trace.

For example, `.array(&[2147483647, 0], &[2147483647, 42])` supplies the initial
array and expects the first element unchanged after overflow while an earlier
write of `42` to the second element survives. This is an explicit test-host input,
not inspection of private bytecode registers or a raw unrooted `Value`.
Unimplemented contract cases are tracked in the foundation roadmap; no ignored
test is evidence of conformance.

Cancellation fixtures install an explicit execution session and request cancellation
from a committed host log callback. Rejection is a distinct outcome: it records the
call without committing its log append. Budget fixtures finish their initial heap
and host writes, then exhaust the instruction budget in the compound assignment's
RHS. In all three cases no final assignment is committed, and call-depth/root
cleanup is checked before post-execution collection and observation. Cancellation
and host-rejection positions count the full host-call sequence, including the array
provider call. The enclosing cancellation session is released before collection.

Host-iteration fixtures keep a collection guard alive while script code receives
the rooted array. They verify that element replacement succeeds but push, insert,
pop, remove and clear each trap before changing structure. Earlier host effects
and element writes survive. After execution the host drops its guard, collects,
checks contents, and performs a push/pop to prove structural access is restored.
These run on all four routes and check the standard-library error category and
operation context. They establish script enforcement of a host-owned guard;
source callback and for-loop integration is still a separate incomplete requirement.
