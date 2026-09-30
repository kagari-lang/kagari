# Permissions and Execution Protection Refactor Plan

Status: queued, documentation only. This plan replaces layered per-execution
authorization and exact logical charging with installation-based API access and
lightweight protection against runaway execution. The user approved the design
direction, not implementation now. Do not commit this planning work without renewed
permission.

Proposed execution order is ST06 acceptance, NR05 acceptance in the
[native provider refactor](native-provider-refactor.md), this plan, then activation
of the [async execution design](async-execution-design.md). Re-audit completed
predecessors before starting. Existing specifications remain authoritative until
the implementing phase updates them; this plan does not weaken ST or NR acceptance.
The [roadmap](implementation-roadmap.md) records this queued dependency.

## Approved design boundary

One runtime serves one host-selected trust environment. The host may embed trusted
or untrusted scripts, but does not need different privilege levels within one
runtime. Installing and exposing a Native function authorizes script use of that
function. Resource protection exists to stop runaway work, not to provide exact
metering or identical exhaustion positions across optimization levels/backends.
Performance must be measured; exact accounting must not dictate the hot path.

The target is:

- The installation defines available APIs, types and objects.
- Runtime limits protect the heap and call depth.
- Each root execution has a coarse work allowance and cancellation state.
- The host manages task scheduling, deadlines, service limits and business access.

No engine-wide `trusted` flag is needed to turn correctness checks off. Different
exposure requirements use different installations/runtimes. Registration remains
a trusted host action, not something artifact bytes or copied source attributes
can perform. Host Native implementations must themselves be trusted; this is not
OS isolation for arbitrary Rust code.

## Permission inventory

| Area | Target rule |
| --- | --- |
| Native calls | A registered and exposed implementation may be called through its checked contract; no second per-call API allowlist |
| Host types and objects | Only explicitly exposed types, instances and members are accessible; validate object identity and lifetime |
| Fields | Follow declaration writeability and exposed read/write adapters; no separate manual field-read or field-write permission configuration |
| Files, network, time and randomness | Expose selected Native APIs; remove generic capability bits such as `net` and `fs_read` |
| Module loading and reload | Host SDK controls loading/publication; script access requires an explicitly installed host API |
| Reflection | Optional installed surface; it cannot bypass member visibility, writeability or host adapter constraints |
| Debugger and JIT | Host setup/tooling configuration, not an execution-time script permission matrix |
| Async launch | A valid exposed host-scope handle selects lifecycle/scheduling ownership; no per-task capability derivation |

`val`/`var`, readonly views, field types, validated host paths and borrow rules are
language/interface contracts. Providing a writable host adapter expresses supported
mutation; the host should not also need `allow_path_mutation` on every execution.
Business rules such as whether a player may update an order stay inside the host
operation or application logic, not in the engine's general permission model.

Installation and executable linking remain distinct. Offline declarations can be
used for compilation without handlers; execution still requires matching trusted
registrations. Binding identity/version checks, representation validation, native
effects and bounded decoding remain mandatory. Registration grants access, not
permission to execute a mismatched or forged call contract.

## Current audit and removal targets

The following paths were inspected at revision `136ec596`. They are an inventory,
not a guarantee that the post-NR05 implementation retains these exact owners.

| Current mechanism | Intended treatment |
| --- | --- |
| [LanguageProfile and SecurityContext](../crates/kagari-runtime/src/security.rs) | Remove duplicated script authorization gates; retain only justified host installation/tool settings in their actual owners |
| [CapabilitySet](../crates/kagari-common/src/capability.rs) | Remove the generic boolean permission matrix from runtime and portable host declarations |
| [HostExposurePolicy](../crates/kagari-runtime/src/security.rs) | Replace per-execution function/module/type allowlists with the installed surface itself |
| [Boundary authority checks](../crates/kagari-runtime/src/authority.rs) | Remove policy conjunctions, retaining contract, identity, lifecycle and candidate-phase checks |
| [HIR feature validation](../crates/kagari-hir/src/profile.rs) | Resolve against installed declarations and actual supported features, not a per-call permission profile |
| [ExecutionContext](../crates/kagari-embed/src/context.rs) | Stop carrying language/capability/exposure matrices; keep execution controls and unrelated tracing/input behavior |
| [ResourcePolicy and counters](../crates/kagari-runtime/src/resource.rs) | Separate runtime protection, per-root coarse work and optional observation |
| [LogicalBudgetCharge](../crates/kagari-abi/src/budget.rs) | Replace exact per-operation charging contracts, including their optimization/backend obligations |
| [VM execution loop](../crates/kagari-vm/src/executor/mod.rs) | Use verified bounded polling/work regions instead of mandatory exact step helpers |

