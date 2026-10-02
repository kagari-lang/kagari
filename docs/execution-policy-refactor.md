# Installation Access and Cooperative Cancellation

Status: active. On 2026-10-02 the user authorized documentation and implementation
before further standard-library work. This replaces the previous coarse-work
proposal: no exact charging, `max_work`, general capability matrix or heap quota
framework remains in scope. The completed native reset is the starting point;
ST06/NR05 are historical and impose no new prerequisites.

## Approved behavior

Kagari primarily embeds trusted scripts. A runtime exposes the native functions,
types and objects installed by its host. Installation is authorization; no second
per-execution function/module allowlist or filesystem/network permission bits are
required. Offline declarations alone never install executable handlers.

Member visibility, val/var, readonly views and host getters/setters are language
and interface contracts, not configurable permissions. Keep their checks, valid
host paths, scoped borrows, object identity/lifetime and generation validation.
Keep native effects used by execution, optimization and candidate initialization.
Reflection must respect declared members and adapters. Debugger and JIT selection
belong to host tooling/backend configuration, not script privilege matrices.

Remove LanguageProfile, SecurityContext, CapabilitySet and HostExposurePolicy.
Remove precise instruction charges, cost hints, charge-only IR operations and
cross-backend budget-position requirements. Do not replace them with coarse work
accounting. Remove instruction, allocation, host-call, reflection, wall-time and
heap quotas. Host services own admission, queue capacity, rate limiting and IO
timeouts; structural decoding limits and checked allocation arithmetic remain.

Keep a runtime call-depth limit and optional root cancellation. Calls, callbacks
and synchronous reentry inherit the root cancellation state. Cancellation is
sticky; a native handler cannot swallow it and resume script effects. Host timers
may request cancellation. The engine does not provide hard deadlines, forced Rust
preemption, process isolation or per-script CPU/memory guarantees.

Interpreter execution and supported JIT paths poll cooperatively; long native
traversals poll in bounded chunks. Retain GC/debug/lifecycle safepoints independently
of removed charging. Initially retain existing appropriate polling coverage and
measure representative paths before changing its frequency. No accounting-only
operations may survive as empty execution instructions. Cancellation exits must
not split an indivisible mutation commit. Preserve completed side effects and
cleanup roots/borrows/temporary resources on failure.

Optional tracing/profiling owns observation. Remove always-updated policy counters
with no remaining consumer; GC occupancy needed for collection is independent of
an execution quota. Keep bytecode/MIR/native verification, overflow/bounds checks,
reload pinning, candidate isolation and invariant-failure quarantine.

## Finite execution phases

### EP01 Installation access

- [x] Remove duplicated authorization models and portable permission/cost fields.
- [x] Migrate production HIR, SDK, native/host calls, reflection and debugger;
  remaining legacy test expectations belong to EP03.
- [x] Prove installation-only access and retained binding identity/signature checks;
  the full readonly/lifetime matrix is carried to EP03.

### EP02 Cancellation and execution state

- [ ] Replace resource policy with runtime call-depth limits and root cancellation.
- [ ] Delete quota-only counters/paths; preserve real allocation and lifecycle checks.
- [ ] Remove logical charges and charge-only operations across MIR, bytecode,
  optimization, interpreter and JIT; preserve cooperative polling and observation.
- [ ] Cover cancellation through loops, callbacks/reentry, native work and cleanup.

### EP03 Integration

- [ ] Update active specs, SDK examples, feature consumers and future async designs.
- [ ] Replace obsolete permission/quota tests with meaningful retained-behavior tests.
- [ ] Run final workspace, JIT, source-free and dependency checks; record measurements
  without inventing speed claims or a new performance framework.

One coherent commit per completed phase, with breaking API notes and
`Policy-Step: EPxx`. Intermediate compilation failures are allowed and must be
recorded here; do not add aliases, ignored settings or stubs to satisfy old callers.
No routine format version bump or old-format reader. ABI/common renaming, new
libraries, async scheduling and general sandboxing remain separate work.

## Validation

Focused checks during implementation; final checks:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p kagari-cli --features jit
git diff --check
```

Exercise installed/absent/mismatched bindings, readonly adapters and stale handles;
loop/recursive/callback/native cancellation; call-depth protection; GC/debug
safepoints; candidate isolation and generation pinning; source-free execution and
supported JIT paths. Removed quota behavior is deliberately not preserved.
Measure existing representative interpreter/JIT workloads where available, recording
machine/toolchain/profile/features/workload and separating build from execution.

## Progress ledger

- 2026-10-02: Activated the simplified scope at revision 52897a4a. Inspected current
  permission gates and session/resource ownership. The former 2026-09-30 proposal
  was documentation only and its max_work/heap-accounting design is superseded.
  Implementation and integration checks are pending.

- EP01 checkpoint: removed permission models, portable permission/cost fields,
  HIR profile checking, SDK CompileOptions (its sole field was a profile), CLI
  profiles and artifact security-profile compatibility gates. Candidate effects,
  native binding validation and declared member rules remain. Runtime host access
  and offline binding tests: 9 passed. `cargo check --workspace` passes with
  cleanup warnings. Structure check: 610 files, zero violations before the new
  focused test; rerun at commit below. `cargo check --workspace --all-targets`
  still reports old test-only capability fields and debugger helper references
  (runtime_substrate, host_nominal, VM debugger tests). EP03 owns their migration
  and the old permission-denial assertions. No production compatibility shim.
