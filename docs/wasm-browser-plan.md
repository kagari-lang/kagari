# Fixed-width integers and browser WASM execution plan

## Status and authority

Status: active; implementation and one commit per phase authorized on 2026-10-18.
WB01-WB04 are complete; WB05 is next. Pushes, remote settings changes, dependency
publication and deployment remain outside this authorization.

The [roadmap](implementation-roadmap.md#browser-wasm-execution) owns activation and
sequencing. This plan owns WB01-WB05, finite scope, design decisions, acceptance
and the sole progress ledger. [Project goals](project_goal.md),
[architecture](architecture.md) and [specifications](README.md#language-and-execution-specifications)
remain authoritative for behavior not explicitly changed here. Current specs
now describe fixed-width integers after WB01; this ledger records migration
acceptance separately from the remaining browser work. Follow [AGENTS.md](../AGENTS.md), including its early-development
and focused/local-final/CI verification policies.

## Outcome and finite scope

1. Remove script `isize` and `usize`, including builtin identities and literal
   suffixes. Use fixed-width integers throughout language contracts. Do not keep
   aliases, compatibility readers or a second numeric implementation.
2. Preserve host-machine `usize` for Rust allocation, lengths and physical indices.
   Separate it from script values through checked, operation-owned boundaries.
3. Compile the existing Rust frontend, standard library, bytecode VM and runtime
   into a browser WASM product. Source compilation and execution happen locally
   inside a Web Worker; users do not have to supply a `.kbc` file or use a server.
4. Retain a source-free browser product capable of loading validated `.kbc`
   products without bundling the frontend. It is a separate build configuration,
   not a second interpreter or a required public file-upload UI.
5. Integrate one bounded, lazily loaded Playground into the existing documentation
   site, with build provenance, cancellation, reset and actual browser acceptance.

Not included: a Kagari-to-WASM code-generation backend, i128/u128 script types,
BigInt arithmetic, a replacement GC, WASM threads/shared memory, JIT/AOT in the
browser, WASI or non-JS WASM host support claims, a new general embedding facade,
full IDE/LSP integration, network packages, arbitrary JS callbacks, multi-file
workspace UI, live-reload UI, translations or a remote execution service. Existing
async, reentry, roots and pinned reload contracts must not be weakened; exposing
all of them as browser APIs is not required by this delivery.

## Baseline and evidence

Inspected baseline: `79148917`, clean worktree at planning start.

- [SDK features](../crates/kagari/Cargo.toml) default to `source` and `native`.
  The source-enabled interpreter dependency graph already passes the command below.
  `native` is machine-code preparation, not the ordinary Rust standard-library
  callbacks that also compile into WASM.
- [Syntax](spec/syntax.md#integer-literals) defines script isize/usize as 64-bit;
  [scalar layouts](../crates/kagari-types/src/scalar.rs) agree. The language has no
  i128/u128 types; internal Rust i128 constant payloads remain valid implementation
  details and are not removed.
- [Sequence storage and typed views](../crates/kagari-runtime/src/native/sequence.rs)
  currently use Rust isize/usize for script elements. Parsing and aggregation in
  [construction bindings](../crates/kagari-stdlib/src/bindings/construction.rs),
  scalar sorting and logical iterator counts also need migration.
- [Cursor indexing](../crates/kagari-runtime/src/frame/cursor/indices.rs) and
  [reflection](../crates/kagari-runtime/src/reflection.rs) contain wide-to-usize
  casts. On wasm32, 4294967296 can truncate to zero before a bounds check.
- [Session creation](../crates/kagari-runtime/src/session.rs) and
  [GC collection](../crates/kagari-runtime/src/gc/collection.rs) call Instant
  unconditionally. Bare unknown-unknown compilation does not supply a working clock.
- [SDK owned execution](../crates/kagari/src/runtime/owned.rs) already exposes
  start/drive, Runnable/Waiting/Complete and cleanup. Reuse it instead of accessing
  VM internals through a new browser-only execution path.
- [Host source input](../crates/kagari-package/src/host.rs) and virtual sources
  already separate source acquisition from execution. The initial UI needs only
  one source file, not an emulated operating-system filesystem.
- The [site](../site/README.md) has 24 guide routes, locked Playwright, shared
  highlighting, fresh-output checks and a Pages workflow. Editor grammar WASM is
  unrelated to compiling or executing the language.

Previously run during this design discussion on Windows, Rust 1.99.0
(`b940084d7`, 2026-09-28), default workspace dev profile and parallelism:

```sh
cargo check -p kagari --lib --target wasm32-unknown-unknown --no-default-features --features source
```

Passed. This is type/target compilation evidence only: no final WASM link, JS
exports, browser execution, timing, panic recovery or 32-bit semantic acceptance
was established. Do not repeat it just to refresh a successful transcript.

## Semantic decisions

### Fixed-width language integers

Keep i8/i16/i32/i64 and u8/u16/u32/u64; floating-point types are unchanged.
Migrate standard lengths, capacities, indices and logical counts from usize to
u64. Inspect each former isize contract and migrate its signed numeric role to
i64. Unsuffixed integer inference still defaults to i32 after contextual checking;
this migration does not introduce blanket implicit conversions or require all
index expressions to be explicitly u64 if the existing indexing rule permits
other integer types. Preserve generic/user-defined index contracts.

Remove identities and suffix recognition through syntax, HIR, shared numeric
facts, contracts, MIR, bytecode, runtime registration, standard library, Rust
adapters/derive support, editor grammar and guide examples. Existing script code
and native declarations using the removed types must be updated. Audit concrete
uses rather than text-replacing all Rust `usize`/`isize` identifiers.

Consolidate ISize/USize storage into existing I64/U64 storage and algorithms;
retain the existing Value carriers where applicable. Do not add ScriptUsize or
ScriptIsize wrappers: they belonged to the rejected design retaining both types.
Remove duplicate numeric tables/tests while retaining distinct overflow, unsigned
range, checked conversion and source-free coverage.

A u64 value of 4294967296 is valid on both desktop and wasm32. An array containing
that one value needs one 64-bit element, not 4294967296 elements. Large logical
`take`/`skip` counts remain u64 and must not fail merely because host usize is
narrower. Arithmetic remains checked according to existing language rules.

### Machine indexing, allocation and host conversions

Keep Rust usize for actual storage positions, lengths, capacities and allocation.
No WASM-only correctness branch: 32-bit native targets have the same issue. Use
checked conversions or explicit proofs shared by all targets. Only introduce a
pointer-width specialization after measured evidence, never bypass validation.

For an existing buffer, comparing the script index against its length in the
wide domain can prove both bounds and representability. Convert only after that
proof. Preserve negative-index handling and base/index/value evaluation order,
including completed effects before failure. Subsequent safe accesses may retain
bounds checks unless optimization removes them; do not introduce unsafe access
merely to claim zero conversion overhead.

Conversion mechanics may be shared; error policy stays with the operation:
indexing traps, optional lookup returns absence where specified, string slicing
retains UTF-8 boundary rules, and allocation checks length/byte-size/resource
constraints. Do not turn a valid large script integer into ModuleValidation just
because it is not a machine index. Error payloads must not truncate the offending
wide value; review index diagnostics if their fields currently require usize.

At the Rust host boundary:

- Portable numeric APIs and standard-library declarations use u64/i64.
- Remove zero-copy NativeElement views that treat script-wide buffers as
  &[usize]/&[isize]. Use &[u64]/&[i64] and update direct consumers coherently.
- Retain ordinary Rust usize/isize scalar convenience conversion only as an
  explicitly checked mapping to script u64/i64, with documented host range limits.
  Such adapters add no new script identity and cannot authorize reinterpreting
  slices. Audit trait registration, inference, ABI descriptors and derive consumers
  so the mapping is consistent, not merely a renamed signature.
- If such a host argument is unrepresentable, fail before callback entry. Hosts
  requiring the full script domain accept fixed-width parameters instead.

On 64-bit hosts, same-range conversion checks can generally optimize away. On
wasm32, range proof is necessary and may combine with an existing bounds check.
No speedup or zero-overhead claim is accepted without generated-code/timing evidence.

### Cross-platform agreement and explicit limits

For equal source, equivalent installed interfaces and inputs, and operations
within platform resources, preserve specified return values, integer errors,
left-to-right once-only evaluation, mutation commits, roots and pinned generations.
Do not promise equal resource limits, real-time cancellation points, HashMap/Set
iteration order, floating-point NaN payloads or unspecified behavior.

Container capacity depends on address width, element size, allocator/runtime
limits and available memory. Not every host-size value can be allocated. Reject
unsupported sizes without wraparound; ordinary checked failures follow their
existing contracts. Underlying OOM may be fatal and is not promised recoverable.

Products encode language contracts, not Rust slice layouts or live pointers.
Update codecs, validation, signatures, fingerprints and direct consumers together.
Development artifacts/caches are disposable: regenerate affected fixtures at
coherent checkpoints and retain meaningful source-free tests. Keep ABI/format
identifiers but do not mechanically bump them for this unpublished migration.
Do not promise old-artifact compatibility or weaken validation to load old fixtures.

## Platform and browser ownership

### Target, features and package layout

Proposed new package: `crates/kagari-web`, producing a cdylib and exposing browser
bindings through wasm-bindgen. This path/package and the web feature do not exist
at planning time. Create them only with real behavior, not empty future facades.

- The Rust target selects instruction set and pointer width.
- Existing SDK `source` selects the frontend independently of platform support.
- A new additive SDK `web` feature forwards the narrow runtime web platform support;
  it does not imply source, native preparation, AOT, or changed language semantics.
- kagari-web depends on kagari with defaults disabled and web enabled. Its default
  source feature forwards kagari/source. A no-default-features browser build loads
  checked artifacts without linking the frontend; its JS surface excludes source
  compilation instead of silently failing through a fake implementation.
- Use target-specific dependency placement and localized cfg for browser clock/JS
  glue. wasm32-unknown-unknown does not itself imply a JS host. Reject unsupported
  browser capability combinations clearly rather than leaving a known Instant
  panic on the first execution. Keep desktop defaults intact.
- The initial supported target is browser/Worker wasm32-unknown-unknown. Audit and
  explicitly guard unsupported AOT/dynamic-loader combinations. Do not silently
  turn a requested backend into a no-op or conflate all generic native preparation
  APIs with an actual available machine-code backend.
- Ensure workspace default all-target Clippy still works on the development host;
  platform-specific tests/exports must be correctly scoped, not exempted globally.

### Timing and deterministic inputs

Use a small runtime timing owner for session/GC measurements: native Instant on
native targets, web-time Instant with web support on the browser target. Do not
invent a general service framework or handwritten JS clock implementation for two
measurement paths. Browser timestamps must stay in their originating Worker;
this delivery does not transfer active runtimes between browser workers.

web-time 1.1.0 was inspected as a mature candidate; freeze its compatible resolved
version and the wasm-bindgen crate/CLI versions during WB02/WB03. It uses
Performance.now(), needs browser Performance availability, and differs across
platforms in sleep behavior and precision. Document those measurement limits.
Do not report zero as a fabricated successful measurement when no clock exists.

Script-visible time and random inputs remain host-selected DeterministicInputs.
Never substitute Date.now()/Math.random() into deterministic script operations.
No per-instruction clock query is required by this design.

### Browser interface and ownership

Keep JS/DOM concerns in kagari-web and the site Worker/UI, not HIR or generic VM
loops. Reuse compile-to-artifact, PreparedProgram validation, load and SDK owned
execution. Do not add an alternate evaluator or bypass source-free validation.

The first interface accepts one named source file, returns structured diagnostics,
starts/drives/cancels execution, and returns bounded output and simple owned
results. Use the existing verified root-main capability where appropriate; do not
make private exports public to obtain an entrypoint. Source compilation produces
in-memory artifacts; no filesystem or server is required.

Freeze the finite message/result schema in WB03. Preserve diagnostics' origin,
code, severity and ranges; translate UTF-8 byte spans to the editor's UTF-16
positions with Unicode/CRLF coverage. Transfer 64-bit integers losslessly using
BigInt or an explicit tagged string representation, never JS Number by default.
No raw heap addresses or unbounded object graph serialization; returned script
objects remain rooted until conversion completes, then roots are released.

Use explicit installed output functions, not Rust println as a browser transport.
Bound source bytes, pending messages, output bytes/messages and displayed results;
define visible truncation/rejection instead of unbounded allocation or silent
success. Retain lower-layer roots/borrows/Send constraints. No arbitrary JS callback
registration or callbacks while runtime borrows are active. Expose neither network
nor filesystem nor DOM mutation to script in the initial environment.

### Worker scheduling, cancellation and fatal failure

One Worker owns one WASM instance and runtime, with one active run request.
Use SDK start/drive slices; Runnable schedules a later browser task, Waiting uses
an owned wake mechanism, and Complete converts results and releases the owner.
A wake is a scheduling hint, not synchronous recursive entry. Yield through actual
event-loop tasks; a self-perpetuating resolved-Promise chain can starve stop events.

Do not redefine language async semantics or introduce execution charging. Slice
size is a host scheduling parameter, not a generic permission/resource matrix.
Long native callbacks and synchronous compilation are not made preemptible by VM
slicing. Normal cancellation completes through the driver and preserves existing
cleanup and completed side effects. If native/compile work does not yield, the UI
must still offer hard reset by terminating the Worker.

Use request/run generations so replies and queued wakes from a retired Worker/run
cannot update a new run. Reset releases normal owners when possible; hard reset
terminates the whole instance and starts a fresh one. Limit queued work rather
than starting concurrent access to one heap.

Default unknown-unknown uses panic=abort. Existing catch_unwind paths cannot be
claimed to recover host panics there. Language diagnostics, checked traps and
cooperative cancellation remain ordinary results; Rust panic, fatal WASM faults
and unrecoverable allocation failure invalidate the instance. Do not reuse it.
Hard Worker termination offers no destructors, rollback or cleanup callbacks.
Restrict initial host capabilities so killing an instance does not strand external
resources. Native panic handling remains unchanged. Browser unwind support would
require a separately tested toolchain contract, not a casual feature toggle.

This is not a hardened arbitrary-code hosting service. Worker isolation and
bounded UI traffic do not establish hard memory quotas or a general security
sandbox. Keep remote imports and arbitrary host capabilities out of scope.

## Mature references and adaptations

- [Rust numeric types](https://doc.rust-lang.org/reference/types/numeric.html):
  Rust usize/isize follow pointer width. Reuse that distinction inside Rust, not
  as duplicate fixed-width script types. Inspected compiler baseline: Rust 1.99.0.
- [Cargo feature unification](https://doc.rust-lang.org/cargo/reference/features.html#feature-unification):
  additive capabilities and target-specific dependencies, not mutually exclusive
  desktop/WASM semantic implementations. Existing workspace resolver is 3.
- [Rust wasm32-unknown-unknown support](https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html):
  std compilation does not supply working filesystem, threads or clocks; default
  panic strategy differs from native unwind builds. Real browser tests are required.
- [web-time 1.1.0](https://docs.rs/web-time/1.1.0/web_time/): maintained browser clock
  adaptation, Performance requirements and sleep/precision limits; measurement
  support must not change Kagari deterministic inputs.
- [wasm-bindgen integration](https://wasm-bindgen.github.io/wasm-bindgen/examples/hello-world.html):
  use its maintained Rust/JS ABI and cdylib tooling rather than a custom export ABI.
  Select and lock compatible crate/CLI versions before producing products.
- [Worker termination](https://developer.mozilla.org/en-US/docs/Web/API/Worker/terminate):
  immediate termination is not graceful cancellation. Reuse Kagari's existing
  owned-execution driver for graceful behavior; destruction is a separate boundary.
- [WASM numeric types](https://webassembly.github.io/spec/core/syntax/types.html):
  i64 operations are available even with 32-bit addresses. No new WASM codegen
  backend or script i128 type is required for this portability work.

These references justify mechanisms, not performance claims. Runtime roots,
checked native contracts and generation ownership remain Kagari-specific invariants.

## Phase order and acceptance

### WB01 — Replace script machine-named integers coherently

Inventory direct consumers before editing. Replace script isize/usize with fixed
width contracts across declarations, syntax/HIR, lowering, ABI/codecs, runtime,
stdlib, Rust conversion/derive, examples, grammar and teaching material. Consolidate
storage and typed views rather than retaining wrappers or duplicate implementations.
Update current spec owners and invalidate/regenerate affected development fixtures.

This is one coherent cross-crate migration, not many compatibility checkpoints.
Intermediate local builds may fail while consumers are being replaced; record the
command, representative diagnostic and same-phase owner here. Before phase
acceptance, restore native compilation and selected numeric, signature, host-view
and source-free contracts. No production stubs, disabled validation or weakened
assertions. WB02 owns remaining platform-width indexing/clock corrections, not
unresolved WB01 integration errors.

### WB02 — Make memory boundaries and measurement portable

Fix wide-to-machine conversions in indexing, reflection, collections, strings,
allocation and host argument handling. Logical iteration/counting remains wide.
Preserve error categories, once-only effects and mutation ordering. Introduce the
localized web timing capability and its target/dependency guards. Keep clock reads
out of generic instruction loops. Review allocation arithmetic and diagnostic
payloads for truncation, not just the most visible casts.

Acceptance: affected native cases pass; target compilation passes with the new
supported feature combination; boundary tests cover 32-bit representability without
large real allocations. Actual browser behavior remains pending WB03, explicitly
not inferred from native simulated-limit tests or cargo check.

### WB03 — Build and execute the minimal browser product

Add kagari-web with real wasm-bindgen exports and the source/default versus
source-free feature split. Freeze message schema, dependency/tool versions and
bounded build/test commands. Add a small real-browser harness and run compilation,
diagnostics, normal execution, fixed-width integers, invalid indices, GC/root
cleanup and artifact validation. Generate one current artifact on the native host
and execute it with the source-free browser build; compare meaningful behavior and
reject invalid products through the existing validator.

Acceptance requires actual linked/loaded WASM, not merely cargo check. Confirm the
source-free production graph excludes frontend dependencies, native/AOT tools are
not bundled, and JS values are converted without precision loss. Keep temporary
harness products under ignored target; use the same adapter in WB04/WB05.

### WB04 — Own Worker execution and lifecycle

Implement task-based slicing, message generations, normal cancellation, Waiting
wake scheduling, bounded output, hard stop and fresh-instance recovery. First UI
entrypoints may be exercised through the bounded harness before site integration.

Acceptance: infinite script loops stop cooperatively; cancellation releases roots
and loans; effect order is preserved. Force termination during non-yielding work
or a controlled fatal test destroys the old instance and permits a clean next run.
Stale responses do not affect the new run. Compilation cannot freeze the main UI.
Reuse reentry/pinned-generation boundary coverage without adding a live-reload UI
or JS callback system. Unsupported browser host capabilities remain explicit.

### WB05 — Integrate, measure and hand off

Add one functional Playground route to the static site with a minimal accessible
editor/input and output/diagnostic UI. No full IDE framework is required. Lazy-load
Worker, JS glue and WASM only on demand; ordinary guide pages must not fetch the
compiler. Reuse maintained syntax/example owners and use text-safe output rendering.
Update route inventory, exact page-count expectations and workflow tests together
(the planned one-route addition makes 25 guides/tool pages plus the generated 404).

Extend the build/output contract for WASM: record relevant Rust sources, Cargo.lock,
toolchain/target/features, binding-tool versions and JS inputs; reject stale or
missing browser products. The ordinary site build consumes an explicit, validated
product, never opportunistically invokes Cargo from content rendering. Define one
bounded orchestration path before changing CI. It builds the WASM product and site,
checks the exact output and uploads it without rebuilding afterward. Keep PR
permissions unprivileged and deployment guarded; no new remote authorization follows
from this plan. Re-review the site's dependency boundary for the new dynamic UI.

Validate /kagari/ Worker and WASM URLs, asset serving, Unicode errors, repeated runs,
keyboard/mobile use and no unexpected network requests. Measure raw/compressed WASM
size, initialization, source compilation and execution separately. Record browser,
machine, Rust/Node versions, profile, features, parallelism, cache state and workload;
set any performance gate before tuning. Make no unmeasured speed claim or local
full-matrix claim. Live site acceptance is separate and requires authorization.

Move lasting contracts to architecture/specs, the browser package README and site
maintenance guide; move unresolved acceptance to the roadmap. Remove this completed
plan. Git history owns phase ledgers and successful command transcripts.

## Verification and intermediate build policy

Reuse tests at their semantic owners; add cases only for distinct uncovered
contracts. Key existing owners include
[numeric operations](../crates/kagari/src/runtime/tests/language/numeric_operations.rs),
[host conformance](../crates/kagari/src/runtime/tests/embedding/conformance.rs),
[error traces and cancellation](../crates/kagari/src/runtime/tests/embedding/error_traces.rs),
[artifact feature boundaries](../crates/kagari/tests/artifact_features.rs),
[fixture ownership](../crates/kagari/tests/fixtures/README.md),
and the existing site output/browser checks.

Required focused coverage:

- Removed names/suffixes and migrated standard signatures, without changing other
  numeric inference rules. Full unsigned range, signed boundaries and overflow.
- One-element collections containing values above u32::MAX, parsing/formatting,
  sorting/reduction and large logical take/skip counts. No giant allocations.
- Out-of-range index reads/writes and string byte ranges, with no wraparound or
  unintended mutation and with preserved evaluation/effect order.
- Checked host narrowing before callback entry; correct fixed-width scalar views.
- Native-produced/source-free WASM artifact execution, malformed input validation,
  roots/GC, traps/cancellation and a small directly affected pinned/reentry set.
- Real browser Unicode diagnostics, lossless results, UI/output limits, event-loop
  progress, normal stop, hard reset, fatal recovery and old-message rejection.

Discover exact test target/filter names before running them and record the finite
selection here when implementation starts. Run a small changed-contract set plus
representative direct-dependent regressions, not a full workspace sweep split into
package commands. Use --no-fail-fast for multiple selected targets. Compilation
failure does not count skipped tests as passed. Preserve unaffected passing evidence;
rerun only failing/affected cases after repair.

For Rust implementation checkpoints, required local checks are:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Manually review production imports, re-exports, ownership, functions and effective
LOC, not only checker output. No blanket exceptions. Run selected target builds
using workspace profiles, default target storage and default parallelism. Browser
commands must own/close their server and Worker/browser processes with bounded
runtime; put reports and screenshots under ignored target. Use uv run python for
Python tooling.

Complete workspace tests, all backend/feature/platform combinations and exhaustive
editor/browser matrices remain CI-only. Local native plus selected real browser
coverage does not satisfy unrun CI. No routine Clippy/structure/Cargo/npm build is
needed for this planning-only change; check document links/content and whitespace.

## Progress ledger

- [x] Design selected and baseline inspected; plan recorded.
- [x] WB01: fixed-width language integer migration.
- [x] WB02: checked machine boundaries and browser timing.
- [x] WB03: linked source-enabled and source-free browser products.
- [x] WB04: Worker scheduling, cancellation and recovery.
- [ ] WB05: Playground integration, focused acceptance and handoff.

### WB01 — fixed-width integer migration

Removed script machine-named identities/suffixes across type facts, syntax/HIR,
contracts, bytecode/lowering, runtime/stdlib, SDK tests, examples and editor grammar.
Consolidated sequence storage/views and numeric tables into i64/u64; retained
checked Rust usize/isize scalar conveniences. Logical take/skip/count/enumerate
state now uses u64. Native signature forgery tests still exercise identical
physical representations with incompatible semantic contracts. Current specs and
teaching examples migrated; ABI identifiers unchanged, no aliases or compatibility
readers. Generated Tree-sitter products and ignored source-free fixtures updated.

Focused validation passed: frontend literal/name/inference/constant cases (including
checks_standard_numeric_surface); 12 contract/compiler cases; 19 runtime/VM/Serde
cases; 31 SDK/artifact executions spanning arithmetic, strings, aggregation,
provider rooting, reload and source-free/native preparation; two stdlib binding
boundary cases; selected grammar/TextMate/Zed capture cases and generated consistency.
Site build/check passed for 24 pages, 20 includes and 22 script tests. Workspace
Clippy, structure (zero violations), formatting and diff checks passed. No full
workspace/backend/editor matrix ran; that remains CI work.

Early parallel compilation waits timed out without completing some selected tests;
only the unfinished cases were rerun. A too-strict HIR filter selected zero tests,
then the corrected filter ran the intended test successfully. The stdlib test link
reported LNK2019 anonymous LLVM symbols after an interrupted incremental build;
package-only cargo clean followed by the same focused tests passed. No carried
WB01 integration failures. WB02 still owns wide index error handling and clocks;
actual browser execution, source-free WASM and performance acceptance remain unrun.

### WB02 — machine boundaries and clocks

Wide indices now reach checked storage owners without truncation; cursor/VM errors
retain full u64 values. Optional lookups preserve absence, writes preserve bounds
errors and effects, string offsets preserve UTF-8 checks, repeat allocation checks
machine length and byte capacity, and host scalar narrowing fails before callback
entry. Reflection and iterator buffer positions no longer truncate. Shared heap
access accepts checked-convertible host or script positions without duplicate
semantic implementations.

Added SDK/runtime web capability and private clock owner. Native targets use std
without JS dependencies; browser clocks use exact web-time 1.1.0. Lock resolution
added wasm-bindgen 0.2.129/js-sys 0.3.106 without upgrading existing packages.
Unsupported unknown-unknown without web and WASM aot-load/aot-build produce explicit
compile errors; AOT modules/dependency are target-gated. Script deterministic inputs
are unchanged.

Passed nine selected runtime/VM boundary tests, two stdlib collection/string tests
and two fixed-width/logical-count regressions; target source,web check; workspace
Clippy, structure (zero violations), formatting and diff checks. Negative capability
checks failed with the intended diagnostics, not unresolved loader symbols. Earlier
concurrent signature-transition errors were resolved before acceptance. Native
checks exercise high values but do not substitute for wasm32 execution; WB03 owns
actual source/source-free browser validation. No full workspace suite ran.

### WB03 — linked browser products and real execution

Added kagari-web, bounded JSON protocol v1, explicit web::print, rooted result
conversion, UTF-8/UTF-16 diagnostic locations and independent default-source versus
artifact-only builds. Exact wasm-bindgen crate/CLI 0.2.129, web-time 1.1.0; builds
use existing dev profile/default target. Product scripts fingerprint Rust inputs,
lockfiles, toolchain/environment, recipe and output bytes; stale products fail
verification. Source-free dependency/export guards exclude frontend/native tooling.
No Worker scheduling or public route is claimed before WB04/WB05.

Actual browser construction uncovered SourceDatabase::default's cwd dependency,
which cargo check could not detect. Fixed the ownership boundary: source-enabled
engines accept an explicit lexical root, capture cwd only when none is supplied,
and retain that root in sessions and generated declaration capture. Browser adapter
supplies its own root; baseline analysis needs only an empty SourceSnapshot. No
fake cwd, filesystem emulation or browser-specific typechecker. Four SDK session
and six HIR native-declaration cases passed, plus adapter Unicode diagnostics.

Native adapter tests passed in source and source-free configurations. Real linked
products ran on Windows Chrome 155.0.8059.82: all nine source cases passed (full u64,
collections/logical counts, wide index effects/traps, GC/rooted results, removed
names, UTF-16/CRLF diagnostics, cancellation/reset and bounds). Source-free product
executed a current native-generated artifact with exact output/u64 max result;
malformed/truncated/hash-corrupted artifacts and admission bounds were rejected.
Products/source hashes stayed unchanged and harness processes shut down. A fixture
used an invalid bare-return match arm and guessed a specialized SDK index code;
corrected the fixture and asserted the existing ScriptTrap mapping with exact wide
index diagnostics and once-only effects. No core semantic weakening or skipped case.
Workspace Clippy, structure, formatting and diff checks passed. Full CI remains unrun.

### WB04 — bounded Worker lifecycle

Added a lazy client and task-driven module Worker with one active request, ordered
response credit, cumulative output bounds and worker/run generations. Waiting
parks rather than polls; cancellation is its currently supported wake. Normal
terminal execution resets/frees the session before reply. Hard stop/fatal recovery
destroys the old Worker, without promising cleanup or rollback.

`node site/scripts/wasm-worker.mjs` passed eight Node contracts and ten actual
Chrome lifecycle cases: cancellation/reuse, effects, bounded output/backpressure,
admission, stale replies, Waiting, hard stop and fresh-instance recovery. Waiting,
non-yielding compile and fatal cases use labelled test-only adapter wrappers;
there are no production fault hooks or panic-unwind/browser-counter claims.
Three selected native regressions cover adapter cancellation/root cleanup,
owned-future reentry and pinned-generation lifecycle. JS syntax and whitespace
checks passed. Rust is unchanged from WB03; its passing acceptance remains evidence.
WB05 owns UI/exact-site-product integration; exhaustive CI remains separate.