At present, ResourcePolicy defaults contain no finite limits. Runtime accessors
prefer the active session's security/resource configuration; these are not a
hierarchy that automatically intersects a runtime ceiling with a request ceiling.
Do not carry this accidental configuration shape into the target API.

## Protection inventory

| Control | Owner | Meaning |
| --- | --- | --- |
| `max_work` | Root execution | Coarse work allowance shared by nested calls, callbacks and reentry |
| `max_call_depth` | Runtime configuration | Maximum permitted execution-stack depth |
| `max_heap_units` | Runtime configuration | Maximum tracked live script-heap/native preparation occupancy |
| Cancellation | Root execution | Sticky cooperative termination, independent of remaining work |
| Drive slice | Host driver | Bounded scheduling quantum; exhaustion yields runnable work rather than failing it |
| Unfinished job count | Host task scope | Admission/lifetime bound, not a hierarchy of resource accounts |

The last two controls belong to the future async driver/host integration, not new
features to implement in this synchronous refactor. No separate script allocation
quota, reflection counter quota or universal host-call quota is required.

Future concrete public types should reflect these owners, for example runtime
limits and per-execution controls rather than one ResourcePolicy copied everywhere.
Names and numeric defaults are decided at EP00 from workloads and embedding needs.
The host may explicitly disable a limit, but disabling work accounting must not
disable cancellation, bounds checks, GC safety or artifact verification. Document
which limits an untrusted-script embedding must configure for its protection goals.

### Coarse work semantics

Work units are implementation-defined protection estimates, not instruction counts,
CPU time, billing units or a cross-backend reproducibility contract. Optimized-away
work need not be charged. Interpreter and JIT may exhaust the same configured value
at different points. No empty instruction may exist solely to preserve superseded
precise charging behavior.

Use bounded regions and checkpoints such as loop backedges, function entries,
allocation boundaries and chunks of long-running native operations. Decide actual
placement and batching from measurements, not by prescribing another per-operation
accounting catalog. Every potentially unbounded cycle, recursive call chain and
native traversal must encounter positive work accounting and cancellation polling.
Bound long acyclic regions too; a very large straight-line function must not bypass
the response bound simply because it has no backedge.

Define an implementation-specific maximum unchecked region or overshoot bound and
test it. Work arithmetic must not wrap into a fresh allowance. All nested entries
share the same root control state; reentry does not replenish it. Reaching the limit
terminates that execution and is not an ordinary script Result.

This deliberately changes observable termination semantics: with a tight limit,
different backends may complete different prefixes of otherwise valid work. Each
prefix must still obey evaluation order, atomic commit rules and cleanup. Ordinary
results, checked arithmetic, traps and side effects with sufficient resources remain
unchanged. Do not use relaxed charging to reorder business effects or swallow traps.

### Safepoints are not all budget checks

The current step path also participates in GC, debugger observation, source origins,
termination and runtime activation. Inventory those responsibilities before reducing
poll frequency. Removing exact charges must not remove required roots, allocation
safepoints, debugger stops or failure cleanup. Debug/trace modes may deliberately
use finer observation than the normal execution fast path.

Trusted Rust handlers cannot be forcibly preempted by a descriptor or deadline.
Long Native work must cooperate with the generic polling/work API; blocking IO must
remain a host concern or use the future async operation protocol. Input-size-dependent
operations, including reflection and allocation-heavy loops, cannot be treated as
constant work merely because they use a single Call instruction.

### Memory accounting boundary

Keep `max_heap_units` named honestly. Current [heap accounting](../crates/kagari-runtime/src/gc.rs)
counts object/element units, not full process bytes or all Rust allocations. Audit
strings, container spare capacity, roots, continuation buffers and native scratch
storage at EP00; document coverage and fix exploitable unbounded growth through
supported APIs without pretending units are an exact byte guarantee.

The removal of cumulative allocation quotas does not remove work checks around
repeated allocation and collection. GC releases live occupancy, not consumed work.
Reserve/check memory before committing mutations, retain checked size arithmetic
and allocation-failure handling, and keep host RPC payload/queue capacity under
host control. Process-level memory isolation remains outside the engine guarantee.

## Existing limit migration

