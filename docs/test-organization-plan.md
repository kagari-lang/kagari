# Repository-wide test organization and fixture migration

## Status, scope and authority

Planning baseline: `85662b70` (`test: Adopt rstest and contract-owned fixtures`),
branch `refactor/test-fixtures`, 2026-10-24. The pilot is committed. The user has
requested a repository-wide execution plan, including organization and duplicate
cleanup. The user subsequently authorized phased implementation and one coherent
commit per phase. TO00 is the initial classification/schema checkpoint; subsequent
phases retain their separate implementation and validation status below.

[Project goals](project_goal.md), [architecture](architecture.md#compilation-pipeline)
and [AGENTS.md](../AGENTS.md#verification-and-tooling) retain authority over language
contracts, ownership and verification. [Testing guidance](testing.md) owns lasting
placement, framework, fixture and execution rules. The
[roadmap](implementation-roadmap.md#test-execution-cost) owns activation; this plan
owns the finite phases, migration decisions and progress ledger for this task.

The older [test preparation plan](test-preparation-plan.md) retains deferred matched
preparation/compile/link and retained-memory measurements. It is not reactivated
or superseded here. The [script testing proposal](script-testing-design.md) remains
separate: this work does not implement `kg test` or script test attributes.

## What repository-wide completion means

The objective is a maintainable test organization, not merely moving strings into
files, adopting one framework everywhere, or lowering the reported test count.

At completion:

1. Every existing test family in the owner map has a semantic owner, execution
   boundary and reviewed disposition: migrate, consolidate, reorganize, or retain
   with a concrete reason. There are no unclassified residual families.
2. Approved same-flow Rust cases use named `rstest` cases or homogeneous file
   discovery; Node/Python/editor runners retain their appropriate native formats.
3. Mixed responsibilities and demonstrably redundant preparation are resolved at
   their owners. Proven duplicate coverage has one assertion owner; distinct
   boundaries remain covered even when their source inputs look similar.
4. Long fixed source/expected text is externalized where useful. Short byte-sensitive
   inputs, structured records and stateful scenarios may intentionally remain inline.
5. Case mappings account for changed names, splits, deletions, routes and assertions.
   No independent scenarios disappear inside a serial aggregate to reduce counts.
6. Required local checks pass; complete CI acceptance is recorded separately. No
   implementation regression, stale filter, dangling fixture or migration bridge
   remains. Pending CI is not described as passed.

A retained family is a valid outcome after review, not a way to leave a known
approved migration unfinished. New cases added during implementation receive the
same classification, but do not silently create new features or extra phases.

## Baseline inventory and concrete findings

Offline Cargo metadata reports **24 workspace members and eight integration
targets**. The [current target table](testing.md#default-placement-and-integration-exceptions)
explains the external-consumer, process, derive and allocator boundaries. Keep that
inventory; no new integration binary, test-support crate or custom harness is
planned. Initial inspection is an owner/family survey, not an exhaustive case
count or a fresh behavioral acceptance run.

### Owner coverage map

Paths in this table are relative to `crates/` unless otherwise indicated. Each row
must receive a disposition during its phase, including families not named in the
concrete examples below.

| Owner | Existing responsibility / expected treatment | Phase |
| --- | --- | --- |
| `kagari` language tests | Ordinary execution, diagnostics, linking and special admission cases; separate these before parameterization | TO01-TO02 |
| `kagari-source`, `kagari-syntax` | Literal/token bytes, AST/recovery, syntax diagnostics and grammar accounting; files/tables where suitable | TO03 |
| `kagari-hir`, `kagari-package` | Static semantics, surviving facts, queries/import identity, package graphs and filesystem acquisition | TO04 |
| `kagari-compiler`, `kagari-mir`, `kagari-bytecode`, `kagari-contract` | Lowering/effects/origins, typed products, source-free admission, proofs, codecs and disassembly | TO05 |
| `kagari` engine/program/embedding/AOT and external consumers | Session, installation, host, async, reload and code/resource lifetimes; retain explicit orchestration | TO06 |
| `kagari-runtime`, `kagari-vm`, `kagari-stdlib` | Substrate ownership, host/native boundaries, VM execution and installed-library composition | TO07 |
| `kagari-common`, `kagari-types` | Scoped identities, cancellation, wire bounds and pure numeric/reference contracts; primarily native Rust tables/scenarios | TO08 |
| `kagari-abi`, `kagari-codegen`, `kagari-derive` | ABI/backend/proc-macro contracts through consumers and compile-fail/API tests; do not invent suites merely to migrate them | TO08 |
| `kagari-codegen-cranelift` | Native product admission, execution, failure recovery, retention and thread transfer | TO08 |
| `kagari-format`, `kagari-lint`, `kagari-cli`, `kagari-analyzer`, `kagari-web` | Source tools, actual binaries/protocols and portable browser adapters; goldens/tables plus distinct stateful boundaries | TO09 |
| `kagari-lua-benchmark` and benchmark scripts | Measured workloads, independent references and report validation; preserve measurement contracts | TO10 |
| `editors/`, `site/`, `scripts/tests/`, feature/AOT scripts and workflows | Tree-sitter/adapter/Node/Python/browser/product/deployment consumers; keep native runners | TO10 |

### Evidence that determines the work

- **Confirmed repeated execution setup:**
  [`conversion_traits.rs`](../crates/kagari/src/runtime/tests/language/conversion_traits.rs)
  has 15 ordinary `execute(...)` calls, alongside diagnostics and a multimodule
  scenario. Its helper and the default/standard-trait helpers select forced GC,
  source/artifact/artifact-JIT routes and result `42`. The common execution engine
  already exists in `test_support/execution.rs`; do not write another backend loop.
- **Confirmed duplicate rejection coverage:** parser
  [`conformance.rs`](../crates/kagari-syntax/src/tests/parser/conformance.rs)
  and [`language_conformance.rs`](../crates/kagari-syntax/src/tests/parser/language_conformance.rs)
  both reject `fn apply(effect: dyn Effect) {}` by checking for an error-severity
  diagnostic. Consolidate that assertion at the spec-linked owner. For their shared
  invalid-field example, preserve the stronger diagnostic kind/count/span checks;
  equal source alone would not justify deleting them.
- **Confirmed mixed ownership:**
  [`lexer/tokens.rs`](../crates/kagari-syntax/src/tests/lexer/tokens.rs) contains
  `byte_literal_parser_contexts_distinguish_scalars_from_arrays`, which exercises
  parser contexts. HIR `typeck/diagnostics.rs` mixes rejection with retained-neighbor
  facts; compiler `tests/verify.rs` mixes rejection with bytecode layout/emission.
  Repair those boundaries rather than merely relocating their source strings.
- **Confirmed helper overlap:** package input/workspace/graph/analysis tests each
  wrap the same virtual-source conversion; compiler source-program tests duplicate
  adoption preparation already in `tests/common.rs`; VM `host_runtime` and
  `reflection_runtime` currently select the same standard runtime. Review and remove
  redundant layers at the existing support owner, preserving identities and settings.
- **Useful organization already exists:** SDK embedding/async helpers, runtime/VM
  source-free constructors, compiler test support, typed integer tables, the shared
  numeric predicate generator, editor corpus/oracles, Node tests and Python
  `unittest` helpers. Reuse them; this is not a blank-slate framework project.
- **Preservation examples, not duplicates:** ordinary source return checks cannot
  replace native-entry/lifetime assertions, forged source-free artifacts, snapshot
  reuse, scoped borrowing or reload alias checks. The same arithmetic at a kernel,
  installed binding and SDK transport boundary can establish three different rules.

Other suspected duplicate cases remain candidates until their assertion and boundary
sets have been compared. No numerical reduction target is assigned.

## Organization and representation decisions

### Semantic ownership before file layout

Keep ordinary Rust tests in the owning crate's `src/` tree and existing harness.
Use ordinary modules and descriptive semantic names. Split growing mixed files by
contracts such as ordinary execution, rejection/admission, linking and lifecycle,
not historical feature commits or catch-all `misc`/`regressions` buckets. Keep a
small coherent inline test beside a private kernel when moving it hurts readability.
A module rename or splitting one long file is not by itself completion.

For example, conversion execution, invalid implementation diagnostics and linked
module identities can have adjacent owner-local modules, while their fixed inputs
live under `tests/fixtures/language/conversions/`. This is a proposed layout, not
a mandate to move public examples or every existing test. Final paths are frozen
per batch after inspecting actual consumers and feature gates.

Do not move lower-layer tests up to the SDK for convenience, or widen production
visibility/re-exports to share test setup. Stdlib/runtime composition stays at its
consuming layer; a runtime unit harness and ordinary runtime dependency have
separate Rust type identities. Existing narrow cross-target sharing remains narrow.

### Select the simplest form that retains the contract

| Contract shape | Preferred form | Explicitly preserve |
| --- | --- | --- |
| Many fixed inputs, identical configuration/assertions | `rstest #[files]` over an owner-local homogeneous directory | Independent discovery, admission policy, source names and missing-input failure |
| Same flow, differing options/expectations/spec metadata | Descriptive `#[case::name]` with typed arguments | Each variant and its meaningful diagnostics; no accidental Cartesian expansion |
| Stable formatter/CLI/disassembly presentation | Input/expected goldens plus small owner-specific orchestration | Width/newline/error behavior, exact text and deterministic ordering |
| Static typing/recovery | File-based UI diagnostic expectations plus typed diagnostics and explicit surviving-fact assertions | Kind/severity/spans/order, useful neighbors and codegen eligibility |
| Numeric/codec/proof kernels | Native structured tables/reference algorithms | Exact bits, allowed sets, traps, corruption and identity targets |
| GC/host/async/reload/native retention | Explicit Rust scenarios, optionally loading long fixed scripts | Ordered operations, handles, roots, poll/drop/wake, publication and cleanup |
| Node/Python/editor/browser/product cases | Existing runner's named cases, tables/corpus and selected data files | Real consumers/processes and framework/platform-specific behavior |

Use workspace-managed `rstest` only as a dev dependency where needed. Keep current
features disabled unless a concrete test requires them. Do not add `rstest_reuse`,
`datatest`, snapshots or a new global support crate by default. Existing exact
text comparisons suffice for the initial goldens; a new dependency requires a
recorded unmet requirement, not speculative future convenience.

The selected mechanism follows [rstest 0.28.0](https://docs.rs/rstest/0.28.0/rstest/attr.rstest.html)
and the source/expectation separation in [rustc UI tests](https://rustc-dev-guide.rust-lang.org/tests/ui.html)
(reference review: 2026-10-24). Do not import compiletest's whole directive system:
Kagari's semantic owners and lifecycle assertions are not presentation snapshots.

### File-based UI diagnostic tests (planned)

Here **UI tests mean compiler user-facing diagnostic tests**, following rustc's
source/expected-output model, not browser or editor GUI tests. They are a concrete
TO03/TO04/TO09 deliverable, not merely a reference or an implemented feature of the
pilot. Keep them in their current owner library/process harnesses with `rstest`;
no new compiletest-style framework, crate, executable or source directive DSL.

#### Ownership and fixture layout

TO00-frozen owner-local layout for the planned UI implementation:

```text
crates/<owner>/tests/fixtures/ui/<family>/
  <case>.kgr
  <case>.diagnostics.json   # parser/HIR diagnostic records
```

Each `.kgr` is one independently discovered case. Its expectation is derived from
the same stem, not discovered through another glob. Missing, malformed or orphaned
expectations fail the owning inventory check; unknown expectation fields fail
rather than silently disabling assertions. An explicitly empty array represents
no diagnostics, not a missing expectation. Keep configuration/spec metadata in
small typed Rust cases or contract-homogeneous groups; do not infer it from source
text or parse arbitrary comments as instructions.

| Owner | What the UI test proves | What stays separately asserted |
| --- | --- | --- |
| `kagari-syntax` in TO03 | Parse diagnostic code/severity/range/message/order on recoverable input | Typed diagnostic kind/payload where stronger, partial tree, token/lossless recovery and budgets |
| `kagari-hir` in TO04 | Static diagnostic records from the checked source snapshot | Healthy-neighbor facts, query reuse, inference/definition identity and codegen eligibility |
| `kagari-cli` in TO09 | Actual human/JSON presentation and process status | Command/filesystem effects, bounded process cleanup and source-location validity |

Use existing typed diagnostics and reporting adapters. In particular,
[`DiagnosticRecord`](../crates/kagari-source/src/presentation.rs) already owns code,
severity, primary span, message, labels and notes. A test-only expectation/projection
may describe those facts, but must not reimplement message generation, resolution,
line indexing or semantic validation. Do not introduce production serialization
APIs or widen visibility just for goldens. Reuse existing JSON support; any extra
parsing dependency is dev-only and limited to an actual UI-test consumer.

#### Structural diagnostic expectations

The TO00-frozen `.diagnostics.json` format is a top-level ordered array of records:

- `code`: the producer's diagnostic code; `severity`: `error` or `warning`.
- `primary`: `null` or an object with `path`, `start` and `end`. The path is the
  fixture's logical source name and the range is zero-based, end-exclusive UTF-8
  byte offsets, not character/UTF-16/display columns.
- `message`: the complete producer message, not an error-substring assertion.
- `labels`: an ordered array of objects with `span` and `message`; each span uses
  the same logical-path/byte-range shape. `notes`: the complete ordered array of strings.

For example, the existing 23-byte missing-parenthesis input has no final newline:

```text
fn add(lhs: int { rhs }
```

Keeping its existing logical source name `test.kg`, the proposed expected record
uses the current `DiagnosticRecord::from_diagnostic` mapping, including its primary
label (this example is a design sketch, not an installed fixture suite):

```json
[
  {
    "code": "KG_PARSE_EXPECTED_FUNCTION_PARAMETER_LIST_END",
    "severity": "error",
    "primary": { "path": "test.kg", "start": 16, "end": 17 },
    "message": "expected `)` after parameters",
    "labels": [
      {
        "span": { "path": "test.kg", "start": 16, "end": 17 },
        "message": "primary"
      }
    ],
    "notes": []
  }
]
```

This is a test-data convention frozen by TO00, not the CLI JSON protocol or a
runtime ABI. TO03 admits the first parser fixtures against it. Compare parsed values with exact record count,
fields, nullability, ranges, messages, labels, notes and order. Do not sort/deduplicate
records, discard unexpected warnings, or accept only the first matching diagnostic.
An identical code is not sufficient to replace stronger typed-kind/payload checks.

Resolve file/revision identities against the captured source set **before** mapping
them to logical fixture paths. Wrong-source, stale-revision, invalid-range or lost
location data must not become a matching anonymous span through normalization.
Identity/retention-specific scenarios stay explicit Rust tests; UI projections
cannot replace those contracts.

#### CLI presentation and normalization

CLI fixtures use `.kgr` plus `.stdout`, `.stderr` and `.exit` expectations, including
empty streams. Keep human/JSON modes and command arguments explicit in owner-local
Rust groups. Run the real `kg` process through its existing integration boundary.
Compare human streams exactly. For JSON mode, validate one complete record per
line and compare all parsed fields, record order and summary fields, together with
the exit code; JSON object-key order is not a substitute for field/value equality.
Retain any existing exact-byte serialization assertions as separate contracts.
Do not reduce rendered JSON to the parser/HIR record subset. Reuse
[`report.rs`](../crates/kagari-cli/src/report.rs), not a test renderer that merely
resembles production output.

Normalization is deny-by-default. A reviewed case family may replace a known
checkout/temporary-root prefix with a stable fixture-root token, or normalize path
separators in actual path fields. It must not globally replace backslashes or trim
messages, excerpts, whitespace, CRLF, source offsets, diagnostic/label order or
summary/status differences. Parser/HIR projections do not serialize process-local
IDs into stable goldens; validate their ownership/revision before projection
instead. Public CLI revision fields remain checked through deterministic fixture
setup and explicit expected values, not blanket scrubbing. Cases whose contract
requires other identities or physical bytes retain direct assertions. A display
line/column contract is checked at its presentation owner, not inferred from the
byte-offset expectation.

#### Compare/update/verify loop

Normal test runs and CI are read-only on tracked expectations. On an intentional
diagnostic change, select the affected cases, emit received diagnostic/stream/status
artifacts under `target/test-organization/ui/`, inspect their semantic and textual
diffs, and update only those reviewed expectations. An optional updater must be
explicit and case-scoped; no updater command or automatic bless mode is implemented
or required by this plan. Rerun ordinary non-update comparison afterward; generating
an expectation is not a passed test.

UI-driver acceptance includes mismatched code/message/span, extra/missing/reordered
records or labels, malformed/missing expectations and CLI wrong-exit/output failures.
Cover Unicode/CRLF coordinates through existing owner cases, not by normalizing
them away. Positive cases must also assert their owner's successful result; expected
diagnostics must not absorb a panic, cancellation, loader failure or unrelated
operational error. Recovery and semantic-fact scenarios remain explicit even when
they reuse the same source fixture as a UI case.

### File discovery and helper rules

- Prefer discovery for homogeneous fixed source groups, not a second large manual
  registration list. Keep a few heterogeneous cases explicit. Group new test-only
  data by contract instead of adding filename-based policy branches or an endlessly
  growing exclusion list. The five public-syntax exclusions from the pilot retain
  their documented reasons; this plan does not reopen that completed pilot.
- Preserve logical source names independently of physical fixture paths. Extraction
  preserves exact bytes, including leading LF, missing final LF and UTF-8 ranges.
  Use narrow LF attributes; keep intentional CRLF/literal-byte construction explicit.
  Even a misleading old logical filename must not change silently during extraction.
- `#[mode = str]` is suitable when only contents are needed. Use path mode and one
  read when original file identity matters; do not parse a generated test name to
  recover paths, pair two file globs into a Cartesian product, or promise a speedup.
- Track directory additions/removals where the framework's discovery requires it.
  Reuse a build script only in crates that need the hook; keep standalone consumers
  independent of external workspace data. Missing required test data fails loudly.
- Exact golden updates are explicit and reviewed. Never auto-bless a whole tree,
  snapshot private `Debug` structure, or normalize away a tested byte/span/order.
- A helper needs compatible actual consumers or an independent construction/assertion
  responsibility. Inline single-caller forwarding wrappers. A substantive builder,
  bounded generator or resource guard is not redundant merely because it has one
  entrypoint. Remove superseded setup in the same batch; do not leave dual runners.
- Immutable baseline reuse does not permit sharing mutable hosts, heaps, registries,
  source sessions, cancellation tokens, browser contexts or filesystem projects.
  Keep dynamically collected documentation checks and bounded numeric predicates
  when their aggregation serves an actual contract/resource bound.

## Consolidation and migration protocol

Each owner batch follows one review loop:

1. **Classify:** record family, current owner/selector, routes/features/platform,
   expected facts/effects/cleanup, input form and proposed disposition.
2. **Prove:** distinguish repeated setup, repeated registration and genuinely
   duplicate coverage. A deletion requires equivalent owner, boundary, settings,
   identity, assertions and side-effect/trap coverage; retain stronger facts.
3. **Refactor:** fix mixed ownership first, move data, parameterize appropriate
   scenarios, then inline/remove obsolete wrappers. Keep the batch buildable.
4. **Verify:** compare mappings and exact extraction bytes, list affected cases,
   execute selected contracts and the small directly affected regression set.
   Fix filters, discovery, cleanup and consumers in the same checkpoint.
5. **Close:** update this phase's ledger and lasting guidance; record retained/no-op
   families and evidence for deletions. There is no permanent duplicate registry.

Temporary machine inventories and old/new mapping files live under ignored
`target/test-organization/`. This plan retains necessary decisions, representative
mappings, exceptions and failures; tracked documentation must not depend on those
files. Keep a compact family-level ledger, not another manually synchronized list
of every test function. Generated/feature-gated/doctest/dynamic cases must be
classified explicitly; source text occurrence counts are not harness inventories.

## TO00 frozen family dispositions

Tracked-source/manifest review covers the following families, including all
submodules, inline tests, support files, rustdoc/compile-fail examples and existing
ignored/generated variants under each listed owner. Dispositions are **M** migrate
same-flow inputs, **O** reorganize/consolidate evidenced overlap, **R** retain the
current typed/stateful contract, and **E** preserve executable product/example or
measurement roles. A mixed row assigns each kind separately, not a blanket
permission to skip a known conversion. This is a family inventory, not a claim
that every generated case has been compiled or executed.

| Owner / complete family grouping | Frozen disposition and destination phase |
| --- | --- |
| SDK conversion/default/standard trait parents and existing children | M ordinary fixed programs; O rejection/linking/admission separation; R forged contracts/callback lifecycle; TO01 |
| SDK other language modules: types/generics/associated items, inference/local functions, dispatch/interfaces, collections/iteration, values/operators/enums/strings/Result/Try | M fixed same-flow execution, O mixed boundaries, R typed/temporal observations; TO02 |
| SDK numeric/float methods and math children, numeric example/check generator | R bounded predicates/reference/transport and checksum rules; M useful finite outer cases only; TO02 |
| SDK language-contract runner/groups and syntax examples | R existing route/effect/native obligations and completed discovery pilot; M only substantial fixed input where useful; TO02 |
| SDK engine sessions/source snapshots/registration/declarations/package inputs/source modules | R evolving snapshot/registration/capture facts; M independent fixed source, O actual local overlap; TO06 |
| SDK program source/native preparation/artifacts/Cranelift and AOT | R verified products, real-native/fallback, code lifetime and source-deleted deployment; M independent rejection rows; TO06 |
| SDK embedding conformance/error traces, typed/scoped calls/transfer, host interfaces/offline functions/composites/nominals/provider reset | R ownership, origins, reentry and installation sequences; M source data only where useful; TO06 |
| SDK async blocks/conversion/diagnostics/groups/scheduler/tasks/handlers/waiters/lifecycle/statics/debugger and generic reload | R cold execution, poll/drop/wake, aliases, pinned versions and cleanup; M independent rejection/source data; TO06 |
| SDK external artifact/async consumers, support and public API compile-fail examples | R exact visibility, dependency, lifetime and source-free proof; TO06 |
| SDK runnable examples, artifact producer/AOT host and baseline examples | E retain product/host/measurement roles and required features; TO06 |
| Source literal/source/database/diagnostic owners | R byte/CRLF/offset/transaction/code tables and scenarios; TO03 |
| Syntax lexer versus parser contexts and conformance/spec rejection | O misplaced contexts and proven duplicate rejection; M spec rows; TO03 |
| Syntax parser AST/item/expression/control-flow/collection/module/pattern/async/static/update families and attributes/declarations/interpolation | R selected trees, metadata, physical bytes and recovery; M only ordinary uniform inputs; TO03 |
| Syntax error fixtures | M missing-delimiter UI pairs, R typed-kind and partial-tree assertions; TO03 |
| Syntax budgets/cancellation/grammar accounting, inline module/raw-kind tests and public API doctests | R generated/admission/witness/lifetime contracts; TO03 |
| HIR structural lower/local-function/pattern/text/typeck standard/trait/language-catalog families | R typed facts/catalog validation; TO04 |
| HIR ordinary resolver/spec/typeck diagnostics versus recovery | M diagnostic UI data, O separation, R healthy-neighbor/constant/inference/codegen facts; TO04 |
| HIR all analysis query/arena/identity/owner/fingerprint/native/completion/member/generic/trait/static namespaces | R query/edit/reuse/eligibility and captured-source ownership; TO04 |
| HIR imports/solver/provenance/aggregate implementation search/native/language roles, inline constraint/scalar/type algorithms and doctests | R bounded algorithms, scope/authority and cancellation; retain ignored solver measurement separately; TO04 |
| Package manifest/module/workspace/graph/analysis acquisition | O four virtual-source wrappers at existing support owner and mixed manifest/module groups; R lock/capture/link/alias/relocation/mutation, M fixed trees only; TO04 |
| Compiler foundation/lower/verify/source-program/ABI/analysis/async/codec/interface/numeric/origin/export/pass/pattern/program-point/visibility families | M four compile-success inputs; O emission/admission placement and duplicate adoption support; R typed effects, source ownership and edited-MIR resealing; TO05 |
| Compiler bytecode artifact/literal/host/identity/interface/lowering/validation and source-lowering ABI/debug families | R constructed admission/flow/ABI and origins; TO05 |
| MIR codec/compaction/scalar/verification and seal doctests | R forged wire, provenance/work bounds, fixed-point interruption and immutable verification evidence; TO05 |
| Bytecode artifact/wire/availability/flow/async/program/origin/trait proof families and seal doctests | R source-free identity/integrity/resource/solver/cancellation contracts; TO05 |
| Bytecode disassembly body/descriptor/instruction/presentation/resolution families | M stable text fragments, R typed fixtures/foreign scopes/independent decoding and IO failure; TO05 |
| Contract callable/evidence/type/application/inheritance/native/proof/export/numeric/decode/operation families and metadata example | R typed tables, forged evidence, prepared proof accounting and representation probe; TO05 |
| Runtime substrate/module admission/state/retention/reload/execution storage/metadata/frame/type/environment families | R invalid records, ownership/generations, atomic publication and cleanup; M independent parameter rows only; TO07 |
| Runtime GC directory/native/roots/values/fields/array/sequence/string/future/task and host borrow/scope/path/view families | R tracing, accounting, commits, cursors, scoped aliases, native resources and async roots; TO07 |
| Runtime native assembly/default/proof/transfer/builder/callable/composition/context/module/catalog/registry/schema/execution families | R registration/ABI/payload/capability/owner/dependency/rollback/transfer sequences; M independent rows; TO07 |
| Runtime objects/metadata/scheduler/session/state/builtin/numeric/range/reflection/value/error/resource families | R typed ownership and wake/session/collection/termination semantics; M compatible scalar tables; TO07 |
| Runtime external schema/derive/Serde and all API positive/compile-fail docs, support and pointer kernels | R external expansion/lifetime/borrow/identity and focused Miri proof; TO07 |
| Runtime examples, ignored diagnostics/memory/frame/native probes | E preserve allocator process, diagnostics gates, ignored status and measured windows; TO07 |
| VM execution/conformance/numeric/collections/interfaces/statics/sessions/debugger/GC/mutation/access/owned-drive/wait families | M independent fixed inputs, R VM-specific faults/temporal facts; TO07 |
| VM native-boundary artifact/control/callback/enum/host/interface/reference/conversion/function/GC/payload/resource/session/storage/transfer families | R source-free scope, result/borrow/commit/reentry/retention; M independent admission rows; TO07 |
| VM preparation/JIT/source program/export/support and external allocator/measurement targets | O equivalent host/reflection wrappers; R fresh codec contexts and native proof; E separate measured process; TO07 |
| Stdlib assembly/registration/documentation/iteration/numeric/runtime-boundary families | R assembly lifecycle, dynamic doc aggregation, reference/binding/installed composition; O compatible preparation only; TO07 |
| Common cancellation and identity/map/table/wire inline/modules/compile-fail/examples | R wake retirement, retained prefixes, explicit scope/import, wire bounds and non-ignored ownership matrices; E isolated allocator example; TO08 |
| Types integer/numeric/payload/ty access/substitution/identity/wire/declaration/language families | R independent references, bounded transforms, ownership/policy/binders and corruption; M independent table rows only; TO08 |
| ABI/codegen/derive crates | R no new suite: ABI/backend consumers, external schema expansion and runtime API compile-fail owners establish these contracts; TO08 |
| Cranelift configuration/scalar/unsupported/link/trap and retained product/failure recovery/thread-transfer families | M independent rows, R verified native entry, exact traps and related lifetime/recovery sequences; TO08 |
| Formatter exact layout/literal/adjacency/grammar inventory and private comment/layout kernels | M goldens/named cases, O CI inventory discovery, R byte/cancellation/private-kernel observations; TO09 |
| Lint ordinary usage/naming/capture/static/local-function versus level/reuse/partial-fact/module policy families | M ordinary data, O mixed independent scenarios, R healthy facts/eligibility/reuse and required errors; TO09 |
| CLI args/config/report/disassembly/project/formatting/writeback and actual toolchain process target | M named args/config and UI outputs, O compatible filesystem setup, R process/IO/platform/writeback/lock/product semantics; TO09 |
| Analyzer protocol/document/session and stdio process | M URI tables, R physical edit/snapshot/version/transport/shutdown and child cleanup; TO09 |
| Web protocol/output/portable adapter and browser artifact producer | R lossless numeric/output/GC/admission/busy/cancel/reset and exact producer selector; TO09 |
| Editor Tree-sitter corpus/native binding/generated/WASI grammar/inventory and lexical oracles/adapters | R existing corpus/oracles/product distinctions; O independent native grammar variation names, no merged adapter semantics; TO10 |
| Editor VSIX/manifest/staging and actual VS Code/Zed hosts | R payload/provenance/path guards, actual products and manual/incomplete host acceptance; TO10 |
| Site contracts/HTML/source includes/grammar/provenance/Pages recipes/Worker protocol and Node tests | R native tables/helpers, mutation isolation, exact assets/digests and product graph guards; TO10 |
| Site browser/WASM/Worker/Playground runners and build/check orchestration | O substantial fixed browser sources with explicit metadata; R real browser/Worker/UI lifecycle, faults, quotas and teardown; TO10 |
| Python structure/exception/benchmark-results tests and checker support | R unittest/subTest, exact bytes/locations, strict policies and independent report mutations; TO10 |
| Feature/AOT scripts and Rust/editor/Pages workflows | R separate producer/consumer and graph edges, deleted sources, actual binaries, checked publication and prerequisites; reconcile selectors only; TO10 |
| Benchmark workspace member/workloads/forms/numeric/controls/diagnostics/profile/storage examples and Python measurement/report/profiler drivers | R/E independent references, frozen workload/timing/provenance/allocator/manual roles; no performance redesign; TO10 |

### Frozen UI and first-batch decisions

The structural JSON schema above is now frozen for this migration: every record
requires `code`, `severity`, `primary`, `message`, `labels`, `notes`; every present
span requires logical `path`, integer `start`/`end`; label objects require `span`
and `message`. Unknown/duplicate fields, wrong types and invalid ranges fail.
Arrays remain ordered and required even when empty. There is no format-version,
legacy reader, implicit field default or automatic updater. Normal comparisons
are read-only; the allowed normalization remains the narrow path-root/path-field
policy above, not a hook for arbitrary regular-expression rewrites.

Representative source/owner fixtures are frozen as follows: parser missing-paren
and missing-block-end records keep `test.kg`, exact EOF bytes and typed kinds;
HIR `reports_unknown_parameter_type`, `reports_unknown_return_type` and
`reports_function_call_argument_type_mismatch` retain their typed payloads while
using final producer records; unresolved-body-hole recovery retains neighbor and
codegen observations. CLI renderer `rendering_uses_retained_revision_without_hir_or_process_ids`
and actual-process `lint_cli_policy_required_errors_and_rule_help` seed human/JSON
expectations, preserving Unicode/tab/CRLF, unavailable-location and status/summary
facts. Full CLI process goldens do not yet exist and will be established in TO09,
not fabricated from this classification.

The first TO01 pre-edit harness inventory lists 102 cases: 19 conversion, 56 default
method (including children/generated arithmetic) and 27 standard-trait cases.
This is discovery only, not baseline behavioral execution. Ordinary fixed programs
move to three contract-specific SDK fixture groups; logical compile names remain
`standard-traits.kgr` for conversions/standard traits and `defaults.kgr` for defaults.
They retain GC threshold `Some(1)`, result `42` and source/artifact/artifact-JIT
routes. Generated arithmetic keeps its two interpreter routes and invalid-record/
linking/callback sequences keep their existing explicit assertions. Owner-specific
case/byte maps are frozen immediately before each edit; future phase maps will
likewise be frozen before their own changes, not inferred from source counts.

## Execution phases

Default order is TO00 through TO11. Work can be delegated within disjoint owners
after shared contracts are frozen, but do not migrate the same source/support file
concurrently. Each owner sub-batch must build and finish its selected validation
before another owner starts. Large phases below are ownership boundaries, not
permission for a single unreviewable patch.

Use coherent authorized phase checkpoints and `Test-Organization-Phase: TOxx`
trailers. Never rewrite user commits or mix production fixes into a test migration.

### TO00 — Freeze the complete family inventory and destination map

- [x] Establish baseline, owner map, framework decision and finite phase scope.
- [x] Enumerate all 24 members and non-Rust runner families from tracked sources,
  Cargo metadata and existing workflow/script entrypoints. Include inline modules,
  integration cases, doctests, macro-generated cases, ignored/manual tests,
  feature-gated cases, examples and benchmark acceptance roles.
- [x] Record each family's disposition and target semantic module/data directory;
  identify actual helper consumers and high-value duplicate candidates. No family
  disappears just because its crate has no dedicated test suite.
- [x] Freeze batch mappings and validation selectors before implementation. Start
  with the existing 15 conversion source calls; do not build/list every workspace
  harness locally just to obtain a global count.
- [x] Freeze the planned UI diagnostic schema, logical source naming, pairing,
  normalization allowlist and failure/update rules using representative parser,
  HIR and CLI cases. Separate diagnostic projection from actual CLI rendering;
  retain all stronger typed/recovery/identity assertions in the case map.

Exit: all owner rows have an assigned family inventory and phase; remaining
uncertainty is named. No production changes or migration framework is required.

### TO01 — Conversions and ordinary trait execution

Scope: SDK `conversion_traits`, `default_methods`, `standard_traits` and their
existing child modules, classified case by case.

- [x] Move the 15 ordinary conversion programs into a contract-owned fixture group,
  with discovery where their route/result/configuration is uniform. Preserve
  threshold `Some(1)`, all three routes, source identity and every script predicate.
- [x] Separate conversion rejection and linked-module orchestration. Extend the
  same treatment to ordinary default/standard-trait cases; preserve callback,
  forged-environment, source-origin and source-free cases as distinct owners.
- [x] Retire local `execute` wrappers only when their remaining callers no longer
  need them; do not retain a single-caller forwarding layer around the shared runner.

Exit: all original programs and boundary cases are mapped; same-flow registration
is concise and reusable execution has one implementation. No lifecycle assertion
is replaced by a final `42`.

### TO02 — Remaining SDK language families and numerical organization

Scope: `kagari/src/runtime/tests/language` outside TO01, plus existing language
conformance case groups. The completed syntax pilot is a retained reference.

- [x] Process separate owner batches for types/generics/interfaces, collections/
  iteration, and values/operators/strings/Result/Option. Split mixed rejection,
  artifact mutation and special runtime cases before adopting discovery.
- [x] Review numeric methods and their generators: parameterize useful finite outer
  dimensions only; keep bounded inner predicates, original route differences,
  independent references, typed carriers, IEEE bits/NaNs/zeros and cleanup.
- [x] Give every residual family a migrated or reasoned-retained disposition; remove
  proven equivalent smoke cases, not installed-binding/transport boundary proof.

Exit: no unexplained same-flow wrappers or unclassified cases remain in this
subtree. No operation/type/bit-pattern/backend product is added by default.

### TO03 — Source, lexer, parser and grammar contracts

- [x] Preserve source/literal byte tables and transactional source-database scenarios
  at `kagari-source`; parameterize/extract only where clearer.
- [x] Move parser-context assertions out of lexer-only ownership. Reconcile parser
  conformance/rejection groups, including the confirmed duplicate described above;
  retain stronger diagnostics and spec links.
- [x] Introduce the first file-based UI diagnostic suite using the existing missing
  delimiter cases. Admit `.kgr`/`.diagnostics.json` pairs through the frozen TO00
  schema, reuse the real producer and compare full records. Verify pairing and
  intentional mismatch failures; retire superseded inline expected text only when
  the UI assertion covers it, retaining stronger typed-kind assertions.
- [x] Use files/tables for appropriate parse-success and diagnostic families, while
  retaining losslessness, partial trees, cancellation/budget and grammar coverage
  accounting. Independent spec rows should be independently selectable.

Exit: lexer versus parser/recovery responsibilities are explicit, duplicate
rejections have one owner, and useful recovery is not replaced by strict parse-ok.
The parser UI suite detects diagnostic and expectation mismatches independently;
matching a golden is not a substitute for recovery assertions.

### TO04 — HIR and package inputs

Two separate owner checkpoints: HIR first, then package acquisition/composition.

- [x] Separate HIR ordinary rejection, recovery facts, import identities and
  incremental query sequences. Reuse existing analysis/lowering helpers and keep
  cache reuse, old snapshots and codegen eligibility assertions explicit.
- [x] Extend file-based UI expectations to ordinary HIR diagnostics using final
  producer records and captured source identities. Preserve healthy-neighbor,
  type/constant and codegen-rejection observations as explicit Rust assertions;
  do not replace them with matching diagnostic output.
- [x] Consolidate the repeated virtual-source conversion in existing package test
  support. Separate manifest policy, module-tree discovery and graph/lock cases;
  externalize fixed trees only when mutation remains legible in Rust.
- [x] Preserve cancellation, aliases, provider/environment identity, fingerprints,
  retained captures and filesystem link/platform gates. Do not turn healthy-neighbor
  facts into a diagnostic-only golden.

Exit: family mappings include both static results and temporal/cache facts. Shared
input mechanics have one suitable owner; lower-layer tests do not acquire an SDK.

### TO05 — Lowering and executable contracts

Separate checkpoints for compiler, MIR/contract, and bytecode/presentation owners.

- [x] Reconcile compiler `verify` versus lowering/emission tests; reuse existing
  adoption/identity support and move ordinary compile-success scripts where useful.
- [x] Retain constructed MIR, proofs, malformed wire/bytecode and admission scenarios.
  Review them for redundant preparation and meaningful local tables, not mandatory
  source-file conversion. Preserve effects, origins, bounds, cancellation and resealing.
- [x] Extract stable disassembly expected text where useful, keeping variable header/
  fingerprint checks and independent decoding/scoped-identity assertions explicit.

Exit: no forged input is silently repaired by setup, compiler facts remain typed,
and source-free validators never depend on source analysis for test convenience.

### TO06 — SDK host, lifecycle and product contracts

Scope: engine/program/embedding/AOT tests, shared supports, public-consumer targets
and SDK example roles not owned by ordinary language cases.

- [x] Organize by session/installation, host borrowing/reentry, execution cleanup,
  async scheduling, statics/reload and native/deployment ownership.
- [x] Extract long fixed scripts and remove demonstrated redundant local setup;
  retain trap/recovery, poll/drop/wake, generations, aliasing and code-lifetime
  sequences. Preserve current pinned reload semantics, not the queued update model.
- [x] Keep public API/feature isolation, source-free consumers, actual-native status,
  artifact producers and source-deleted AOT checks. No universal lifecycle DSL.

Exit: lifecycle tests remain explanatory Rust scenarios with explicit observations;
external consumer/producer boundaries and exact product identities remain covered.

### TO07 — Runtime, VM and stdlib ownership

Three owner checkpoints, preserving their existing dependency direction.

- [x] Review runtime GC/roots, frames/cleanup, host scopes/borrows, native registration,
  module initialization and reload families. Keep stale/foreign handles, rollback,
  generation guards and accounting facts; table only independent variations.
- [x] Review VM execution/native-boundary/debugger/owned-drive families. Consolidate
  evidenced redundant wrappers, while retaining fresh codec identity contexts,
  callback effects, mutation commits, traps, cancellation and synchronous reentry.
- [x] Review stdlib assembly, documentation, bindings and runtime composition.
  Consolidate preparation only across compatible consumers; preserve analysis-only
  documentation aggregation and installed carrier/reentry/resource assertions.

Exit: local supports no longer duplicate the same responsibility, while shared
runtime semantics are not reimplemented by a broad test runner. Existing external
schema and allocator/measurement targets retain their purposes.

### TO08 — Common/types and native/compiler API consumers

- [x] Review `kagari-common` scoped-identity/cancellation/wire families and
  `kagari-types` numeric/declaration tables. Preserve reference algorithms,
  retained-prefix lifetimes, explicit imports and corruption bounds.
- [x] Classify ABI/codegen/derive contracts at their real consumer/doctest owners;
  a reviewed no-code-change disposition is expected when already well organized.
- [x] Parameterize independent Cranelift rows where useful, without dismantling
  backend-drop/product retention, failure-then-success, thread-transfer or exact
  native trap scenarios. Fallback is not actual-native evidence.

Exit: low-level owners are covered without creating empty suites or promoting
frontend/SDK dependencies into foundational crates.

### TO09 — Rust tooling and web adapter tests

- [x] Extend formatter golden organization and review its manual full-grammar list
  for discovery. Retain a small local selection; if discovery changes the ignored
  full matrix, update its CI selector and prove it remains CI-owned.
- [x] Organize lint rule, level/reuse and partial-fact tests; parameterize ordinary
  CLI arguments and analyzer URI variants. Keep filesystem mutation, document-edit
  histories, real command/LSP process transport and cleanup explicit.
- [x] Add file-based CLI UI expectations for selected existing human/JSON diagnostic
  cases via the actual process harness. Compare stdout/stderr, summary/JSON-line
  structure and exit status using only the declared normalization. Keep LSP
  transport/GUI/browser scenarios distinct from compiler UI diagnostics.
- [x] Review portable web adapter families and its artifact producer. Preserve
  source-free adapter graphs, root/depth cleanup and exact producer entrypoints;
  do not treat these as substitutes for browser/Worker tests.

Exit: tooling families have coherent owners, stable output expectations and correct
selectors; real process/filesystem/adapter boundaries remain intact.

### TO10 — Native runner ecosystems, workflows and benchmarks

- [ ] Review Node/editor/site and Python families with their existing frameworks.
  Use named cases/subtests and externalize substantial fixed inputs when beneficial;
  retain Tree-sitter corpus and shared lexical oracles instead of duplicating them.
- [ ] Keep real native/WASI grammar products, TextMate/Oniguruma, editor hosts,
  browser/WASM/Worker/UI, standalone feature/AOT guards and platform prerequisites
  distinct. Fake Worker or in-process checks cannot replace actual consumers.
- [ ] Classify benchmark/example/manual roles. Preserve independent references,
  workload registries/digests, timing regions, batches and sampling; representation
  cleanup does not authorize redesigning or rerunning performance acceptance.

Exit: all non-Rust/benchmark rows have a disposition, with only evidence-backed
organization changes. Existing CI does not run every real-editor/browser check;
unrun/manual acceptance remains explicitly unverified.

### TO11 — Final reconciliation and retirement

- [ ] Reconcile TO00 inventory against every migrated, consolidated, split and
  retained family. No owner row or approved conversion remains unaccounted for.
- [ ] Resolve dangling fixture paths, discovery invalidation, documentation filters,
  platform/ignore gates, provenance/digests and workflow/script entrypoints. Remove
  all obsolete wrappers, registrars and genuinely superseded assertions.
- [ ] Finish required local validation, classify pending CI/manual scope and transfer
  lasting rules to `testing.md`. Remove this completed plan; move only concrete
  unresolved follow-ups to their existing roadmap/review owners.

Exit: finite local implementation acceptance is complete. Do not retain a completed
phase ledger merely because exhaustive CI remains pending, and do not mark CI green
without actual results.

## Validation and failure policy

Documentation planning requires content, relative-link and diff checks only.
Implementation uses the repository's focused/local-final/CI split:

- Before a batch, compare source-level mappings and selected harness discovery when
  names change. Do not run/build/list all workspace harnesses as a local census.
- Run changed contracts and a small directly affected regression set with package,
  target and qualified filters. Use `--no-fail-fast` for multiple selections.
  Compilation or discovery is not test execution; skipped selections remain pending.
  Confirm renamed filters select the intended nonempty set: Cargo's successful
  zero-test result is not acceptance of a missing case.
- Verify fixture bytes and logical origins. For discovery changes, probe add/remove
  behavior without editing the registration list, fail missing paired expectations,
  and clean temporary inputs. Scripts/logs/measurements belong under `target/`.
- For UI migrations, verify mismatch/pairing/normalization rejection, all projected
  fields and retained stronger assertions. Separately exercise normal comparison
  after any intentional expectation update; a bless/update run alone never counts
  as acceptance. Do not run an additional full UI matrix locally.
- Review handwritten Rust ownership/imports/visibility, macro tokens, ordinary
  modules and effective LOC including tests; no blanket structural exceptions.
- Required implementation checks are:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Run checker self-tests if its behavior changes. `rstest` stacked-attribute parser
support from the pilot is retained, not permission to suppress new syntax errors.

Select affected Python/Node tests with their native filters. Review commands before
running: some generated-editor checks rewrite products before comparing them, and
site `check` already runs Node tests. Update exact-name dependencies such as the web
artifact producer, ignored formatter inventory, Windows CLI selections and
standalone-manifest targets in the same batch. Preserve normal/build versus dev
edge distinctions in dependency guards and source-free producer/consumer separation.

A migration-caused failure is repaired before changing owners. For an unrelated
baseline failure, reproduce only the affected selection, record command/cause and
owner here, and do not weaken assertions or silently fix production behavior.
Previously passing unaffected checks remain evidence; do not rerun them for a
fresh all-green transcript. Use workspace profiles, default target and default
parallelism. Full workspace tests and the complete feature/backend/platform matrix
remain CI-only, not a local sweep split across packages.

There is no compile-time, runtime, allocation or RSS target in this task. Any optional
measurement must record machine/toolchain/profile/features/parallelism/cache/workload
and separate compilation, preparation, execution and memory. Framework adoption,
fewer Rust strings, fewer binaries or a different test count is not speed evidence.

## Progress ledger

| Phase | Implementation state |
| --- | --- |
| TO00 | Complete: family dispositions, UI schema and first-batch inventory frozen |
| TO01 | Complete locally: 59 ordinary inputs, 43 retained cases; focused validation passed |
| TO02 | Complete locally: SDK language families reconciled; 463 selected cases discovered |
| TO03 | Complete locally: parser UI pairs, lexer/parser ownership and spec rows |
| TO04 | Complete locally: HIR UI/recovery split and package input preparation |
| TO05 | Complete locally: lowering/admission split, adoption support and disassembly goldens |
| TO06 | Complete locally: SDK lifecycle ownership, fixed sources and preparation cleanup |
| TO07 | Complete locally: runtime rows, VM sources/support and stdlib preparation |
| TO08 | Complete locally: common/types/native rows and public consumer review |
| TO09 | Complete locally: formatter/lint/tooling rows and real-process CLI UI |
| TO10 | Planned, not started |
| TO11 | Pending completion of owner phases |

2026-10-24 TO09: formatter -> lint -> CLI -> analyzer -> web checkpoints complete.
Eleven formatter pairs and seven lint sources are byte-exact fixtures; meaningful
short option/rejection/URI rows are named. Formatter's manual 48-file full inventory
is now ignored directory discovery with the exact CI selector updated; nine local
representatives retain their original policies. An unchanged target mirror proved
48 -> 49 -> 48 discovery, selected execution and default ignore; real inventory
listing confirmed all 48 ignored cases without locally running that CI matrix.

Six real kg process rows cover warning, denied warning and required error in both
human/JSON modes from two existing source programs. All 18 stdout/stderr/exit
expectations include empty streams. Revision 1 was verified, not scrubbed. Human
path mapping touches only recognized location slices; JSON mapping touches only
reviewed primary/label path fields. Full records/summary, stderr error footer,
source coordinates, revision, messages/notes/excerpts and order remain exact.
Strict shape/required/duplicate/unknown/type/range/status admission rejects malformed
expectations. Parent review reproduced nested object-as-array acceptance and fixed
the test-only map visitor; raw duplicate discriminator/record fields remain rejected.
The existing process helper now owns exclusive temp roots, bounded concurrent pipe
capture, deadlines and kill/wait cleanup; no production process policy changed.

All 80 original functions/21 families map to their retained or parameterized owners.
Renderer Unicode/CRLF/retained/missing-location checks, filesystem mutation/locking,
LSP framed transport/edit history/cleanup and all ten web adapter/producer tests
remain distinct. Web's exact browser producer and absent product are untouched.
Two CI selectors changed (ignored formatter inventory and separated JIT/AOT argv
rejection); only three dev rstest edges added, no target/production graph change.

Local selection passed 71 distinct permanent cases initially, then six new strict
shape/duplicate cases (77 distinct total); the focused repair rerun passed 24
negative/JSON-process/normalization checks. Real wrong-exit/wrong-stderr/reordered
record probes failed and restored comparisons passed. Original fixture/source/
body/config/selector proofs, workspace Clippy, format, structure (1,603 files,
zero violations/exceptions) and diff checks passed. No remaining reproduced errors.
Full suites, ignored grammar execution, JIT/AOT/platform matrices, standalone graphs
and actual browser/Worker/products remain CI/manual scope. TO10 is next.

2026-10-24 TO08: common -> types -> Cranelift checkpoints complete. Four original
functions expose 12 capacity workloads, six valid/eight invalid character inputs,
and four malformed-link/four exact-trap rows. All row values, native direct calls,
trap coordinates, resource/reference and identity assertions are preserved. Common
scoped imports/retained prefixes, cancellation retirement, wire bounds; types'
reference algorithms/substitution/binders/corruption; and native product retention,
failure-then-success and thread-transfer scenarios remain unchanged. ABI/codegen/
derive contracts stay at actual consumer/doctest owners without empty suites.
All 95 files/59 original definitions are accounted for; 92 Rust files and 55
retained test bodies are unchanged. Only three dev rstest edges were added; no
production behavior, new target or active CI/script selector change.

Local selection passed six common, 15 types and 11 Cranelift tests (32 distinct).
Four capacity rows plus lifetime/cancellation neighbors were selected, not all
12 workloads; every character/native row and three native temporal regressions
passed. Workspace Clippy, format, structure (1,597 files, zero findings/exceptions)
and diff checks passed. No carried errors. Eight capacity rows, other suites/
API fences and exhaustive feature/platform/AOT acceptance remain CI-owned.
TO09 is next.

2026-10-24 TO07: runtime -> VM -> stdlib checkpoints completed serially. Seven
runtime groups expose 23 independent admission/GC/ABI/proof/input rows; two VM
groups expose 15 optimization/forgery rows. Sixteen VM scripts retain 13,212 exact
bytes. Four proven VM wrappers and 17 consumers now use actual standard/preparation
owners; reflection deliberately keeps its unverified load_program admission path,
not the stronger load_test_module helper. Two stdlib fixture producers serving
12 tests share compatible compilation only. All 38 row arguments match original
tables; no test deletion, new target or semantic production change.

All runtime/VM/stdlib families, 889 original static test definitions, 561 support/
example bodies and 39 API fences are accounted for. GC roots/native resources,
foreign/stale/generation checks, scoped borrows, frames, reentry, wake/session,
mutation commits, source-free native registration/reload and installed stdlib
composition keep their temporal/typed assertions. Dynamic documentation aggregation,
external schemas/derive/Serde, ignored probes and allocator measurement processes
retain their original roles; no lower-layer SDK dependency was added.

Local selection passed 26 runtime, 51 VM and 18 stdlib tests, plus ten scoped-host/
callback-loan compile-fail doctests. Two affected VM cases passed again after unused
import cleanup; no new coverage is claimed for repeats. Exact source/body/row/EOL
proofs, workspace Clippy, format, structure (1,597 files, zero violations/exceptions)
and staged/new-file whitespace checks passed. Eleven exact fixture paths preserve
inherited indentation through narrow attributes. No remaining errors. Other API
fences, full matrices, Miri and measurements remain unrun CI/manual scope; pointer
kernels were not changed. TO08 is next.

2026-10-24 TO06: three serial SDK owner batches completed. Seventeen scripts
(17,436 exact bytes) move to embedding fixtures; three existing aggregates expose
11 named rows. Typed host paths and async factories/iteration have focused owners;
all 233 reviewed test definitions remain. Five proven setup cleanups remove a
duplicate cancellation observer and default-context wrapper, reuse one immutable
provider-forgery baseline with fresh contexts/runtimes per mutation, avoid a
source-module artifact discarded by one caller, and separate provider installation
from final async program selection. The latter removes unused preliminary
preparation/loading while preserving original source-free synthetic metadata and
codec authoring. No mutable fixture state is shared between scenarios.

Sessions/snapshots/registration, host borrows/reentry, errors/origins, cold async
poll/drop/wake/cancel, alias/reload/statics, native product lifetime and all public
API/lifetime fences keep their contract-specific assertions. All eight external
targets and legitimate shared support paths, examples/producers/AOT roles remain;
no script/workflow exact-name consumer needed updating. No product regeneration,
production behavior, dependency or new test target.

Local selection: 49 passing executions across library, public facade, adjacent
external boundaries, one scoped-borrow compile-fail doctest and one independent
no-feature source-free async consumer. That consumer's normal/build graph excludes
frontend/compiler/backend crates; it is not the distinct artifact_features product
proof. Exact payload/body/assertion/settings comparisons and workspace Clippy,
format, structure (1,597 files, zero violations/exceptions) and diff checks passed.
No remaining diagnostics. Full matrices, unavailable artifact products, real-DLL
AOT/source-deleted deployment and manual measurements remain CI/platform scope.
TO07 is next.

2026-10-24 TO05: compiler, MIR/contract and bytecode checkpoints completed in
order. Four compile-success inputs are byte-exact fixtures with named rows. Two
emission tests move from verify to bytecode/lowering, and struct-update lowering
moves unchanged to its lower owner. Proven duplicate adoption/identity collection
now uses existing common::adopt/adopt_bytecode; no forged input is repaired and
local lowering support still has ten actual call sites. Four disassembly expected
fragments become exact goldens; variable headers, typed/foreign scopes, independent
decoding and IO failure assertions remain explicit. No scenario was deleted.

All compiler/MIR/bytecode/contract families (390 original scenarios in 71 groups)
are accounted for: constructed proofs/wire/ABI, effects/origins, typed tables,
resource bounds, cancellation/resealing and seal doctests retain their owners;
137 MIR/contract Rust files are unchanged. No production behavior or new binary.
Local selection passed 48 distinct unit tests and four seal compile-fail doctests.
A real golden mutation failed, original bytes were restored and normal comparison
passed. Workspace Clippy, format, structure (1,595 files, zero violations/exceptions)
and diff checks passed. No carried error; exhaustive suites/matrices remain
CI-owned. TO06 is next.

2026-10-24 TO04: HIR and package completed as separate serial owner checkpoints.
HIR now has 47 exact-input UI pairs (26 bare, five installed-standard, 16 local
functions), 59 ordered final records and 33 stronger typed-payload cases. Shared
source-owned schema is reused, not duplicated; HIR checks captured source/revision
and raw ranges before real record adaptation. Six recovery and six typed-success
functions separate healthy-neighbor/inference/constant/codegen facts from ordinary
diagnostics. Local-function codegen rejection, 12 accepted structural rows and
four spec writeability rows remain explicit. Removed one parser-only HIR function
containing three inputs already covered by TO03 with stronger severity assertions.
All other HIR analysis/query/native/identity/owner/cache/import/solver/catalog,
type/scalar/text/pattern and public-API families retain their distinct contracts;
531 retained/moved bodies and 47 source values were compared to frozen originals.

Package's four identical virtual-source wrappers and 22 callers now use existing
test_support::sources. Manifest policy has its own 12-case owner; module rejection
has four named rows. Graph/locks/capture, workspace inheritance/relocation/physical
identity, analysis aliases/dependencies/exports and platform-link scenarios retain
all 135 original assertion sites. Compact mutable trees stay inline, without a
fixture-tree loader. All 36 original declarations map to 49 independently
scheduled cases; no package coverage deletion or new binary.

Local: 39 distinct permanent HIR tests passed (10 of 47 UI comparisons, 11 typed
payloads, two contracts and 16 adjacent recovery/query/identity/solver/spec cases);
24 selected package tests passed, including Windows links. Six deliberate HIR
pair/malformed/message/order failures were detected and restored. A paired probe
passed and discovery restored exact 26 -> 27 -> 26 names. All temporary captures
were removed; generation of 47 records is not claimed as 47 passed comparisons.
Workspace Clippy, format, structure (1,595 files, zero violations/exceptions) and
diff checks passed. Initial capture compilation timeout was retried successfully;
no carried errors. Remaining UI/typed cases, Unix and exhaustive matrices remain
CI-owned. TO05 is next.

2026-10-24 TO03: source literal/diagnostic/database/coordinate tables and transactions
remain at their existing owners, byte-identical. Lexer token/span/lossless facts
stay lexical; parser-context portions move to parser/literals. Twelve spec rows
are individually selectable. Three equivalent `dyn Effect` rejection occurrences
become one spec-linked assertion retaining the stronger first-error requirement;
field diagnostic count/kind/text and distinct ref/receiver/let-mut cases remain.
Other parser AST/metadata/recovery, attributes/interpolation, generated budgets,
cancellation, grammar accounting and public doctests retain their contracts.

Two existing 23/18-byte missing-delimiter inputs move unchanged into UI/parser,
with complete ordered diagnostic expectations and logical `test.kg`; the separate
25-byte recovery input is unchanged. A test-only source-owned shared schema projects
real DiagnosticRecords after validating captured file/revision and UTF-8 ranges.
Required/unknown/duplicate/null/type/object-shape and range checks are strict;
raw parser spans are validated before adaptation could anonymize invalid locations.
No renderer, production serialization, global runner or test binary was added.
Syntax alone gains existing Serde/JSON dev dependency edges and a fixture-tree
watch; HIR will be the second concrete schema consumer in TO04.

Discovery: source remains 22, syntax 118 -> 143 (exposed rows, ownership splits,
duplicate removal and UI/support cases). Focused local validation passed 71 distinct
syntax library cases, 10 source cases and one selected parser doctest; affected
schema reruns also passed. Real missing-pair and changed-record probes failed as
intended; restoring original expectations passed. Paired additions/removals changed
discovery and were removed. Workspace Clippy, format, structure (1,588 files, zero
violations/exceptions) and diff checks passed. Resolved intermediate import and
strict-Serde-shape/probe-encoding issues; no carried errors. Unrun library cases,
unchanged doctests and exhaustive matrices remain CI-owned. TO04 is next.

2026-10-24 TO02: completed five serial owner batches, each compiled and finished
its focused behavior selection before the next batch. Added 157 exact-source
fixtures and 59 wired child modules. The family mapping is:

| Batch / complete owner scope | Disposition and preserved boundaries | Case accounting: frozen source inventory -> actual discovery |
| --- | --- | ---: |
| associated constants/types, callable traits, generic associated types, instantiation, interface identity, local functions, never, inheritance, inference | 60 discovered fixed programs; explicit diagnostics, bounds, linking, forged admission, generated inference and trap/recovery. Local-function sync/async/export scenario is byte-identical. Inheritance keeps four distinct preparation/route groups; contextual result 84 remains explicit. | 100 -> 105 |
| arrays, collection access/callbacks/interfaces, iteration, list algorithms | 40 exact inputs; LF/CRLF line-rule rows; explicit host, rejection/admission, cursor resources and callback cancellation. Arrays keep their two-consumer runner; list Bool/encoded-only, constant retirement and heap cleanup remain. | 56 -> 59 |
| enum payloads, library namespaces, native enums, numeric operations, operator traits, Result/Option, interpolation, String methods, Try | 44 exact inputs plus canonical public Try input; focused rejection, admission, linking, binding and failure scenarios. Enum version/layout scenarios remain byte-identical. Native enums keep fresh source-unaware installed consumers and three runs on one heap. | 71 -> 81 |
| integer/float methods and all children, numeric check generator and sampling example | 24 integer and 28 float named outer rows; bounded generators, reference math, exact bits, NaN/zero sets, typed transport, patched pools and cleanup retained. NumericChecks, sampling, invalid-clamp lifecycle and real-backend/fallback aggregate retain their substantive ownership. Two fixed integer sources extracted. | 25 -> 59 |
| language-contract preparation/evaluation/types-dispatch | 11 substantial sources extracted; independent cancellation/return/initialization/alias/trap rows exposed; five mutation rows named. Four routes, actual-native obligations, attempted/committed effects and publication facts remain with the existing runner. Preparation and 73 type/dispatch cases unchanged. | 149 -> 159 |

The final selected discovery has 463 cases. The original 401 is the frozen
source-level scheduled-case inventory, not a pre-edit execution claim. The net
increase of 62 exposes independent programs/widths previously hidden in serial
functions, minus one proved duplicate: `user_print_is_direct_call` duplicated
`shadowed_print_has_no_host_effect` with only scalar input/output changes and no
lowering observation. The latter remains; no exact-name consumers existed. The
only numeric deletion is one repeated quiet-NaN/quiet-NaN atan2 predicate per width;
one identical predicate remains and unequal argument orders are unchanged. Removed
the language-contract runner's write-only reflection selector, not the reflection
scenario or any observed host effect.

The completed syntax discovery pilot is retained without changes: mixed public
examples keep their exclusions, metadata/routes, origin/trap/closure assertions and
two-consumer source-example runner. Remaining stateful/native providers and
numeric preparation/reference helpers retain real consumers or independent
responsibilities. No unclassified language owner or unexplained forwarding wrapper
remains. No dependency, integration target, production runtime or shared execution
API was changed. This does not implement future UI diagnostic suites.

Preservation review compares every extracted input against frozen literal values,
retained bodies/assertions and full generated predicate/outer-binding streams.
An independent read-only review also compared fixtures and exceptional paths
against the committed baseline. Legacy unencoded compiled-artifact execution is
not relabeled as checked-source. Result/Option still requires actual collection
on every lane; String/Try still retire constants before final heap observations.
Narrow path-specific Git whitespace attributes preserve inherited final blank or
unterminated indentation lines rather than trimming literal values.

Local behavior: 65 selected tests passed (types 12, collections 13, values 13,
numeric 11, language-contract 16). These cover ordinary routes and representative
rejection/admission, source-free linking, typed bits, actual-native, GC, cancellation,
trap/reuse and pinned-publication boundaries; they are not all 463 cases. Corrected
a too-long file-derived filter by using Try's `base_dir` and confirmed the intended
carrier case actually executes. The source-only build hook now watches discovery:
adding a temporary list fixture changed the selected list 6 -> 7, its one selected
case passed, and removal restored the exact six names without registration edits.
The probe is removed. Workspace Clippy, structure (1,583 Rust files, no violations
or exceptions), formatting and diff checks passed. Full affected suites and the
complete feature/backend/platform matrix remain CI-owned. No speed claim or
carried implementation error; TO03 source/parser/UI work is next.

2026-10-24 TO01: moved 15 conversion, 27 default-method and 17 standard-trait
programs to directory discovery. The 59 ordinary inputs match their frozen source
values byte-for-byte; all retained test bodies/assertions are unchanged. The shared
cursor uses its canonical CRLF input without a copy or new newline exception.
Three exact fixture paths permit their inherited final blank line in Git whitespace
checks; the source values are not trimmed to satisfy a formatting check.
Admission tests reuse ordinary comparison fixtures. Removed each now-unused local
`execute` wrapper. Rejection/linking/native-aggregation and standard-trait
admission/callback/hash boundaries have focused owners; generated arithmetic keeps
its two-route macro and independent reference/cleanup assertions. No test or
semantic input was deleted as duplicate in this phase.

Pre/post selected discovery lists both contain 102 cases (59 ordinary, 43 retained).
Nineteen focused behavioral cases passed across conversion identity/construction,
rejection/linking/overflow cleanup, defaults/cursor/tuple/capability/arithmetic and
standard collection/callback/hash/linking contracts. An add/run/remove fixture
probe independently appeared as case 103, passed its three routes, and disappeared
again without registration edits; the restored name set matches the 102-case list.
Static verification compares all inputs and retained bodies against pre-edit
snapshots. The build script watches the language fixture tree for discovery.
Workspace Clippy, structure, formatting and diff checks passed. These checks do not
claim execution of all 102 cases: exhaustive affected suites and the complete
feature/backend/platform matrix remain CI-owned. No performance result is claimed;
TO02 and subsequent phases are not complete. No carried implementation errors.

2026-10-24 TO00: implementation/phase commits authorized. Reconciled all 24 members,
eight integration targets and tracked non-Rust runner/product families. The static
role scan recorded 729 Rust source/support/example/rustdoc candidates and 68
non-Rust runner/product/corpus candidates; these are deliberately **not test-case
counts**. Reviewed family dispositions are retained above, independently of the
disposable machine inventory. The selected 102-case TO01 harness listing succeeded;
no baseline behavior execution or global workspace harness sweep was performed.
Document links, JSON example and diff checks complete the documentation checkpoint.
TO01 starts from the committed code baseline, with no carried implementation errors.

2026-10-24 planning history: inspected current source/test/support patterns across frontend,
execution/runtime and tooling ecosystems at `85662b70`; confirmed the 24-member,
eight-integration-target metadata inventory. The examples in this plan distinguish
confirmed duplication from candidates and retained boundaries. No implementation,
new dependency, build, test execution or performance measurement was performed for
this planning task. No baseline behavioral failure was reproduced; historical
review/CI findings remain at their existing owners, not newly passed or fixed here.

2026-10-24 UI follow-up: added an explicit file-based diagnostic contract and
TO00/TO03/TO04/TO09 acceptance steps after user confirmation. Inspected the existing
source presentation records and CLI reporting owner to avoid a second diagnostic
implementation. This remains design only; no expectation reader, updater, UI
fixture suite, dependency change or production behavior has been implemented.