| Existing setting | Replacement or destination |
| --- | --- |
| `max_instruction_steps` | `max_work`, with new coarse semantics; no exact numeric compatibility promise |
| `max_call_depth` | Runtime-level execution-stack limit |
| `max_heap_units` | Runtime-level live occupancy limit with explicit coverage |
| `max_allocation_units` | Remove independent per-execution quota; retain useful allocation metrics only where justified |
| `max_host_calls` | Remove universal call-count limit; service rate/concurrency limits belong to the host |
| `max_reflection_operations` | Remove independent quota; account meaningful work through the common guard |
| `max_modules` | Move admission capacity to host loader management, not per-execution policy |
| `max_dirty_records` | Move capacity to the host mutation-ledger owner, preserving reservation before write commit |
| `max_wall_time_ms` | Host deadline triggers cancellation; engine no longer maintains a second general wall-time budget |
| Native `resource_cost_hint` | Remove precise extra charges and their portable policy field; use bounded cooperative work where needed |

Keep structural decode limits, bounded metadata tables and allocation overflow
checks. They are not execution quotas. Moving module/dirty-record controls does
not make those stores unbounded or permit a field write before dirty-record capacity
has been secured. Do not retain removed policy fields as ignored compatibility knobs.

Counters used for tracing or measurements are not part of the execution contract.
Keep only useful statistics; assess optional collection where hot-path cost matters.
Do not implement nine always-updated counters underneath an apparently simpler API
without measuring and justifying that cost.

## Deadlines and async integration

The host owns RPC timeouts and task deadlines. An RPC timeout may be its declared
business Err; a task deadline requests sticky cancellation. The embedding adapter
must deliver cancellation even while the script is parked, and a CPU-bound script
must still poll cooperatively. Do not assume a timer on a starved Actor can interrupt
that Actor; combine bounded drive slices with appropriate host cancellation delivery.

The [host scope design](host-task-scope-design.md) uses a host-selected default work
allowance for each admitted job. There is no per-task capability intersection or
parent/child budget transfer tree. Same-execution calls/awaits share their guard;
launch creates a separate managed execution. An already terminated caller cannot
launch more work, and resumption never resets that execution's allowance.

A scope limits unfinished jobs and cancels them on close. This is not a rate limit:
an untrusted script may repeatedly create short jobs or issue expensive host requests.
If the deployment requires aggregate CPU/IO/start-rate protection, the host must
enforce it at admission/service boundaries. This plan deliberately does not claim
that a per-job work cap and unfinished-job count alone provide that guarantee.

Waiting consumes no script work. Slice exhaustion means reschedule; total work
exhaustion means terminate. Runtime heap usage remains runtime-wide instead of
being precisely attributed among interleaved async roots. Queues and completion
payloads stay bounded by their owners, not a generic engine resource ledger.

## Correctness boundaries that remain

- Validate types, call signatures, bytecode/MIR, native contracts, bounds and versions.
- Preserve readonly/writeable declarations and declared host adapters without
  adding manual field permission flags.
- Preserve host borrow/no-escape rules, GC roots, object/runtime ownership and stale
  generation detection; untrusted artifacts cannot forge retained identities.
- Pin program/provider generations across calls and future async waits.
- Preserve prepare-before-commit behavior and cleanup without business rollback.
  Never insert a cancellation exit inside an indivisible commit.
- Keep cancellation/exhaustion sticky through nested calls, and quarantine genuine
  engine invariant failures rather than converting them into business success.
- Retain candidate-initialization isolation and effect restrictions. Installed
  access does not authorize publishing a partially validated reload candidate or
  performing external writes during its restricted evaluation.

Native effect metadata remains useful for optimization, roots, callback execution
and candidate phases. Removing permission bits does not erase those facts or
make all providers' executable representations interchangeable.

## Ordered implementation phases

### EP00 Inventory and baseline

- [ ] Confirm NR05 acceptance; record the revision and re-audit policy/charge consumers.
- [ ] Review current safety specifications, tests, profiles and external SDK consumers.
- [ ] Define installation/tool options, runtime limits, root controls and host-owned
  capacities; decide defaults, supported protection bounds and heap-unit coverage.
- [ ] Measure current checking/charging overhead with and without configured limits,
  collecting the environment and workloads listed below.

Exit: every removed field and mixed step-helper responsibility has an owner; no
unresolved safety path is dismissed as merely a redundant permission check.

### EP01 Installation based access

- [ ] Replace duplicated capability/profile/exposure gates with installed-contract
  access across HIR, SDK, linking, runtime calls, paths and reflection.
- [ ] Move debugger/JIT choices to host tooling owners; update CLI profiles/examples.
- [ ] Remove portable permission fields and update artifacts/fingerprints with no
  old-reader or ignored-field compatibility path.
- [ ] Prove a host function works by installation alone while absent, mismatched,
  forged and invalid-lifetime calls still fail before implementation entry.

Exit: a normal host function needs one registration, not repeated permission flags;
declared writeability and candidate isolation still hold.

### EP02 Runtime protection and interpreter work

- [ ] Separate runtime heap/depth limits from root work/cancellation controls.
- [ ] Implement bounded coarse work/poll regions through verified MIR, bytecode and
  interpreter execution, with independent GC/debug/lifecycle safepoints.
- [ ] Preserve shared controls through ordinary calls, callbacks and host reentry.
- [ ] Move loader/dirty-buffer capacities and remove superseded per-operation quotas.
- [ ] Cover loop, recursion, allocation churn and long Native traversal termination.

Exit: interpreter protection is bounded without exact charging and cannot be bypassed
by untrusted control flow, malformed metadata or nested entry.

### EP03 Backends and obsolete contracts

- [ ] Update the supported JIT subset/helper ABI to the coarse guard; keep checked
  fallback for unsupported code and never restart after native effects.
- [ ] Remove obsolete LogicalBudgetCharge, charge-only BudgetCheckpoint uses,
  cost-hint policy and optimization constraints; review real safepoints separately.
- [ ] Reverify generated/optimized executable polling coverage and version all changed
  artifacts, helper links and cache identities.
- [ ] Replace exact cross-backend budget-position assertions with bounded protection
  tests while preserving ordinary result, trap, effect and native-entry coverage.

Exit: all supported execution paths obey the new protection contract; none relies
on a silent unmetered fallback or retains the old exact budget implementation.

### EP04 Embedding and specification integration

- [ ] Migrate SDK configurations, examples, tests and external consumers to the small
  installation/runtime/execution split; remove obsolete public entrypoints.
- [ ] Update security, execution, failure, host interop, JIT, artifact, embedding and
  architecture specifications with the implemented boundaries and semantic change.
- [ ] Document host deadlines, cancellation delivery, Native cooperation and memory
  coverage; sync the future async/scope designs without implementing them here.
- [ ] Remove stale permission/delegation and exact-accounting requirements from active
  documentation; preserve historical acceptance records as historical evidence.

Exit: public documentation and behavior agree, and no compatibility flags conceal
the simpler model behind the old policy API.

### EP05 Acceptance and measurements

- [ ] Complete the boundary matrix below and predecessor feature/dependency consumers.
- [ ] Compare interpreter and supported JIT workloads to EP00, including polling
  frequency, cancellation response, work overshoot and memory coverage.
- [ ] Resolve every carried build/test error and document performance tradeoffs.

Exit: all required checks pass and measured results support the selected checkpoint
granularity; no performance gain is claimed solely from reducing configuration fields.

## Acceptance and performance evidence

Required behavioral evidence includes registration-only access, rejection of absent
or forged bindings, readonly host fields without manual permission flags, bounded
loops/recursion/straight-line work, shared cancellation through callbacks, large
Native operations, allocation churn, memory reservation failure, and correct cleanup.
Test candidate isolation, pinned reload versions, debugger/GC safepoints and source,
encoded artifact and supported JIT paths. An untrusted artifact must not supply zero
work for a cycle or omit required polling coverage to escape termination.

Budget tests should assert termination class, bounded response and valid completed
effects, not identical step counts/source stop locations between backends. Keep
ordinary arithmetic error origins, result semantics and mutation guarantees tested.
Host capacity rejection must occur before module admission or dirty-write commit.

Measure tight loops, calls, host callbacks, collection/string traversal and allocation
workloads, plus the currently supported scalar JIT cases. Compare enabled/disabled
work limits and observer modes. Record toolchain, machine, workspace profile,
features, default Cargo parallelism, cache state and inputs. Separate build time
from execution; record repeated-run variation, native entry versus fallback, polling
counts and retained memory. Store transient logs under ignored `target/`; preserve
durable conclusions here. No numeric speed or overhead target is invented now.

Final implementation checks:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p kagari-cli --features jit
git diff --check
```

Follow completed NR/ST feature and dependency checks as well. This refactor changes
APIs/artifacts and resource-exhaustion semantics; implementation checkpoints use
Conventional Commits with breaking-change notes and `Policy-Step: EPxx` trailers.
Record attempted checks, representative intermediate errors and their owning phase
here. No production stubs, weakened safety validation or unrelated algorithm changes
are allowed to make a phase pass. No commit is authorized by this documentation task.

## Progress ledger

- 2026-09-30: Queued EP00-EP05 after the user confirmed one trust environment per
  runtime, registration as authorization and runaway protection rather than precise
  metering. Explicitly excluded manual field read/write permission configuration.
  The inventory is based on the current source and is to be refreshed after NR05.
  All phases remain unstarted; no runtime or performance result is claimed.
- Documentation validation: checked 114 local links, 12 heading anchors and
  whitespace across the six related documents; `git diff --check` passed.
  No Rust build or tests were run for this documentation-only change. No commit
  was created.
