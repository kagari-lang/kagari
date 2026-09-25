# Foundation Refactor

This track supersedes conflicting milestone behavior in implementation-roadmap.md.
It is a breaking replacement: no old API facade, dual semantic path, artifact
upgrade, or legacy interpreter. Runtime ABI/schema/authority checks remain required.

## Checkpoints

Each checkpoint requires implementation, focused verification, updated examples
and specifications, and a Conventional Commit with `Roadmap-Step: Rxx`.
Unchecked entries are not implemented claims. Full LSP, async, advanced GC,
complete replay, automatic state migration, and advanced JIT are later tracks.

- [x] R01: Authoritative value, failure, and activation contracts.
- [x] R02: Source/result/diagnostic/host-effect conformance harness.
- [x] R03: Unified source database, revisions, identities, overlays, coordinates.
- [x] R04: Recoverable HIR analysis; checked-only code generation.
- [x] R05: Immutable queries, cancellation, parse/body reuse and invalidation.
- [x] R06: Offline host declarations and checked runtime bindings.
- [x] R07: Nominal concrete identity, layouts, bounded reachable monomorphization.
- [ ] R08: Verified IR and linked-only runtime operands.
- [x] R09: Canonical bounded artifact format and explicit fingerprint algorithm.
- [ ] R10: Shared immutable generations and runtime-local state.
- [ ] R11: Value semantics, owned handles/roots, nonmoving mark-sweep baseline.
- [x] R12: Execution sessions, synchronous host reentry, shared cleanup/budgets.
- [ ] R13: Failure-atomic standard mutation and dirty-record commit.
- [ ] R14: Acyclic initialization and isolated prepare/initialize/publish.
- [ ] R15: Compile-time capability and resource limits.
- [x] R16: Injectable deterministic context and host trace fixtures.
- [ ] R17: Interpreter/JIT/debugger contract equivalence.
- [ ] R18: Obsolete-path audit, full validation, reproducible resource baselines.

## Validation

Focused tests accompany each semantic change. Final gates are `cargo fmt --all
-- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test
--workspace`, and `git diff --check`. Performance evidence records toolchain,
workload, repetitions and measurements; no unmeasured performance claims.

## Current implementation status

- R08 bounded trait-proof checkpoint: source analysis and executable validation
  can use the same implementation matcher. The shared query preserves generic
  substitution and recursive trait bounds while exposing cancellation and
  explicit candidate/depth limits for untrusted artifact checks. Reaching a
  limit is a verification failure, not evidence that a constraint is absent.

- R08 host-bound proof checkpoint: standalone modules and dependency-closed
  programs recheck each concrete host trait argument against script interface
  tables and declared host trait tables. Applied trait arguments must have one
  matching implementation; missing or ambiguous evidence rejects the product
  before execution. Script and host evidence survive artifact encoding, while
  a changed argument without a corresponding implementation is rejected.

- R08 conservative-root checkpoint: typed IR identifies heap-valued locals
  and temporaries, and KBC format 34/runtime ABI v34 carry their local and
  register slots. Verification requires the exact function-wide conservative
  set before execution; encoded artifacts with missing or extra roots are
  rejected. Interpreter frames still root all slots, while the existing JIT
  retains its precise-stack-map requirement for supported functions.

- R08 interface-object ownership checkpoint: the integer placeholder is gone.
  A runtime interface value is a generation-checked GC object containing its
  concrete payload, applied trait identity, linked method slots and retained
  executable dependency version. Construction checks the verified table and
  concrete receiver; tracing keeps the payload alive, and collecting the last
  interface releases version retention. This entry currently accepts concrete
  non-generic script tables. Generic table instantiation, source coercion,
  host-backed interface values and source-level dynamic calls remain to be
  connected. The VM embedding entry can invoke a linked interface method
  against the receiver's pinned version after validating and rooting arguments.

- R08 interface-call ABI checkpoint: boxed methods retain their verified
  parameter and result types. Embedding calls check the full script nominal
  signature before entering a frame and check the result on exit; a Struct with
  the same coarse heap representation but a different declaration is rejected.
  Exact host-root arguments use the linked host type identity. The invocation
  roots all arguments across initialization and execution, including nested
  script objects in tuples. Linked dependency layouts participate in nominal
  matching instead of limiting checks to the defining module.

- R08 interface-allocation instruction checkpoint: typed IR names an
  implementation declaration; lowering resolves it to a version-local table
  slot. KBC format 35/runtime ABI v35 encode `MakeInterface`, and both IR and
  bytecode verification require a concrete non-generic table, a heap result and
  a receiver with the table's physical representation. Runtime construction
  checks the full concrete receiver ABI and retains the linked program. An
  artifact round trip executes the instruction; invalid slots and receiver
  representations fail before execution. In-frame dynamic dispatch still
  needs call target wiring.

- R08 dependency-table linking checkpoint: `MakeInterface` now carries both a
  dependency-program module slot and an implementation-table slot. KBC format
  36/runtime ABI v36 reject the former encoding. Program verification checks
  the referenced table in its defining module and proves that module is in the
  caller's dependency closure; the VM constructs the object from that pinned
  member. A detached dependency reference is rejected before execution.

- R08 source-coercion checkpoint: HIR records the unique concrete implementation
  declaration when an expression is used where an interface is expected. IR
  emits `MakeInterface` from that fact, including for dependency tables, and
  whole-program verification rechecks the referenced table and dependency
  reachability. Source execution tests cover empty and method-bearing tables;
  source-level dynamic method calls still require a dedicated call instruction.

- R08 method-slot checkpoint: interface objects store method bindings in trait
  declaration order even when an implementation declares methods in another
  order. The runtime resolves a verified ordinal only after checking the
  object's applied interface identity; missing slots and mismatched interfaces
  trap. The embedding identity lookup remains available, while the forthcoming
  script call instruction can use the ordinal without searching method names.

- R10 pinned-frame checkpoint: every interpreter frame owns a loaded member of
  its executable program. Ordinary descendant calls resolve module slots from
  the current frame; a rooted interface method can enter a frame from its
  retained older program with argument validation and return ABI checking.
  A reload regression test proves its descendant uses that older program and
  a failed argument check leaves the frame stack intact. Source-level interface
  call lowering and the remaining cross-version lifecycle audit are still open.

- R03 lexer-boundary checkpoint: unknown Unicode scalars now retain full UTF-8
  byte ranges. Parsing unsupported Chinese or emoji tokens produces diagnostics
  and lossless CST tokens instead of slicing inside a code point and panicking.

- R04 member-origin checkpoint: lowering stores the exact CST name range for
  field reads and writes in the HIR source map. Definition, host-field and host
  call navigation consume that range directly; the source-text suffix heuristic
  is removed. Tests cover member access with trailing Unicode comments, including
  a write target.

- R04 qualified-reference checkpoint: HIR also stores the terminal CST name
  range for path expressions. Source, host and local declaration queries select
  that name instead of treating an entire qualified call as one target. A
  qualifier and its `::` separators no longer navigate to the final function
  or enum variant. The same range is used when following source imports across
  files. Tests include source and host calls after Unicode/CRLF text; existing
  constructor/query examples now select the referenced member name.

- R04 initializer-target checkpoint: struct construction records the CST ranges
  of its type name and each field label. Definition queries use the checked
  aggregate and per-field identities, including known labels beside invalid
  values or unknown labels. A label's `:` and value expression do not inherit
  its target. The source_queries example demonstrates this under a rejected
  assignment.

- R04 enum-owner checkpoint: qualified enum constructors retain separate CST
  ranges for the enum owner and variant. Definition queries use the checked
  constructor's nominal owner on `Event` and its variant target on `Ready`;
  `::` stays targetless. Known enum owners remain navigable beside unknown
  variants and through source facades.

- R04 type-name checkpoint: type lowering records a separate terminal-name
  range for annotations, bounds, where targets and explicit constructor type
  arguments. Definition and offline host-type queries use that name range;
  generic delimiters, array brackets and qualified separators have no type
  declaration target. The complete annotation span still supports type queries.
  Tests include nested applications, source facades, host types and CRLF/Unicode.
  The source_queries example checks both sides of this range distinction.

- R04 declaration-origin checkpoint: named module, function, method, const,
  struct, enum and trait declarations now expose their identifier token as the
  target location. The full item range remains in the HIR source map for
  diagnostics and lowering. Name ranges trim CST trivia, including leading
  spaces; source_modules verifies an imported target lands on `Data` itself.
  Parameter, local, field and variant name ranges use the same token boundary.

- R04 declaration-site checkpoint: `definition_at` resolves a declaration's
  own identifier for named items, parameters, generic parameters and local
  bindings. The semantic table explicitly records navigable sites so the
  synthetic module initializer and missing names cannot claim surrounding
  source. Tests cover declaration and reference queries beside CRLF and Unicode;
  source_queries exercises function and local declaration sites.

- R04 incomplete-member checkpoint: declaration-only member queries now use
  the same explicit site set. A field or variant with a missing identifier
  retains its recovery fact but cannot make the surrounding type annotation or
  payload a navigation target; valid neighbors remain available.

- R04 signature-navigation checkpoint: signature queries now resolve named
  declaration sites and checked type-reference targets before body analysis.
  Full and signature queries share one type-target selector, including the
  innermost-annotation rule for unresolved nested types. Tests cover generic
  binders, imported facades, errors, Unicode/CRLF and agreement with full
  analysis; source_queries navigates an enum payload type from signatures.

- R04/R06 offline signature-host checkpoint: signature queries now expose
  checked host type declarations from offline interface data without checking
  bodies. Full analysis and signatures share one annotation target selector;
  punctuation and unknown nested names cannot borrow a host target. The
  offline_compile example and embedding test require no runtime registration.

- R06 path-fingerprint input checkpoint: runtime index and virtual path
  registrations no longer accept arbitrary member fingerprints. The shared
  canonical whole-path encoder derives identity from their resolved inputs;
  only declared fields contribute a member fingerprint. Tests compare equal
  contracts and changed permissions/names. Artifact format/runtime ABI v25
  rejects prior products. Complete portable index/virtual paths remain open.

- R06 portable-segment checkpoint: index and virtual registrations now carry
  common, serializable segment declarations with `HostValueType` contracts
  instead of runtime type slots. Registration checks and resolves every type
  before publishing a descriptor; unrelated runtime slot shifts preserve the
  canonical path fingerprint. VM dynamic-argument and runtime tests exercise
  the new API. Source host index syntax remains for later R06 work.

- R06 complete-path declaration checkpoint: KHI v7 replaces field-only records
  with ordered field/index/virtual `HostPathDeclaration` segments. Runtime
  registration exports every path, verifies that its portable and resolved
  fingerprints agree, and linking requires a unique matching binding. The old
  field-path data type and registration entry are removed. KBC format/runtime
  ABI v26 reject earlier products. Offline roundtrip and runtime rebinding
  tests cover mixed paths; source host index syntax remains open.

- R06 source-index checkpoint: a declared single root index is now selected in
  HIR for reads and assignment targets. IR passes the captured index as a typed
  dynamic path argument; compound assignment captures the root and index before
  its RHS. Source, encoded artifact, interpreter and existing JIT fallback tests
  agree on call and modification order. Primitive runtime type registration also
  exposes its portable host type contract, so a path need not rely on an unrelated
  host member to bind its scalar index or result. Field/index chains and virtual
  source members remain open. `offline_compile` also emits a declared source
  index read without starting a runtime.

- R06 field-index checkpoint: source reads and assignment targets now select a
  complete nominal field chain followed by one declared index. HIR retains field
  identity and type facts without requiring a separate intermediate field path;
  IR evaluates the host root and dynamic index once and uses one linked path
  operation. Source, artifact and JIT fallback tests verify no intermediate read
  and identical modification order; `offline_compile` exercises offline binding.
  Paths with multiple indexes, fields after an index and virtual source members
  remain open.

- R06 mixed-path checkpoint: HIR selects one declared path for an ordered source
  suffix of fields, indexes and virtual members, including repeated indexes and
  fields after indexes. Each dynamic index is evaluated once in source order,
  then passed by its declared slot to one typed IR path operation; write targets
  capture the root and indexes before the RHS. IR includes nominal types reached
  only through index or virtual path results in the required host interface.
  Source, encoded artifact and existing JIT fallback tests exercise a two-index,
  virtual-member and trailing-field compound assignment. `offline_compile`
  compiles that path using declarations alone. Source syntax requires each index
  step to have a distinct dynamic slot; repeated-slot declarations remain valid
  for runtime adapters but do not map to independent source expressions.

- R06 acceptance: one portable KHI declaration provides nominal type/member
  identities, signatures, access, passing/effects, documentation and canonical
  fingerprints for offline analysis and runtime registration. Compilation and
  queries do not register callbacks or start a runtime; source, artifact and JIT
  fallback execution link required bindings by identity and contract before
  publication. Host function and method boundaries validate opaque nominal roots,
  including nested values. The host trait implementation table remains R07 work.

- R16 acceptance: optional per-root trace records the verified dependency
  closure fingerprint, root identity, explicit time/seed inputs and ordered host
  invocations with bounded argument/result snapshots and outcome categories.
  Invocation slots are reserved before callbacks, so synchronous reentry keeps
  call order. The trace reports dropped calls after its 10,000-call cap. The
  runtime session exposes it directly; successful embedding reports include it
  when `tracing_enabled` is set. Replaceable host callbacks provide external
  results. Tests compare source-built and decoded artifacts under identical
  inputs and results, plus nested calls, host failures and trace limits. Opaque
  runtime handles are diagnostic identities, not replay data; full replay and
  cross-platform floating-point bit identity remain later work.

- R16 execution-input checkpoint: `ExecutionContext` supplies fixed logical time
  and a random seed to the root session. Host callbacks read the time and a
  deterministic SplitMix64 stream through `HostCallContext`; synchronous reentry
  consumes the same stream, and the next root resets it. Equal inputs produce
  equal observed host results in tests. The trace and replaceable-host acceptance
  are recorded above.

- R15 diagnostic-output checkpoint: full semantic analysis now emits at most a
  configurable number of semantic diagnostics per file (default 1,000), followed
  by one structured limit diagnostic if exceeded. Changing the limit invalidates
  cached full results without mutating older snapshots; checked compilation still
  rejects limited results. This bounds output, while earlier diagnostic generation
  and downstream recursive traversal limits remain in the R15 audit.

- R09 direct-load parity checkpoint: shared verified code construction and direct
  reload preflight now apply artifact count, identity, nested-record and encoded
  size limits before fingerprinting or linking an in-memory program. Oversized
  programs return a resource-limit error without publishing a version.

- R10 shared-code checkpoint: `VerifiedProgram` verifies a `BytecodeProgram` once
  and owns immutable, reference-counted module code. Multiple runtimes can link
  that code without copying functions or layouts; each creates its own host slots,
  module instances, epoch, permissions, heap and execution cache. Loaded handles
  reject cross-runtime use even when module keys coincide. Full capture/interface
  ownership and version reclamation remain in the R10 audit.

- R10 cache lifecycle checkpoint: reload removes invalidated interpreter/JIT
  records from the runtime-local registry instead of retaining tombstones and
  their executable metadata. Epoch retention is released with the removed JIT
  records; active-call retention remains independent. Tests inspect the registry
  after reload as well as public artifact lookup.

- R09 acceptance: format 24 fixes field order and little-endian integer encoding;
  FNV-1a-64 fingerprints use a versioned domain instead of Rust Debug output.
  Language, format, runtime/helper ABI, host-interface and dependency identities
  remain separate. Construction and loading enforce the 64 MiB budget, collection
  counts, nested operand/type depth and identity path limits before publication.
  Loader validation checks version, checksum, bytecode, derived tables and binding
  requirements before runtime publication. Conformance tests prove source and
  decoded-artifact module identity agreement, old-format rejection and malformed
  payload refusal; the offline example round-trips and validates the artifact.

- R09 encoded-size checkpoint: construction checks canonical encoded program
  and build-metadata size before verification, then checks the assembled artifact
  before content hashing. In-memory loading and encoding repeat the 64 MiB
  bound. Oversized source names and mutated header strings are rejected before
  fingerprinting; byte decoding already checks input length before parsing.

- R09 instruction-operand checkpoint: calls, tuple/array/nominal constructors
  and typed path instructions preflight their register vectors at 4,096 elements.
  The complete program permits at most 1,000,000 embedded operand records.
  Construction, in-memory loading, encoding and byte decoding reject oversized
  operands before execution.

- R09 in-memory identity checkpoint: KBC construction, in-memory loading and
  encoding now reject overlong module, host, aggregate, header and public ABI
  identity paths before fingerprinting. Host declarations reject the same shape
  before KHI encoding or binding. Tests cover source-owned, host-owned and
  mutated header identities. Encoded-size checks now bound scalar/string data.

- R09 nested metadata parity checkpoint: function-table parameter layouts and
  debug frame parameter/local/register vectors now receive the same in-memory
  record limit as their decoder preflight. Construction, mutated-artifact load,
  encoding and byte decoding reject oversized records; detached debug metadata
  is checked before fingerprinting.

- R09 identity-path decoding checkpoint: shared module and declaration identities
  reject encoded paths longer than 64 segments before reading elements. Forged
  identity lengths fail in direct decoding and complete KBC headers. Host and
  identity guards share one common sequence preflight implementation. In-memory
  identity limits are checked separately; scalar/string resource audit remains.

- R09 host declaration decoding checkpoint: standalone KHI and embedded KBC
  host interfaces now reject oversized declaration/member/path vectors during
  decoding. In-memory validation enforces the same per-vector limits. Forged
  short KHI headers and oversized path payloads fail before linking. Identity
  path decoding is now bounded separately; scalar/string allocation audit remains.

- R09 metadata decoding checkpoint: function/debug metadata, artifact tables,
  verification summaries and signature lists now check encoded vector lengths
  before deserializing elements. A forged debug source-name count is rejected
  before reading names. Host interface and identity path decoding are now
  bounded separately.

- R09 executable-sequence decoding checkpoint: module, function, instruction,
  module-owned table, concrete layout and public ABI member vector lengths are
  rejected from the encoded sequence header before element deserialization.
  Tiny forged-length inputs fail at that boundary; aggregate post-decode limits
  remain. Host interface and metadata decoding are covered by later checkpoints.

- R09 ABI type wire checkpoint: format 24 replaces recursive ABI type encoding
  with flat preorder nodes. At most 4,096 nodes and depth 64 are accepted before
  reconstructing the type; previous artifacts are rejected. Broader decoder
  allocation audit remains open.

- R09 nested-record checkpoint: executable struct/enum layouts, host declarations
  and paths, and public ABI declaration members now have 4,096-record per-vector
  and 1,000,000-record program-wide limits. Construction, memory loading,
  encoding and byte decoding reject oversized nested collections before
  verification. Later checkpoints bound ABI type nodes and remaining vectors.

- R09 artifact count-limit checkpoint: `.kbc` input remains capped at 64 MiB;
  construction, decoding, encoding and in-memory loading now also bound modules,
  functions, instructions, module/metadata vectors and declared section counts.
  Small crafted payloads with huge counts are rejected before verification or
  execution. The offline_compile example round-trips a valid bounded artifact.
  The later nested-record and decoder checkpoints close the table-allocation audit.

- R09 artifact-directory checkpoint: generation and loader validation derive
  section records and source/debug name tables from the same program and optional
  metadata. A recomputed outer hash cannot hide stale counts, altered fingerprints,
  missing/reordered sections or forged source names. The offline_compile example
  checks the function section against executable records. Later R09 checkpoints
  add count, depth and encoded-size bounds.

- R08 named-entry ambiguity checkpoint: VM entry selection rejects multiple
  matching bytecode names before initialization instead of choosing the first.
  The embedding layer classifies this as a bytecode verification failure.
  Source/encoded and interpreter/JIT fixtures retain uninitialized instances,
  zero instruction steps and no roots. Internal calls remain slot based.

- R08/R14 entry preflight checkpoint: interpreter and existing JIT routes resolve
  a named entry after bytecode validation and before dependency initialization.
  A missing entry leaves module instances uninitialized and executes no script
  instructions. Source and encoded artifact tests cover both backend routes;
  scoped_execution demonstrates the embedding behavior.

- R08 unsupported-call verification checkpoint: bytecode verification now rejects
  register callees and the unimplemented dynamic invocation helper before a
  module is published. IR already rejected both forms; the VM's separate
  register-call scan is removed. Direct bytecode loading and encoded-artifact
  tests cover both forms, including a runtime with the dynamic-invocation
  capability enabled, and assert that failure leaves the module registry empty.
  Future dynamic calls require an explicit verified call contract rather than a
  late VM error.

- R08 reflection-helper contract checkpoint: IR and bytecode now share one
  helper-call contract. Both verify argument count, `type_of` result layout,
  field/index receiver representation, integer index representation, writable
  payload eligibility and write-result layout before bytecode publication.
  Bytecode tampering tests reject malformed helper calls while existing source,
  artifact and VM reflection fixtures retain valid behavior. Dynamic member
  existence and capability decisions remain runtime checks.

- R08 public host-trait verification checkpoint: IR and bytecode verification
  recheck every host trait table whose defining script trait has a public ABI
  record in the same module or dependency closure. The check compares applied
  argument count, complete method roster, receiver, nested parameter and result
  types, and rejects mismatches before publication. Encoded-artifact tampering
  tests cover changed arguments, results and method mappings; a cross-module
  fixture checks the dependency owner. Private trait contracts and applied
  trait-parameter bounds are covered by later R08 checkpoints below.

- R08 private trait-contract checkpoint: KBC format 33/runtime ABI v33 carry
  bounded executable contracts for private traits only; public traits continue
  to use their single public ABI record. IR and bytecode reject duplicate,
  foreign or malformed private contracts, public/private name collisions and
  missing or mismatched host trait mappings before publication. The private
  `Readable` example runs through source, encoded artifact, interpreter and
  existing JIT fallback; tampered private signatures and omitted contracts fail
  artifact validation. Trait-parameter bounds are covered by the following
  checkpoints.

- R08 host standard-bound checkpoint: source analysis and executable host-trait
  verification now call the same standard-constraint predicate for portable
  host type arguments. A decoded private `Readable<T: HashKey>` artifact rejects
  a forged `Readable<f32>` binding even when its host method signature is
  changed consistently. Applied trait bounds that require another trait
  implementation are covered by the host-bound proof checkpoint above.

- R08 private implementation-table checkpoint: executable interface tables for
  locally defined private traits are now compared with the private trait
  contract before method slots can serve as bound evidence. The verifier rejects
  changed result types, missing method rosters and absent trait declarations;
  public tables continue to use their public ABI record. Applied trait-bound
  proof across the dependency closure is covered by the host-bound proof checkpoint.

- R08 host-type closure checkpoint: whole-program bytecode verification now
  rejects conflicting declarations for one host type identity or symbol across
  modules before linking. Equivalent declarations with different documentation
  remain the same executable contract. A two-module regression checks both
  cases; this makes the host catalog used by later trait-bound proofs unique.

- R04/R06 call-navigation checkpoint: offline `host_function_at` and
  `source_function_at` queries restrict dotted callees to their member names.
  A receiver call retains its separate checked target; dot positions no longer
  report the outer method. The offline_compile example covers host methods.

- R04 source-member navigation checkpoint: `definition_at` now scopes checked
  field and method targets to their source names for reads, writes and calls.
  Receiver bindings remain queryable and the dot has no declaration target.
  The source_queries example and focused regression cover this behavior.

- R04 host-field navigation checkpoint: `host_field_at` selects a checked member
  only on its source name, not on the receiver or dot in the enclosing expression.
  Read/write, mixed source/host paths, recovery after call errors and offline
  compilation share this query rule. Host path diagnostics still reject codegen.

- R04 assignment diagnostic ownership checkpoint: error descriptions now read
  checked place facts instead of rerunning receiver inference. Host-target probes
  reuse known readable-place types. Nested invalid indexes are diagnosed once per
  source occurrence, including read-only and unknown-field assignment targets;
  original index and write errors continue to reject code generation.

- R04 indexed-write recovery checkpoint: invalid array indexes preserve target
  types for contextual RHS inference and retain field targets through indexed
  receiver places. Index validity remains mandatory for checked code generation.
  Tests cover direct and projected writes with boolean, unresolved and missing
  indexes, phantom generic constructors, neighboring functions and source queries.

- R04 array-index recovery checkpoint: invalid, unresolved and missing array
  indexes retain the known element type for downstream field navigation and
  receiver queries. Index diagnostics still reject code generation; tuple member
  selection still requires a valid constant index. The source_queries example
  demonstrates navigation after an invalid boolean array index.

- R04 write-member receiver checkpoint: field-write places now participate in the
  same protocol-independent receiver query as field reads. Tests cover valid nested
  writes, unknown fields and incomplete trailing-dot targets, exact snapshot reuse,
  source movement across emoji/CRLF, old snapshot stability and full-file/body-query
  agreement. Invalid sources still reject codegen. The source_queries example
  demonstrates a field-write receiver. Edited erroneous bodies may be rechecked;
  these tests do not claim cross-revision reuse for them.

- R04 read-only target facts checkpoint: parameter/val/field/tuple write rejection
  preserves known place types and supplies RHS contextual inference independently
  of write permission. Position type queries now include semantic place spans.
  Four regressions retain concrete generic initializer types and target queries
  with only the original write diagnostic; codegen stays rejected and neighboring
  functions remain queryable. The source_queries example shows parameter recovery.

- R04 assignment-index recovery checkpoint: unresolved receiver/field projections
  no longer skip independent index expression analysis in readable or writable
  places. Four recovery cases retain index member types/navigation, diagnose an
  independent call-arity error once, preserve a correct neighboring function and
  reject code generation. The source_queries example exercises the same tooling
  boundary. Broader semantic ownership/execution audits remain unchecked.

R02 acceptance evidence:

- [Unified language contracts](../crates/kagari-vm/src/tests/language_contract.rs)
  provide source/dependency input, expected values/diagnostics/traps, ordered host
  calls, committed mutation records and rooted post-failure heap observations.
- [The acceptance matrix](spec/language-conformance.md) maps value, failure,
  initialization and activation rules to positive/negative fixtures. All executable
  cases use source/artifact × interpreter/JIT, with recorded native invocation for
  selected scalar cases; container and module calls retain existing fallback.
- Publication fixtures keep a root session pinned while publishing new dependency
  code, observe old/new results 42/99, reject a fully initialized stale candidate,
  release its module resources and verify that the chosen entry remains unchanged.
- Focused subsystem tests are retained. Accepting this harness does not complete
  R10/R11 ownership, R13 mutation or R14 activation audits; source callbacks and
  generalized heap write tracing are not claimed as implemented.


- R14 dependency-aware staging checkpoint (R02 fixtures): code-free candidates
  are completed during Prepare only after their dependencies are initialized.
  Previously a code-free root could already be marked Initialized when dependency
  failure occurred, masking the original error during cleanup. Shared source-based
  fixtures now cover forbidden root/dependency host effects and initializer traps
  on all four routes, preserving error category, current entry, loaded-module
  count and old-entry results, with no forbidden host call or mutation. The same
  compiler helper builds old and candidate programs without modifying bytecode.

- R02 multi-module checkpoint: every fixture now uses one source database,
  immutable snapshot and checked-program compiler path. Optional dependency sources
  share the fixture value/diagnostic/call/mutation expectations; artifact routes
  serialize the entire program. Diamond initialization order, one-time initialization,
  cached dependency failure preventing root effects, and cycle rejection now run
  through the common four-route entry. Focused activation tests remain valuable,
  and publication isolation/stale candidates/old dependency closures now also
  have shared source-based fixtures, as recorded in the R02 acceptance evidence.

- R02 host-owned iteration checkpoint: five source fixtures exercise array push,
  insert, pop, remove and clear under a host collection guard on all four routes.
  They verify standard trap context, preserved earlier replacement/host effects,
  released execution roots, retained contents after collection, and restored
  structural access after guard release. This does not implement source callbacks
  or for loops; the source type system still lacks function values and the VM
  standard callback path remains unconnected.

- R02 interrupted compound-write checkpoint: rooted-array fixtures now cover host
  rejection, callback-triggered cancellation and RHS instruction-budget exhaustion
  across source/artifact × interpreter/JIT. Previously committed heap and host
  changes survive, rejected host calls produce no host mutation record, final
  assignment is absent, call depth returns to zero and collection preserves the
  explicitly rooted result. Cancellation is represented separately from script
  traps and resource exhaustion in the shared fixture format.

- R02 post-trap heap checkpoint: shared fixtures can declare an offline array
  provider, bind an explicit rooted array per runtime, and compare contents after
  execution and collection. They assert frame roots are released and record provider
  calls. Four source/artifact × interpreter/JIT fixtures verify prior writes survive
  overflow/bounds traps, RHS deletion prevents final write, and compound assignment
  reads the RHS-updated element. Other object observers and heap write-event traces
  remain outstanding; array final-state evidence is not a general mutation log.

- R02 artifact/JIT checkpoint: all shared contract fixtures now run the full
  source/artifact by interpreter/JIT matrix. The artifact/JIT route decodes and
  validates serialized bytes before loading, and applies the same host calls,
  committed mutations, traps, cleanup and repeat expectations. Native-required
  scalar fixtures assert actual backend invocation on both JIT routes. This adds
  artifact/backend equivalence evidence; outstanding heap observations and
  activation implementation audits remain assigned to their runtime checkpoints.

- R15 public-layout verification checkpoint: template matching now builds an index
  once per validation call and checks cancellation while indexing and traversing
  layouts, fields, variants and payload members. Cancellation remains distinct from
  layout mismatch and is honored for empty inputs. Enum matching compares payloads
  individually without cloning whole payload vectors. Tests cover 256 public
  templates, reachable instances and cancelled verification; the layouts example
  demonstrates the structured cancellation result. Artifact verification remains
  synchronous; analysis uses its caller-provided cancellation token.

- R07 unused public-template checkpoint: the shared ABI validator rejects empty
  aggregate/member names, duplicate aggregate/member names and mixed struct/enum
  members before layout matching, even without an executable instance. Ten malformed
  template cases exercise both IR and bytecode rejection. Redundant enum binder and
  aggregate-shape checks were removed from layout matching; the ABI validator owns
  these facts. The layouts example includes an unused generic public declaration.

- R07 executable identity checkpoint: shared IR/bytecode layout validation rejects
  noncanonical aggregate paths and nonzero declaration/member occurrences. Six
  tampering regressions cover struct/enum parents, fields and variants while keeping
  their relative owner paths consistent. HIR recovery identities remain available
  for erroneous source; executable layouts use only unique top-level declarations.
  The layouts example asserts these ownership paths.

- R07 concrete struct-field checkpoint: layouts now retain `AbiType`, including
  nominal identities and nested container/tuple types; lowering substitutes generic
  arguments before encoding. IR/bytecode derive operand representations from that
  single type, reject unresolved field parameters and absent nominal layouts, and
  include field host references in interface validation. Allocation and replacement
  share the concrete runtime check before allocation accounting or target writes.
  Runtime regressions cover wrong nominal/tuple members and unchanged targets;
  embedding tests cover valid generic fields through source, artifacts and JIT
  fallback. The `layouts` example shows a concrete array field. Format 23/runtime
  ABI v24 reject older products. Nominal operand-flow verification and dynamic interface fields remain audit work.
- R07 public struct-template checkpoint: IR and bytecode check concrete field
  types, order, names and permissions against substituted public templates. Program
  validation also checks imported instances when the owner has no executable
  instance. Tests independently reject wrong array element types with the same
  representation, changed permissions, renamed and missing fields in local and
  imported layouts. The public `Pair` in the layouts example exercises this check.

- R07 interface-identity checkpoint: HIR retains each anonymous impl's
  declaration identity, and the public interface-table ABI carries it separately
  from its diagnostic display label. IR/bytecode validation requires a unique,
  local impl identity, verifies method binder ownership against that impl, and
  checks complete unique method rosters and signatures for local public traits.
  Signature comparison substitutes trait `Self`, trait arguments and method
  binders through nested ABI types, and checks parameter mutability and bounds.
  Public ABI reload keys use the canonical encoded identity, so equal display
  labels in different packages cannot alias. KBC format/runtime ABI v27 reject
  older products. The layouts example prints the checked table; executable
  method tables and generic impl specialization are covered by later R07 entries.

- R07 executable-function identity checkpoint: bytecode functions and their
  function records now retain concrete declaration identity plus type arguments
  from verified IR. Bytecode verification rejects foreign, unresolved, duplicated
  or mismatched identities before execution; artifact limits bound their encoded
  type arguments. Hand-authored functions without a source declaration use an
  explicit absent identity. KBC format/runtime ABI v28 reject older products.
  The layouts example checks the function records. A later R07 entry links
  interface method slots to these executable identities.

- R07 executable interface-table checkpoint: bytecode now carries verified
  implementation tables with trait method identities and concrete function
  slots. Non-generic implementation methods are emitted even when uncalled, so
  concrete public tables have complete method rosters. Verification rejects
  missing, foreign or mismatched slots before loading, and artifact limits bound
  the records. KBC format/runtime ABI v29 reject older products. The layouts
  example checks the executable slot. Generic implementation specialization and
  runtime interface dispatch remain open.

- R07 generic interface-implementation checkpoint: trait implementation lookup
  matches a receiver against its impl type template, preserves repeated binder
  equality, checks impl parameter bounds, and passes the inferred arguments to
  reachable method monomorphization. A generic implementation and another
  implementation for the same trait and receiver head are rejected as
  overlapping; this conservative coherence rule avoids order-dependent lookup.
  Method ABI records carry only method-local binders and bounds, while the table
  owns inherited impl binders and bounds. Bytecode verification checks each
  executable method slot's concrete argument arity. The language contract tests
  exercise source, artifact, interpreter and existing JIT routes. Runtime
  interface values and dispatch remain R07/R08 work.

- R07 applied-trait implementation checkpoint: local impl headers now resolve
  applied generic trait arguments instead of discarding them. Method-signature
  checking substitutes the trait's arguments and Self into each implementation
  contract; interface ABI tables retain the applied trait identity and round-trip
  through KBC. The `applied_traits` example inspects the verified method slot and
  encoded artifact. Invalid arity, unknown arguments, unsatisfied trait-parameter
  bounds and signature mismatches reject compilation. Applied
  generic bounds, imported trait implementations and dynamic interface values
  remain open.

- R07 applied-bound identity checkpoint: HIR constraints, selected trait-call
  targets and public ABI now retain the complete nominal trait application.
  Bounds in inline and `where` form distinguish `Echo<i32>` from `Echo<String>`;
  concrete and template implementations specialize by both receiver and trait
  arguments. Multiple disjoint applications on one receiver select distinct
  methods, while overlapping templates are rejected. KBC format/runtime ABI v30
  reject older products. The `applied_traits` example compiles a bound call and
  round-trips its artifact. Imported trait constraints/impls and runtime interface
  values remain open.

- R07 instantiated interface-slot checkpoint: the bytecode verifier now
  substitutes both impl and method type arguments into each declared method
  signature and checks the executable function's parameter and return layouts.
  A forged concrete function identity with the right arity but wrong argument
  representation is rejected before loading. Trait implementation signature
  checks compare method-local generic binders by position, so equivalent binders
  with different names match and different binder counts are rejected. Runtime
  interface dispatch remains separate work.

- R07 applied method-bound checkpoint: interface ABI validation now compares
  method bounds after substituting trait arguments, `Self`, and method-local
  binders. Equivalent applied bounds with different binder declarations pass;
  changed nested type arguments fail. A source-to-bytecode regression covers
  a method bound that refers to an applied trait parameter.

- R07 private method-bound checkpoint: HIR checks trait impl method-local bounds
  with the same trait-argument and binder substitution used for method signatures.
  This catches mismatched bounds on private traits before code generation, even
  when no public interface ABI record exists. Tests cover equivalent and changed
  applied arguments.

- R07 applied interface-type checkpoint: interface compatibility distinguishes
  a trait's inherited type parameters from method-local generic parameters.
  `Echo<i32>` may appear in an interface annotation when its methods are
  otherwise interface-compatible; a generic method still rejects that use.
  Runtime interface construction and dispatch remain outstanding.

- R07 imported-trait implementation checkpoint: imported trait declarations now
  retain stable method identities during signature checking. Local implementations
  of imported applied traits use the shared checked trait catalog to validate
  method roster, signatures, method bounds and trait-argument bounds. Bound calls
  link to the local concrete method slot. Whole-program bytecode verification
  matches an imported interface table against the dependency's public trait ABI
  and rejects a changed contract before execution. The
  [two-module example](../examples/imported-traits/main.kgr) runs from source and
  encoded artifacts through the interpreter and existing JIT fallback. Importing
  an implementation from another module and runtime interface values remain open.

- R07 dependency-defined implementation checkpoint: the aggregate catalog now
  carries checked implementation identities, receiver and trait applications,
  bounds and trait-method-to-function identities across a module's dependency
  closure. A concrete bound call can select an implementation declared in a
  dependency and link its method by declaration and checked signature to that
  dependency's function slot. Two visible concrete matches reject the bound
  instead of choosing by traversal order. The
  [three-module example](../examples/imported-traits/consumer.kgr) runs from
  source and encoded artifacts through the interpreter and existing JIT fallback.
  Generic dependency implementations, cross-module overlap coherence outside
  bound calls, and runtime interface values remain open.

- R07 dependency-closure coherence checkpoint: signature completion rejects
  duplicate concrete trait/receiver implementations in a module's reachable
  closure even when no call uses them. The diagnostic belongs to the importing
  source revision. Rechecking after an overlay edit removes the conflict from
  the new snapshot without changing the old one. Generic-pattern overlap and
  runtime interface values remain open.

- R07 generic coherence checkpoint: the same conservative overlap rule used
  for implementations in one module now applies across the reachable dependency
  closure, including generic receiver and applied-trait patterns. Overlap fails
  during signature checking even when no method is called. Distinct concrete
  applications such as `Echo<i32>` and `Echo<bool>` remain independent. The
  regression checks a sibling-module template/concrete conflict, an overlay
  repair and retained old-snapshot diagnostics. Executing methods from generic
  implementations defined in dependencies remains open.

- R07 cross-module specialization checkpoint: bound checking now matches
  dependency-defined generic implementations after substituting receiver and
  applied-trait arguments, including their declared bounds. Program lowering
  collects concrete method-instance requests from cross-module calls, repeats
  dependency lowering until transitive requests settle, and shares the existing
  generic-instance and instruction budgets across the closure. Link bindings use
  declaration identity plus concrete type arguments; verification checks the
  selected instance and call representation before bytecode emission. The
  [generic imported-trait example](../examples/imported-traits/generic-consumer.kgr)
  produces two method instances and runs from source and encoded artifacts through
  the interpreter and existing JIT fallback. A transitive dependency fixture
  requires more than one planning pass. Runtime interface values and dispatch
  remain open.

- R07 host trait-table declaration checkpoint: KHI v8 attaches trait identity
  and trait-method-to-host-method bindings to an offline host type. Validation
  rejects foreign method owners, duplicate mappings and missing host methods;
  the type fingerprint covers the table but excludes documentation. Runtime
  linking requires both an identical registered type table and callbacks for
  every mapped host method before publication. KBC format/runtime ABI v31 reject
  older products. Executable host interface dispatch remains open, so this
  does not complete R07. The
  [host trait-table example](../crates/kagari-runtime/examples/host_trait_table.rs)
  demonstrates offline round-trip and callback linking; an encoded-artifact test
  rejects missing callbacks before module publication.

- R07 host trait-signature checkpoint: signature completion compares each host
  binding with the script trait in its defining module. It rejects missing or
  extra methods, receiver/parameter count and type mismatches, return type
  mismatches, and generic methods without a concrete host binding.
  These are source diagnostics before code generation; changes to host
  declarations or script signatures invalidate them. Host passing styles remain
  explicit ABI properties. Executable interface dispatch remains open.

- R07 static host trait-call checkpoint: a trait table on a concrete
  host type satisfies static generic bounds. Reachable specialization resolves
  the trait method identity to its declared host method and emits an ordinary
  verified host call, retaining the host capability and borrow contract. The
  [host bound example](../examples/host-trait-bound.kgr) runs from source and
  encoded artifacts through the interpreter and existing JIT fallback; removing
  the host trait table rejects the bound before code generation. A same-trait
  script implementation for that host type is rejected as an overlap. Runtime
  interface values and dynamic dispatch remain open.

- R07 applied host-trait checkpoint: KHI v9 stores ordered concrete trait
  arguments in each host implementation; duplicate applications are rejected,
  disjoint applications remain independent, and the type fingerprint includes
  their arguments. Signature completion substitutes trait parameters, checks
  their bounds, then checks every bound host method. Static generic calls match
  the complete applied identity. The [host bound example](../examples/host-trait-bound.kgr)
  selects distinct `Readable<i32>` and `Readable<bool>` methods through source,
  artifact and JIT fallback. KBC format/runtime ABI v32 reject older products.
  Dynamic interface values remain R08 work.

- R07 generic trait-method call checkpoint: method-local type parameters are
  inferred from arguments and expected results, their bounds are checked by the
  shared generic-call rule, and concrete method arguments are recorded in HIR.
  Local and imported implementations specialize by receiver, impl and method
  arguments; IR verifies imported method parameter and result layouts after all
  substitutions. The [generic method example](../examples/generic-trait-methods.kgr)
  produces distinct bool and i32 method instances and runs through source,
  artifact and JIT fallback. Dynamic interface values remain R08 work.

- R07 acceptance: nominal types and generic binders use declaration identity,
  ordered concrete arguments and owner/position rather than display names.
  Reachable struct/enum layouts, field slots and interface method tables are
  encoded and verified with concrete signatures. Function instances are keyed
  by declaration and arguments, deduplicated and charged to a configurable
  limit shared with layout instances; public generic entry functions are
  rejected, leaving public overloads with concrete signatures. Local, imported
  and host trait implementations participate in checked static dispatch,
  including applied generic traits. Source, encoded artifact, interpreter and
  existing JIT fallback fixtures exercise those paths. Dynamic interface values
  and their linked dispatch belong to R08.

R03 acceptance evidence:

- [Source database tests](../crates/kagari-common/src/source_database.rs) cover
  logical module bindings, captured project roots, virtual URIs, overlay precedence,
  immutable revisions and checked Chinese/emoji/CRLF position conversions.
- [Source snapshot integration](../crates/kagari-embed/tests/source_snapshots.rs)
  covers compilation/tooling sharing, module rebinding, diagnostic provenance and
  source/artifact identity. Disk files and host text enter the same database.
- [Declaration navigation](../crates/kagari-hir/src/analysis/identity_tests.rs)
  covers same-named modules, declaration kinds/owners, generic owner/position and
  analysis-owned bindings. [Member tests](../crates/kagari-hir/src/analysis/member_tests.rs)
  cover field/variant arena ownership, exact declaration ranges, stable nominal
  identity after reordering, old snapshots and retained duplicate declarations.
- [Arena tests](../crates/kagari-hir/src/analysis/arena_tests.rs) and
  [body-owner tests](../crates/kagari-hir/src/analysis/owner_tests.rs) cover foreign
  local IDs, interleaved cache reconstruction, explicit function/constant ownership,
  synthetic nodes and rejection of cross-body edges. Declaration/signature/body
  query tests prevent old revisions from replacing newer cached results.
- The source_queries example exposes declaration-site member navigation alongside
  independent declaration, signature and function queries.

This completes source identity and query provenance. Concrete executable
layouts and interface dispatch retain R07/R08 acceptance; R03 does not imply
those execution features are complete.

R04 acceptance evidence:

- [Recovery and identity tests](../crates/kagari-hir/src/analysis/identity_tests.rs)
  retain declaration, scope, receiver and member facts beside parse/name errors;
  the integrated R04 case rejects code generation for that erroneous result.
  [Function queries](../crates/kagari-hir/src/analysis/body_queries/tests.rs)
  keep an incomplete member receiver typed while excluding unrelated bodies.
- [Type recovery tests](../crates/kagari-hir/src/analysis/generic_type_tests.rs)
  cover `Unknown`/`Error` holes, independent composite members, erroneous
  annotations, call arguments and checked-only conversion. Member, constructor,
  owner and arena tests cover semantic targets and source ownership.
- [Signature queries](../crates/kagari-hir/src/analysis/signature_queries/tests.rs)
  navigate checked source/imported type targets before body analysis;
  [offline host tests](../crates/kagari-embed/tests/offline_nominal.rs) verify
  host declarations are queryable without runtime bindings. The source_queries
  and offline_compile examples exercise both paths.
- IR lowering takes only the sealed `CheckedAnalysis` type; its production
  dependencies contain HIR and common data, not the syntax crate. Ordinary
  calls, fields and writes consume resolver/type-table target identities.
  Concrete interface dispatch and execution linking remain R07/R08 work.

R05 acceptance evidence:

- [Declaration queries](../crates/kagari-hir/src/analysis/declaration_queries/tests.rs)
  cover standalone discovery, recovery, shared unchanged file results, dependency
  invalidation and rejection of stale/cancelled cache publication.
- [Signature queries](../crates/kagari-hir/src/analysis/signature_queries/tests.rs)
  cover checking without body analysis, independent reuse, rebased error locations,
  immutable lowering sharing and consumption by full analysis.
- [Function queries](../crates/kagari-hir/src/analysis/body_queries/tests.rs) cover
  selecting one body, absence of neighbor bindings/facts, incomplete member receiver
  types, exact-cache sharing, unchanged-body remapping, fresh local identities,
  same-named impl and cross-module identities, dependency signature invalidation,
  deletion and stale/cancelled publication. Results match fresh analysis.
  Declaration-environment comparison ignores lexer trivia, so leading Unicode
  comments and CRLF shifts reuse unchanged checked bodies while rebasing query
  locations. Token boundaries and literal contents remain significant; declaration
  type changes invalidate reuse. Bodies with diagnostics are still rechecked.
- [Snapshot integration](../crates/kagari-embed/tests/source_snapshots.rs) compares
  artifacts after cache reuse with fresh compilation and checks source/profile
  invalidation. Existing analysis/identity/import tests cover queries on erroneous
  files, dependency facades and old snapshot navigation; source_queries exercises
  declaration, signature, one-body and full queries through the embedding API.

Full analysis batches bodies through the same resolver/type checker used by the
single-function query. Module constants remain shared semantic prerequisites and
are checked when querying a body. This is bounded query caching, not a complete
incremental dependency framework. R15 resource limits and R18 performance
measurements retain separate acceptance.

R12 acceptance evidence:

- [Execution sessions](../crates/kagari-runtime/tests/execution_sessions.rs) cover
  immutable root permissions, per-root budgets/peaks, cancellation, inherited
  scopes and root-version retention; independent calls start fresh budgets.
- [Execution frames](../crates/kagari-runtime/tests/execution_frames.rs) cover one
  session-owned stack, suffix cleanup, GC roots and cleanup after quarantine.
  Module instance borrows are confined to individual load/store/init operations;
  the existing explicit executor remains the single script driver.
- [Host scopes](../crates/kagari-runtime/tests/host_scopes.rs) cover registered
  temporary roots/leases, retained root lifetime, ordinary error/termination/fault
  cleanup, foreign and expired tokens, and declared borrow conflicts.
- [VM session fixtures](../crates/kagari-vm/src/tests/sessions.rs) cover synchronous
  reentry, GC-safe returned objects, nested cancellation/budget cleanup and pinned
  epochs through direct/encoded interpreter and existing JIT fallback routes.
  [Path fixtures](../crates/kagari-vm/src/tests/helpers.rs) also reenter during path
  validation/read/preparation, while commit reentry still quarantines the runtime.
- The host_reentry embedding example exercises rooted results, temporary scopes
  and complete-stack observation. No async API or cross-thread execution was added.

This completes R12, not R10/R11/R13/R17: interface/capture ownership,
the wider engine-invariant audit, lexical debug visibility and remaining backend
contracts retain their own acceptance requirements.

R01 specifies target behavior. Remaining runtime behavior must not be described
as conforming until its corresponding checkpoint and regression tests pass.

Implemented foundation slices:

- R02: one source/artifact/JIT fixture format now checks values, structured
  diagnostic codes, index traps, host failures, ordered host calls, committed
  host mutation records and final host state. It covers left-to-right evaluation,
  alias/identity/tuple behavior, short circuit, effects surviving traps, rejected
  host mutations and cached initialization failure. Run with `cargo test -p
  kagari-vm language_contract`. The baseline contract matrix is now accepted for
  R02; remaining value/activation implementation audits retain their own steps.
  Enum payload equality, distinct mutable enum members, shallow container copies,
  and rejection of interface equality now run through these same routes.
  Map/Set parameter and return aliases, identity inequality for equal contents,
  shallow map-value projections, independent set-to-array structure, and Map
  identity inside enum/tuple members now have the same three-route fixtures.
  These are JIT/fallback fixtures; they do not claim native container compilation.
  The runtime callback `iter.for_each` entry now holds a rooted collection guard
  and roots pending snapshot items across callbacks. Array/Map/Set structural
  writes through all heap mutation entries are refused while guarded; element
  replacement and existing-key updates remain allowed. Tests cover every current
  structural builtin, unchanged contents/allocation counters, nested guards,
  callback failure cleanup, GC after replacement, and foreign/stale handles.
  Heap pop/remove/clear now return explicit Results, separating absence from
  iteration, invalid-key/handle and execution rejection. Callers were replaced
  directly, and tests cover empty collections, stale handles, quota preservation
  and candidate initialization rejection without legacy Option-only adapters.
  Array element and Struct slot replacement now return Results too, preserving
  execution failures through VM writes and retaining reflective internal-fault
  categories. A corrupted struct storage/layout invariant quarantines its runtime;
  tests verify failed replacements preserve targets and subsequent writes fail.
  Ordinary VM array bounds failures retain their existing InvalidIndex outcome,
  now mapped directly from a distinct runtime IndexOutOfBounds category. The VM
  no longer rereads length to infer the cause of a rejected write; foreign
  payload/handle failures cannot be relabeled by an independently invalid index.
  The runtime example `collection_iteration` demonstrates host guard ownership.
  Source callback/for-loop integration and their cross-route cleanup tests remain
  outstanding; this runtime substrate does not complete iteration acceptance.
  Integer arithmetic traps, retained pre-trap host effects and budget precedence
  now use these routes too; selected fixtures require actual native invocation.
  Plain and compound assignment fixtures cover computed roots, single index
  evaluation, RHS replacement/removal/creation of a target, current-slot reads,
  root identity retention, tuple copy updates and arithmetic failure.
- R03: source IDs/revisions, immutable snapshots, base/overlay precedence and
  checked UTF-8/UTF-16/CRLF coordinates. The engine now compiles and queries the
  same snapshots; disk loading, host text and overlays use one ingestion path.
  Relative paths resolve against a captured root; virtual URI schemes survive
  normalization. Embedding diagnostics carry file/revision ranges. Cross-module
  aggregate/member declarations share the same identity model.
  Logical package/module bindings now belong to source documents and survive
  overlays. Rebinding invalidates analysis; duplicate source bindings are rejected.
  Source-based HIR carries its origin through IR, bytecode and artifact metadata.
  The AST-only analysis entry and post-analysis identity overrides were removed.
  Declaration paths now include module, kind, owner and duplicate occurrence.
  Parameter/local identities include their body and analysis instance; stale local
  IDs cannot resolve in a new analysis. Definition navigation returns file/revision
  ranges and distinguishes same-spelled declarations in separate modules.
  Raw expression/block/statement/place/pattern/local/parameter/type-reference IDs
  now include the identity of their immutable lowering arena. Bare-index constructors
  were removed; semantic lookups reject foreign IDs and node/source-map access checks
  arena ownership before indexing. Body/signature reuse remaps IDs into the current
  arena. Equal source revisions may still require remapping when independently
  reconstructed lowerings meet caches at different query stages. Cross-lowering
  collisions and that interleaved-query case have regression coverage. Local IDs
  also carry an explicit HirOwner allocated during lowering: function, constant,
  or shared declaration context. Module initializer identity is allocated before
  its nodes, so interleaved declarations and synthetic spans cannot misassign it.
  Resolver scopes share the same BodyOwner and reject cross-body edges/bindings;
  nodes and source maps verify stored owners. Shared impl receiver types keep their
  declaration owner. Storage remains in immutable module arenas, with each body's
  ownership explicit in its IDs. Field and variant IDs also carry arena, owner and
  slot; signature reuse remaps field keys. Enum variants have nominal owner/name
  paths and exact source-map entries, including separate duplicate occurrences.
  Declaration-site member queries work before body analysis; duplicate variants
  remain queryable with diagnostics and cannot pass code generation.
  Generic parameters now have owner/position identities and declaration ranges;
  inherited trait/impl parameters keep their original owner in method signatures.
  Snapshots now resolve standard, offline host and registered source imports through
  one import fact table. Package-qualified source namespaces and public items retain
  file/revision identity; conflicting namespaces and private imports are diagnosed.
  Cross-file definition queries follow public source facades and keep old snapshot
  locations. The source_modules embedding example prints a dependency-first graph.
- R04: analysis retains facts and diagnostics; unknown/missing expressions are
  represented explicitly, and codegen requires a sealed CheckedAnalysis.
  Host calls with missing or extra arguments preserve their declared return type
  and downstream member facts. Available arguments still receive type checking;
  arity/type diagnostics reject code generation. Free-function and method recovery
  tests retain neighboring function queries and known host field declarations.
  Failed generic-call inference substitutes error arguments into the return type
  as well as the recorded instantiation. Callee binders cannot leak into callers;
  known arguments, caller-owned binders and nominal member identities survive.
  Inference traverses partially erroneous composite arguments to use their valid
  members. Partial whole-type candidates merge complementary facts from later
  arguments through structural recovery, while conflicts remain diagnosable.
  Recovery now fills independent member holes even when another member conflicts;
  established facts and conflict diagnostics remain intact. Different nominal owners,
  kinds and arities cannot contribute recovery facts across their shape boundary.
  Member recovery and conflict comparison also use explicit stacks; a 10,000-level
  regression checks hole filling, compatible comparison, conflicting leaves and
  preservation of established facts without recursive member traversal.
  Tuple and nominal-constructor regressions cover
  this recovery. Annotation resolution now returns recoverable types directly,
  preserving composite shapes containing unknown members. Parameters, returns,
  fields, enum payloads, local annotations and constants retain facts and report
  unresolved-type diagnostics; a 24-case matrix verifies queries and codegen
  rejection. The old optional whole-annotation result is removed.
  Index analysis uses the known outer composite shape even if an unrelated member
  is erroneous. Tuple and nested-array queries retain selected member types;
  out-of-range and noninteger tuple indexes still diagnose and reject codegen.
  Type-conflict checks now compare known composite structure recursively. Recovery
  members suppress only their own dependent mismatch; unrelated member conflicts,
  nominal identities and arity differences remain diagnosable.
  If/match branches and array elements merge complementary recovery facts through
  the same structural operation. Original diagnostics continue to reject codegen;
  subsequent real conflicts remain errors and cannot overwrite established facts.
  All local module-level declarations and import aliases now share an immutable
  name table. Calls, annotations, constructors, bounds and impl headers consume
  it; duplicate names retain identities but have no winning target or codegen.
  Regressions cover all 36 ordered declaration-kind combinations, unaffected
  neighbors and invalidation when a collision is introduced and removed. Duplicate
  imports revoke every target, and failed imports block fallback. Qualified source,
  host and standard-library calls respect lexical root shadowing; the checker no
  longer reconstructs standard-library targets from expression strings. Runtime
  helpers now have explicit prelude resolver targets; the checker's remaining
  helper string fallback was removed. Arity errors retain those targets, local
  declarations/bindings shadow every helper, and body cache reuse rebases calls.
  Bare function/module/type items now fail with a structured HIR diagnostic rather
  than giving a bare function its return type and reaching an IR rejection.
  Source/artifact/JIT fixtures exercise helper effects, shadowed print and those
  diagnostics. Impl trait headers now retain the same typed references as bounds,
  including generic arguments and exact source spans; validation and ABI lowering
  consume the checked targets. Applied traits cannot silently erase arguments or
  register an implementation, and generic binders shadow trait/standard-constraint
  names consistently with annotations. Imported trait targets retain nominal
  identity/navigation through facades while their execution remains explicitly
  unsupported. Regressions cover recovery, signature-cache rebasing and cross-module
  targets. Trait constraints, implementation keys and trait-method call targets now
  carry nominal declaration IDs instead of local trait/method numbers; query and
  monomorphization consumers use those identities. Regressions reject foreign
  trait/method IDs even with matching local slots, preserve targets across declaration
  reordering and rebase cached receivers. Source/artifact/JIT fixtures distinguish
  two same-named trait methods implemented by one receiver type. Imported trait
  execution and interface linking retain R07/R08 acceptance. The shared aggregate
  catalog now owns checked trait/method contracts, including nominal parameters,
  bounds, Self types and source targets. Method calls and navigation consume this
  catalog for local and imported interface annotations; local HIR method lookup is
  removed. Ambiguous bound methods and duplicate method declarations reject codegen.
  Tests cover facade imports, same-named methods across modules, signature errors,
  unchanged caller-body reuse and dependency signature invalidation. Dependency
  source revisions still conservatively invalidate consumers, even for body-only
  dependency edits; this does not claim dependency-level incremental reuse.
  Checked function signatures now own generic constraint maps. Parameter/return
  validation, body environments, calls and method catalogs consume these maps;
  repeated downstream HIR constraint reconstruction is removed. Signature-query
  tests cover inline/where impl bounds under method shadowing, partial constraint
  errors, body-edit reuse and bound-change invalidation. The source_queries example
  exposes constraints before body analysis; execution fixtures forward checked
  where bounds through source, artifact and existing JIT fallback routes.
  Type applications now resolve their base through the same declaration/binder
  lookup as named annotations. Explicit bindings block standard constructor
  fallback, and empty argument lists are retained instead of becoming bare types.
  Invalid applications retain base and argument targets; navigation selects the
  innermost annotation even when its target is unknown. Regressions cover all four
  standard constructors, facades, partial arguments, signature rebasing and repair.
  Source struct/enum applications now retain declaration-owned binders and check
  arity and bounds. Constructors infer arguments from fields/payloads; field access
  substitutes the receiver's arguments. Signature reuse preserves binder identity.
  Semantic Struct/Enum/Trait identities now carry a NominalType with declaration
  identity and ordered arguments. Substitution, inference, concreteness, recovery
  and display recurse through those arguments. ABI types preserve the same shape;
  format 22/runtime ABI v23 reject older products. Layout verification rejects
  applied nominal payloads until their concrete layout exists, rather than binding
  them to zero-argument declarations. Reachable struct/enum layouts are now emitted
  per concrete instance and deduplicated. Layouts and function instances share the
  program-wide instantiation budget; recursively growing layouts terminate with a
  structured diagnostic. Public generic enum ABI templates validate against each
  local/imported instance, including when the owner emits no instance. Tests cover
  facades, distinct field representations, bounds, malformed binders, source/artifact/
  JIT fallback behavior and the standard-library example. Backwards generic-call
  constraints, generic impl specialization
  and interface tables were pending at this checkpoint. The shared argument-inference entry for
  calls and enum payloads is now used by
  concrete and trait parameter contexts too, with cancellation between operands.
  Constraint matching now uses an explicit work stack and checks analysis cancellation
  between members, preserving left-to-right inference precedence. A 10,000-level
  synthetic composite test covers traversal without recursive stack growth; type
  cloning and other operations retain their separate depth-limit work.
  Constraint inference returns explicit cancellation; return-context, constructor
  field and generic-call consumers stop before deriving missing-argument diagnostics
  or instantiations from a cancelled argument batch.
  Parameter contexts retain known composite members beside uninferred callee
  binders, which enter the context as Unknown holes rather than escaping into
  caller facts. Constructor/call finalization diagnoses remaining Unknown holes;
  Error members retain their existing diagnostics. Source/artifact/JIT fixtures
  exercise independently contextual enum and struct members in tuple arguments,
  while unseeded constructors and containers remain rejected.
  Struct fields now use that same context operation as call and enum payload
  arguments, preserving independent tuple members even before the enclosing
  binder is inferred. Context reconstruction reads substitutions directly instead
  of cloning the substitution map per argument. Struct/enum execution fixtures
  and negative constructor cases cover the unified path.
  Failed argument finalization is shared by calls and constructors: it reports
  uninferred holes and converts only those leaves to Error, preserving known
  tuple/container members and nominal identities for tooling. Regression cases
  inspect retained local-reference facts and reject codegen in all three paths.
  Finalization now also updates the shared substitution before constructor member
  validation. Result facts and field/payload diagnostics use identical recovery
  types; known sibling conflicts still diagnose after missing positions become
  Error. The former function-only substitution update is removed.
  Profile-gated set_field/set_index helpers now pass checked target types to their
  RHS expressions and compare known members using the same conflict predicate
  as assignment. Missing members suppress dependent mismatches without hiding
  independent conflicts; analysis and source/artifact/JIT fixtures cover both.
  Reflection field helpers reject nonexistent members in HIR instead of inventing
  Unit results; reflective indexing rejects known invalid receiver/index pairs.
  Operand recovery errors suppress dependent diagnostics. Focused tests ensure
  invalid helpers cannot obtain checked codegen input and unknown operands emit
  only their original name diagnostic.
  Reflective field writes now check declared field writeability in HIR, matching
  the runtime layout guard. Read-only fields still supply RHS type context and
  preserve independent type mismatch diagnostics; reads remain permitted.
  Reflection field names now have an explicit HIR constant-String check. Invalid
  names retain their helper call target and Error result instead of falling through
  to ordinary function resolution. Name-type, nonconstant-name and RHS errors keep
  distinct diagnostics; existing name errors do not gain cascading call errors.
  Resolved reflection field-name expressions retain declaration-owned field
  targets in the semantic table. Definition queries reuse the normal field-fact
  path, including invalid writes; same-spelled fields remain distinct. Tests
  verify navigation after body reuse and Unicode/CRLF shifts and retain old
  snapshot locations without changing the field-name expression's String type.
  Ordinary and reflection field reads now share member resolution, target fact
  recording and missing-member recovery. A known Struct with erroneous generic
  arguments still diagnoses absent fields, while valid fields keep their types
  and declaration targets. A wholly unknown receiver suppresses dependent errors;
  incomplete dot access retains its explicit missing-name diagnostic.
  Ordinary reads and reflective index writes share checked index diagnostics.
  Partially erroneous composite indices still diagnose their known noninteger
  shape; only whole Unknown/Error indices suppress dependent diagnostics.
  Positive recovery cases and independent invalid-index errors cover both paths.
  Standard-library argument checks now use the same known-member conflict
  predicate as script calls and assignments. Whole erroneous operands and missing
  operands no longer duplicate primary expression/arity diagnostics. Array method
  and qualified calls plus String functions cover recovery and independent
  mismatches; partially erroneous non-container operands still diagnose.
  Standard container arguments now receive element/key/value context after the
  receiver is checked in source order. Method and qualified calls share this
  path, including Set algebra and Option/Result unwrap_or fallback context.
  Constructor positive/negative cases and source/artifact/JIT execution verify
  Array and Map mutation plus Option fallback; the standard-library example
  includes a context-inferred generic element constructor.
  Binary recovery preserves bool results for comparison/logical operators and
  checks independent known operand conflicts rather than discarding all facts
  when any member is erroneous. Equality checks known Comparable members on both
  sides; numeric and logical operations reject known incompatible shapes even
  beside an unknown operand. Erroneous expressions still cannot pass codegen.
  Unary negation now consumes declaration-owned SignedNumber bounds, including
  where clauses and forwarded generic calls. Unconstrained/OrderedNumber-only
  binders and unsigned instantiations remain invalid. Known nonnumeric composite
  shapes still diagnose beside error members; whole error operands do not cascade.
  Source/artifact/JIT execution and the standard-library example cover generic
  negation without a separate runtime implementation.
  Standard min/max/clamp validate every known operand, merge recovery holes into
  their result and retain independent mismatch diagnostics. assert_eq checks both
  operands' Comparable constraints and known-member conflicts. The first operand
  supplies context to subsequent matching operands, with enum RHS constructor
  inference verified across source/artifact/JIT execution.
  Ordinary binary operators now pass the checked left type into RHS inference;
  logical operators supply bool context. Explicit constructor arguments still
  determine their own type and mismatches remain errors. Enum/tuple cases cover
  contextual constructors, and execution fixtures count left/right calls to
  verify source order and single evaluation through all existing execution routes.
  Array elements and completing if/match branches also pass established types
  forward when no enclosing context is present. Terminating branches do not seed
  result inference, and unreachable match arms retain independent checking.
  Explicit type conflicts remain rejected; execution fixtures count effects in
  array elements and selected branches across source/artifact/JIT routes.
  Array element joins now stop at the first expression that cannot complete
  normally. Terminating and subsequent elements keep independent diagnostics
  but do not produce spurious homogeneous-element conflicts. Tests retain real
  prefix conflicts and confirm prefix effects survive while suffix execution
  is skipped across source/artifact/JIT routes.
  If/while conditions now require bool only when they can complete normally.
  Conditions that return on every path retain their inner diagnostics without
  inventing a Unit-to-bool mismatch. Partially returning non-bool conditions remain
  invalid; execution fixtures preserve prefix effects and skip branch/loop bodies
  and continuation code through source/artifact/JIT routes.
  Explicit return checks likewise compare a value only when its operand can
  complete normally. Nested returns no longer fabricate an outer Unit mismatch;
  inner return mismatches and partially completing wrong types remain diagnosed.
  Initializer and assignment type checks now also require a normally completing
  value. Terminating compound RHS expressions do not fabricate operator errors;
  annotations, inner diagnostics and target writeability checks remain intact.
  Source/artifact/JIT fallback tests verify one RHS condition side effect, no
  final field write and no execution of subsequent statements.
  Script function parameters now exclude terminating operands from argument
  comparisons and generic inference. Calls terminated by an argument do not
  invent missing generic-argument errors; arity and inner diagnostics remain.
  Source/artifact/JIT fallback coverage confirms that concrete and bounded generic
  calls skip callee side effects while retaining the terminating argument effect.
  Script functions and trait methods now reuse the standard parameter comparison
  entry, removing duplicated mismatch construction and applying the same
  completion/cancellation checks. Trait and debug-assert fixtures cover valid
  termination, wrong completing values, inner errors and source/artifact/JIT
  execution with preserved operand effects.
  Math and debug-equality constraint checks now exclude operands with no normal
  completion, while preserving independent known operand errors. IR records call
  and enum-construction result layouts only after operands complete, avoiding
  spurious unresolved-result rejection for calls that cannot execute. Tests cover
  first, later and all-terminating operands across source/artifact/JIT fallback.
  Offline host signatures now supply argument context and use the shared
  completion-aware parameter checker instead of a separate mismatch path.
  Composite host tests cover inferred empty arrays, branches, rejected completing
  values and callback counts for terminated calls across all three execution
  routes; reading declarations still requires no runtime registration.
  Enum payload comparison now also uses the shared parameter checker. Constructors
  that exit during payload evaluation do not fabricate missing generic-argument
  diagnostics. Tests verify inferred/explicit enum paths, preserved inner and
  payload errors, no unused concrete layout and skipped later operand effects
  across source/artifact/JIT fallback.
  Struct fields likewise exclude terminating expressions from generic inference
  and assignment comparison; missing/duplicate/unknown field checks remain.
  IR delays Struct result layouts until all field values complete. Inferred and
  explicit generic Struct fixtures verify inner errors, invalid later fields and
  skipped later effects across source/artifact/JIT fallback.
  Unary negation/not now check only normally produced operands. IR layout
  registration is centralized after expression lowering and normal-completion
  checking, replacing per-call/constructor exceptions. Nested unary/Array/Tuple/
  standard-call fixtures verify no unused result layout and correct effects
  across source/artifact/JIT fallback.
  Binary operand checking explicitly distinguishes absent values from produced
  types. Terminating operands supply no matching constraint or RHS context,
  while known invalid counterpart types remain diagnosed. Source/artifact/JIT
  tests verify arithmetic/comparison termination and both taken and skipped
  short-circuit RHS paths.
  Reflection field/index writes now share contextual RHS checking and compare
  only normally produced values. Tests preserve read-only/invalid-index and
  inner diagnostics; source/artifact/JIT execution confirms prior RHS effects
  survive while neither final field nor array writes occur.
  Shared index resolution now accepts terminating indexes without requiring a
  fabricated integer value. Array element facts survive; Tuple member facts stay
  unknown when no index is produced. HIR retains receiver/writeability failures;
  source/artifact/JIT tests cover reads, ordinary/compound writes and reflection
  writes, including skipped RHS side effects.
  The shared completion evaluator now uses a cancellable explicit work stack
  instead of recursive expression/block/place traversal. Lazy sequences preserve
  early termination and match-arm reachability. Tests cover 20,000 nested
  expression/block layers, 20,000 place layers, no subsequent operand request
  after termination and cancellation during operand acquisition. Other frontend
  depth limits and recursive traversals remain R15 work.
  Completion traversal now memoizes finished node exits per query, keyed by full
  owned HIR IDs. A 48-level shared-subtree fixture avoids exponential expansion,
  and a shared block test verifies cached breaks are consumed only by loops.
  Cancellation discards query-local facts; there is no cross-snapshot cache.
  Match analysis now uses scrutinee completion to gate pattern compatibility
  and arm result joins. Unreachable arms retain independent diagnostics.
  HIR positive/negative cases and source/artifact/JIT execution verify scrutinee
  effects and skipped arm dispatch without fabricated Unit/pattern mismatches.
  If-result joins and sibling context now also require a normally completing
  condition. Condition checking returns completion/cancellation explicitly;
  callers reuse the fact instead of repeating traversal. HIR and execution
  fixtures cover mixed unreachable result types, absent else branches,
  retained inner diagnostics and skipped branch effects.
  Field reads no longer fabricate missing-member errors for terminating
  receivers. IR evaluates the receiver before requiring its checked field/layout
  target. Tests cover chained reads, incomplete names, invalid inner returns and
  known missing members; all execution routes preserve the receiver effect.
  Index reads also distinguish absent receiver values from invalid produced
  containers. HIR retains independent index errors and normal-path compatibility
  checks; chained-index execution fixtures verify later index effects are skipped
  consistently across source/artifact/JIT fallback.
  Qualified standard container/String/Option/Result/iterable calls now share
  completion-aware receiver extraction instead of repeated raw-first-argument
  fallback. Terminating operands cannot seed later argument context. Tests cover
  receiver shape errors, inner returns and skipped subsequent argument effects
  across source/artifact/JIT fallback.
  Reflection get/set field and set index now share produced-receiver inference,
  keeping cancellation separate from absent receiver values. Target checks wait
  for a produced receiver; field names and later operand errors remain checked.
  Execution fixtures verify only the terminating receiver effect is observed.
  Calls whose callee evaluation exits now carry an explicit TerminatingCallee
  semantic target, including the evaluated expression identity. IR consumes that
  fact and checks its termination contract before skipping arguments; it does
  not reconstruct callability from syntax. Tests cover retained inner errors,
  invalid produced callees, body-cache rebasing and execution effect order.
  Source/artifact/JIT fixtures verify the inner result and single evaluation of
  the branch condition.
  It still propagates constraints forward; backwards contextual inference remains
  an outstanding solver change. Independent signature queries construct
  the shared aggregate catalog and check applied bounds in signatures. Full/body
  analysis consumes that result instead of repeating header validation. Tests cover
  facade dependency changes, repair, body-edit diagnostic rebasing, unchanged query
  sharing and a correct function beside an invalid applied signature. The
  source_queries embedding example demonstrates the separate diagnostic boundaries.
  Enum payload annotations now survive lowering as declaration-owned type references.
  Signature checking retains every member, including missing/unknown Error facts,
  and exposes types before body analysis. Nominal enum/variant payload contracts
  enter the shared aggregate catalog for reachable dependencies; body-only edits
  rebase their references and payload contract edits invalidate dependent body reuse.
  Public variant ABI records encode checked structural/nominal payload types in
  format 11, so changing payload types rejects reload before publication. HIR and
  embedding regressions cover recovery, cache reuse, cross-module identity,
  encoding round trips and unchanged runtime entry after ABI rejection.
  Qualified constructor paths now retain resolved owner facts and nominal enum/
  variant targets. Payload arity/type errors preserve target navigation and result
  types; missing variants retain their enum owner while arguments remain checked.
  Source imports/facades, local shadowing and independent cached body queries share
  those facts. IR now emits nominal enum layouts and constructor operands; bytecode
  links them to checked enum/variant slots. Layout validators check ownership,
  payload references, counts and representations, and reject cross-module conflicts.
  Runtime variants retain their executable generation; allocation validates runtime,
  payload and schema before charging resources. Arbitrary string enum allocation and
  Option/Result string dispatch were removed in favor of typed tags. Value equality,
  mutable-member identity and evaluation order share source/artifact/JIT-fallback
  conformance fixtures; embedding tests cover foreign runtimes, changed nested
  schemas, rooted old-version survival, same-named standard types and dependencies.
  Format 22/runtime ABI v23 reject older products. Remaining interface/layout
  contracts retain R07/R08 acceptance.
  Erroneous signatures retain parameter slots with Error types. Invalid local
  annotations, call targets and indices produce diagnostics; unresolved operands
  suppress dependent mismatch diagnostics. Empty Map/Set constructors retain
  concrete parameters inferred from binding annotations.
  Scalar const values are now HIR facts. Arithmetic failures and annotation
  mismatches reject code generation while retaining unrelated facts. IR no longer
  evaluates const syntax or rebuilds legacy aggregate const objects.
  Literal expressions and match patterns now carry checked scalar facts, including
  the i32 minimum spelling. Invalid literal ranges and pattern type mismatches
  are source diagnostics. IR consumes these facts without reparsing literal text.
  Name resolution now owns lexical scopes, binding introduction points and match
  arm bindings. Tool scope queries consume these facts instead of reconstructing
  scopes from HIR blocks. Navigation uses resolved expression and assignment names;
  cross-module definitions also use the shared import graph. Type references retain checked types and
  declaration targets in signatures, field/const/local annotations and impl headers.
  Unknown composite members do not discard later members. Type navigation and
  interface permission checks use these facts; IR no longer formats impl type syntax.
  Semantic user types now carry declaration identities, and generic TypeId values
  use owner/position identity; diagnostic names do not affect equality or hashing.
  Generic constraints now resolve once to standard or trait targets. `where`
  targets must name generic parameters; impl constraints are inherited by methods
  without leaking into shadowing parameters or sibling method scopes. Diagnostics
  use individual bound ranges and inherited invalid references report once.
  Navigation, generic checks, trait method selection and bound ABI metadata consume
  these facts. Applied trait constraints reject codegen rather than discarding
  arguments; concrete instantiation remains pending R07.
  Struct fields have owner/slot HIR identities and module-owned declaration paths.
  Field types, read/write targets and initializer targets are retained as HIR facts;
  IR field operations and field ABI metadata consume them. Invalid field types retain
  navigation targets without unknown-member cascades; duplicate fields reject codegen.
  Resolved field and struct-initializer targets now use nominal declaration IDs
  instead of file-local struct/field IDs. A checked aggregate catalog supplies field
  types, declaration order, writeability and source locations for local and imported
  accesses. Nested imported struct construction/read/write uses the same checks;
  IR consumes the catalog instead of looking fields up in the current HIR module.
  IR now carries nominal struct layouts and owner/slot operands for construction,
  reads and writes. Its verifier checks layout/field identities, slot completeness,
  representations and write permissions before bytecode emission. The old IR field
  name operands and string-based field interning were removed. Bytecode carries
  these layouts and uses StructId/FieldRef slots instead of named field records.
  Heap structs retain a verified version layout and positional values. Reads check
  the actual layout; writes also check permissions and value representations before
  committing. Explicit reflection resolves names from layouts and shares slot
  mutation checks. Allocation rejects foreign-runtime layouts and invalid field
  values before accounting. Old objects retain their layout; compatible layouts
  across versions require equal identities, slots and schemas. Initializer values
  are emitted in layout order after source-order evaluation. The layouts IR example
  shows these contracts. Full nominal field schema verification and interface
  tables remain pending, so R07/R08 are not complete.
  Calls now carry one HIR target and an explicit receiver. IR and reflection
  permission checks consume that target; duplicate Array/String method checking,
  backend builtin-name classification and fallback call dispatch were removed.
  Source String length uses the specified len_bytes/len_chars intrinsics; the old
  String.len source entry is rejected. The old BuiltinMethod declaration tables,
  IR/bytecode operands and runtime dispatch API are removed. StandardIntrinsic is
  the sole standard-library execution path, including manually built bytecode.
  Array/string iteration and empty pop now have source/artifact/JIT fallback
  fixtures covering Option results, Unicode indexing and shared-array mutation.
  Trait-call targets support navigation even with argument errors; executable
  interface dispatch still awaits linked implementation tables in R07/R08.
- R05: unchanged files share parse/analysis results; identical function bodies
  reuse remapped type facts, while declaration changes invalidate that reuse.
  Scope/type/member-receiver queries work on erroneous files. Shared cancellation
  now reaches lexer character iteration, parser traversal, HIR expression/block
  lowering, name resolution and body checking. Cancelled snapshots do not publish.
  Query ownership is now explicit; compile-time limits remain in R15.
  Language profiles participate in cache reuse; old queries cannot publish over
  a newer revision. Engine snapshot compilation exposes cancellation explicitly.
  Reused bodies remap scalar expression and pattern facts; emitted artifacts are
  checked against fresh analysis after preceding code changes shift arena IDs.
  Call facts remap their receiver IDs on body reuse and share this artifact check.
  Inferred generic call arguments are retained on reuse; the cache/fresh artifact
  comparison now includes a reused generic template and a call to its i32 instance.
  Field access, assignment and initializer facts also remap on body reuse; signature
  changes invalidate field slot reuse while named field identity survives reordering.
  Body-local type-reference facts are remapped too, including after preceding edits
  shift type arenas. Shared implicit receiver references retain their impl context.
  Stable named declarations can be located in a new snapshot, while local binding
  handles remain scoped to their original analysis, including on profile changes.
  `cargo run -p kagari-embed --example source_queries` exercises these tool APIs.
  Source dependency revisions and public export sets now participate in analysis
  reuse. Adding/removing an overlay-only dependency invalidates unchanged importers;
  cached old snapshots retain their original targets. This is conservative file
  dependency invalidation. Function signatures, field types and impl contracts now
  have a separate immutable query result, prepared for all files before body checking.
  Imported function calls and public source facades consume those checked signatures;
  scalar/composite argument checks preserve nominal declaration identity. Signature
  facts survive dependency body/constant errors. Transitive facade signature changes
  invalidate callers even when their direct import revisions did not change; unrelated
  file results and unchanged local signature results remain shared. Edits confined
  to user/impl function bodies now reuse checked signatures, including erroneous
  signatures. Type-reference arena IDs and diagnostic ranges are rebased onto the
  new lowering; generic/field/impl targets retain their declaration identities.
  Declaration text, imported type bindings or host declaration changes invalidate
  reuse. Cached and fresh analyses are compared for types, navigation, diagnostics
  and emitted artifacts, including CRLF/Unicode edits and transitive type changes.
  `FileAnalysis::signatures_reused()` reports whether its signature query reused
  checked facts; unchanged query results keep their original statistic.
  `AnalysisDatabase` and `KagariEngine` now expose independent `declarations` and
  `signatures` queries with immutable file results. Both stop before body name
  resolution, local binding collection, type checking and const evaluation.
  Full analysis consumes these same cached queries and creates fresh local binding
  identities only when bodies are analyzed. Lowered HIR is shared immutably across
  declaration, signature and full results instead of deep-cloned per stage.
  Complete snapshots expose the exact
  declaration/signature snapshots they consumed. Each query publishes caches only
  after cancellation checks and cannot replace newer source revisions. Partial
  declaration and signature results retain their own diagnostics and can be reused
  without any previous full analysis. `body(source, definition, cancel)` now queries
  one function independently and retains its own immutable source/signature context,
  scopes, bindings and type facts. Neighbor bodies are not resolved or checked.
  Its cache handles source/dependency changes, deleted declarations and old queries;
  unchanged user/impl bodies remap facts by declaration-order function identity,
  including same-named methods. Full analysis uses the same checker in batch form.
  All module declarations are now available before signature checking. Public struct,
  enum and trait type annotations resolve through direct imports, qualified module
  aliases and source type facades. Parameter, return, field and local annotations
  retain nominal identity and dependency-owned navigation targets. Type import changes
  invalidate local signatures and bodies, including through unchanged facade files.
  Function exports may use types imported from another source module. Public facade
  target traversal is shared by type imports, imported calls and definition queries;
  cyclic or stale targets cannot escape their snapshot. Imported trait implementation
  contracts remain pending; local generic impl specialization is covered above.
  Aggregate contracts from reachable dependencies participate in body invalidation,
  including when a function's nominal return type stays unchanged but its fields
  change. Unrelated module results remain shared. Local body edits can reuse nominal
  field targets while navigation uses the new catalog's declaration positions.
- R03/R14: an immutable import graph detects strongly connected components with
  explicit stacks and rejects reachable cycles before compilation. Its deterministic
  dependency-first order visits diamond dependencies once and ignores unrelated
  cycles. Embedding errors preserve the dependency file and revision.
  CheckedProgram owns the complete immutable source closure;
  dependency body/constant errors reject compilation with their own file/revision.
  IR retains module boundaries and dependency edges, and verifies imported declaration
  contracts against module/function link slots, including public source facades.
  Same-spelled functions and stale document targets remain distinct. Generic-instance
  and instruction limits are shared across the closure. CheckedModule now exposes
  this program instead of a single analyzed root. BytecodeProgram now encodes this
  closure in dependency-first order. Imported calls use module/function slots;
  validation checks graph reachability, signatures and shared layout/host contracts.
  Runtime initialization visits every dependency once, including unused imports,
  and caches failures per runtime and version. Root calls retain one shared program
  version across module calls. VM and embedding reload now stage a verified/linked
  candidate, execute its dependency-first initializers in a restricted root session,
  and only then publish. Failed initialization releases candidate instances and
  module quota without changing the active entry or invoking forbidden callbacks.
  Suspended ordinary calls regain their original session and dependency version.
  The runtime no longer exposes direct reload-and-publish APIs; low-level drivers
  stage a candidate, initialize it, then request publication. Publication rejects
  uninitialized members and rechecks the baseline and host links before activation
  and cache invalidation. Stale candidates cannot alter the newly published entry.
  Candidate instances remain reachable during collection; publication preserves
  initialized results and dropping a candidate removes all of its members.
  Candidate sessions reject external-service, host-mutation, suspension and borrowed
  host-call contracts before callbacks, and reject typed paths before adapters.
  Pure host calls remain subject to permissions and budgets. Immutable configuration
  reads have an explicit offline effect contract with owned value-only signatures;
  mutable containers and opaque handles are rejected. KHI v6, format 22 and runtime
  ABI v23 reject old encodings. A full isolation/ownership audit remains pending.
  Module admission and release belong to the module store, including staged and
  unreachable versions. Cleanup releases quota even after quarantine. Epoch
  reservation is separate from activation; discarded identities are never reused
  and exhaustion is rejected before installation. Source and encoded program tests
  cover forbidden host effects, initializer traps, successful pre-publication
  initialization, old-call restoration and retained old-version execution.
  Ordinary execution rejects unpublished modules at session entry, so a candidate
  handle cannot bypass effect restrictions. Candidate sessions borrow their staged
  owner; publication also rejects candidates with an active execution session.
  Session cleanup checks its own frame scopes and host resources rather than the
  runtime's aggregate call depth. Ending a suspended empty session cannot mistake
  candidate frames for leaked resources; using a suspended execution stack is an
  engine fault, and quarantine still allows all candidate/old resources to unwind.
  Heap slots record candidate allocation ownership. Candidate host arguments/results
  and script call arguments traverse their reachable graph and reject mutable
  objects allocated outside that candidate, including old objects inside fresh
  wrappers. Immutable enums are traversed by member rules. Candidate allocations
  remain usable through collection and publication; ordinary execution is unchanged.
  All mutable object access now checks candidate ownership before opening the target
  for writes. Direct array/map/set edits against preexisting objects fail without
  changing values or resource counters; candidate-owned edits remain permitted.
  Script-visible reads of external mutable objects are also rejected, including
  snapshots and struct fields. Collector tracing retains its internal access so
  old roots survive candidate collection. Host results undergo ownership checks
  before shape traversal. Module snapshots and mutable instance borrows are limited
  to the candidate program at the store boundary. Public initialization operations
  reject external instances, while existing lifecycle guards retain an internal
  failure-cleanup path. Publication independently checks every candidate module slot
  and initialization result against the candidate allocation owner after the session
  ends. Late external references inside candidate containers reject publication and
  release candidate quota without changing the active entry. Other ingress paths
  and complete checkpoint acceptance remain under audit. Embedding tests now exercise
  a diamond program whose shared dependency allocates before a later dependency
  traps: all candidate members and quota are released, unreachable candidate objects
  are collected, and repeated fresh attempts leave the old root usable. A subsequent
  successful reload initializes every member before publication; old/new root calls
  continue to observe their own dependency implementations, for source and encoded
  artifacts. Cancellation at candidate entry and instruction-budget exhaustion during
  initialization also have source/encoded coverage: both restore the old session,
  release frames, temporary roots, candidate quota and retention, and leave a later
  ordinary root and a fresh reload attempt usable without quarantining the runtime.
  Candidate handles cache terminal initialization errors across session teardown.
  Entry cancellation, cancellation observed on session exit, and budget exhaustion
  prevent both retry and publication even for modules without initializer code.
  Staged root sessions require the dedicated candidate entry; setting the phase on
  an ordinary execution request cannot bypass that lifecycle.
- R06: host functions now take a separate declaration containing nominal identity,
  typed scalar/opaque signatures, borrowing, effects, capabilities, cost and docs.
  The old metadata API, string type names and caller-chosen function fingerprints
  were removed. Offline KHI encoding is canonical and bounded; explicit registry
  linking checks complete contracts without executing callbacks. HIR print checking
  and CLI logging share the same declaration. The offline_host example exercises
  export, decode, binding verification and invocation. Scalar host function and
  module imports, aliases and qualified calls now compile from immutable offline
  declaration inputs. Lowering retains all import paths and source spans; name
  resolution diagnoses unknown paths and conflicts instead of silently discarding
  nonstandard imports. Host revisions invalidate semantic/body caches and old
  snapshots retain their original declarations, types and docs. Protocol-neutral
  callee queries return offline declarations. The offline_compile embedding
  example needs no runtime registration. Host calls require the language profile
  and cannot execute in scalar constants. Composite scalar/container declarations
  now cover Tuple, Array, Map, Set, Option and Result; HIR retains nested types and
  runtime boundaries check complete member shapes with cancellation. KHI v6 and
  artifact format 22/runtime ABI v23 reject old products; host types use bounded flat nodes
  with 4096-node and 64-depth limits. Tests cover malformed type encodings, nested
  source mismatches, wrong callback results, nested borrow escape, foreign/stale
  handles, callback GC and retained roots through source/artifact/JIT fallback.
  The offline_compile example
  compiles array contracts through a facade. Artifact and verification-metadata
  constructors are now fallible and verify before fingerprinting. In-memory loader
  checks reject malformed host types before serialization; invalid root fallback
  metadata was removed. Host field/path and trait integration
  remains outstanding; this does not mark R06 complete.
  Host type registrations now carry declaration identities independent of labels.
  Duplicate/invalid identities reject before general metadata registration, and
  linking checks nested opaque references before module publication. Calls match
  nominal root/borrow types; root handles carry registry ownership and no public
  unchecked constructor remains. Tests cover same-named declarations, renamed
  exports, failed-registration cleanup, foreign roots with coincident numeric IDs,
  borrowed type mismatches and foreign path-view chaining. The atomic_host_path
  example uses an explicit declaration identity. Portable HostTypeDeclaration now
  owns field/method identities, signatures, access, borrowing, effects and docs.
  Runtime registration consumes this definition and generates slots/fingerprints;
  the old caller-supplied runtime member/fingerprint registration model was removed.
  Batches support mutual references and publish metadata only after all types
  resolve. KHI v6 and artifact format 22 carry type declarations; linking verifies
  the complete contract and ignores documentation for ABI comparison. Offline
  queries reject stale catalog IDs, and offline_compile reads field declarations
  without a runtime. Tests cover canonical encoding, invalid member owners,
  reference closure, batch failure atomicity, generated metadata and ABI changes.
  Source host types now enter imports, annotations, call signatures and facade
  re-exports with declaration identities. HIR exposes host_type_at, preserves
  erroneous application targets and invalidates query reuse on host revision
  changes. Script structs and host types remain distinct; host generic equality
  and script construction are rejected. Generic script functions instantiate
  Host types, and public ABI carries their nominal identities. IR/bytecode use a
  separate HostHandle representation and carry the required type/member reference
  closure, including annotation-only dependencies. Unused catalog types are omitted.
  Tests cover offline queries, shadowing, source mismatches, missing declarations,
  transitive linking, callback GC and source/artifact/existing-JIT fallback call
  traces. The offline_compile example exports a facade-typed host signature without
  runtime registration. The obsolete unsupported-host-type diagnostic and duplicate
  IR host-contract validation path are removed. Host methods now derive executable
  contracts from their member declarations, with explicit nominal receivers and
  declared borrowing, effects, capabilities and resource cost. Offline analysis
  resolves receiver calls and preserves their target/docs under argument errors.
  Registration rejects missing owners or changed contracts before installing
  callbacks, and linking rejects missing methods. Lowering evaluates receivers
  before arguments once and uses existing linked host slots. Common/HIR/embedding
  tests cover corrupted contracts, offline queries, failed registration, missing
  callbacks, capability denial, exclusive receiver reentry conflicts and repeat
  execution after cleanup across source/artifact/JIT fallback routes. The
  offline_compile example compiles a declared method without runtime registration.
  Field path registration now accepts declaration identities instead of hand-written
  slots, types, permissions and member fingerprints. It resolves fields against the
  current nominal owner and generates runtime segments from declaration metadata.
  Tests reject same-named foreign fields, private/disabled fields, access escalation
  and wrong result types before publication. Existing VM, embedding and GC path
  fixtures now declare their actual fields; atomic_host_path keeps the field ID
  directly from its declaration. Host trait implementations
  and declared index/virtual paths remain pending. Whole-path fingerprints now use
  a versioned fixed-order encoding of resolved contracts, permissions and schema;
  the caller-supplied fingerprint input is removed. Types without portable host
  contracts reject registration. Tests prove unrelated runtime slot shifts and
  documentation edits preserve fingerprints, while field type, permissions,
  capabilities and schema changes alter them.
  The canonical encoder now lives in common declarations as HostPathContract;
  runtime registration no longer owns a separate encoding implementation.
  HostInterface::field_path_contract resolves offline nominal field chains with
  ownership/access checks. Tests compare offline and registered fingerprints,
  nested interface round trips and invalid chains; offline_compile generates a
  field path contract without starting a runtime. Source host index paths remain pending.
  KHI v6 now persists HostFieldPathDeclaration records with nominal field chains,
  schema, access and capabilities. Runtime field-path registration consumes these
  declarations, exported interfaces retain them, and linking rejects missing or
  ambiguous required paths. Canonical-order, duplicate, length-limit and binding
  tests plus offline_compile/atomic_host_path exercise the same declaration model.
  Format 22/runtime ABI v23 reject prior products; source host index paths are pending.
  Source host field reads now resolve a unique declared chain in HIR, preserving
  member/type facts and offline docs under missing/ambiguous-path diagnostics.
  Body reuse remaps path root IDs. IR consumes the checked root/contract and emits
  one ReadPath for nested chains; required host types and path declarations flow
  into bytecode and linking. Tests cover root-once order, capability denial, GC
  cleanup, source/artifact/JIT fallback agreement and snapshot reuse. offline_compile
  now compiles both field reads and method calls without runtime registration.
  Source field writes and compound assignments now own checked root-place/path
  facts in HIR and lower to SetPath/ModifyPath after capturing the root before RHS.
  Readonly paths and disabled path-mutation profiles reject compilation. Tests
  verify root/RHS/read/prepare/commit order, dirty old/new values, and no final write
  after division by zero, overflow or RHS target deletion; completed RHS effects
  remain. Source/artifact/JIT fallback and reused-body place-ID checks pass.
  offline_compile includes parameter-rooted assignment and compound assignment.
  Protocol-independent host field queries also consume assignment-place facts,
  including readonly-path diagnostics and reused bodies. Tests retain the old
  snapshot's queries after edits and declaration changes.
  Mixed script/host field-chain analysis now finds the first host receiver and
  resolves its complete suffix, without requiring independently declared prefix
  paths. This preserves abstract member facts; host handles remain prohibited as
  script heap payloads. Execution tests replace a local host root during RHS and
  verify that successful writes and failed updates still address the captured root
  across source bytecode, encoded artifacts and existing JIT fallback.
  IR/bytecode path records now require the contract fingerprint. Load and reload
  link module-local paths to runtime descriptors before publication, reject missing
  or ambiguous bindings and validate operand/write contracts. VM execution consumes
  immutable version bindings instead of casting PathId to a descriptor number.
  Linking also checks dynamic index argument counts and register representations
  for all four path operations; tests cover missing/extra/wrong-type arguments,
  successful bindings and rejection without module publication.
  Source-bytecode, encoded-artifact and existing JIT fallback tests bind path 0 to
  descriptor 1, ignore debug labels and preserve old bindings after registration.
  Artifact path ABI hashes exclude diagnostic labels. Format 22/runtime ABI v23
  reject previous products; source host index paths remain pending.
  Runtime path registration also rejects disconnected field owners
  and index collections before publishing or consuming a descriptor slot; tests
  cover both root and intermediate mismatches. Format 22/runtime ABI v23
  reject prior products. Later R06 checkpoints complete the source path model.
  Source facades now re-export host functions and modules using one final import
  binding table. Name resolution, imported signature/type catalogs and navigation
  share those targets; downstream facade traversal and the unsupported-host-export
  diagnostic were removed. Original source edges preserve initialization order.
  Offline query tests cover aliases, chained facades, namespace calls, shadowing,
  duplicate exports and host revision invalidation. Embedding tests verify missing
  bindings reject before publication, facade initialization runs once, and source,
  encoded artifacts and existing JIT fallback produce the same host call trace.
  The offline_compile example uses a source facade without runtime registration.
  Required host declarations now link at bytecode/artifact load and reload before
  publication, resource counters or initialization. Calls carry HostImportId and
  resolve to registry-owned slots; execution has no host-symbol fallback.
  IR/bytecode checks verify call representations and reject conflicting imports.
  Runtime scalar callback arguments/results are also checked. Later R06 work
  validates nominal opaque roots and nested callback values.
- R07/R08/R09: public function parameters/results, const types, struct fields,
  trait methods and interface targets now encode checked structural AbiType facts,
  preserving module/declaration identity and nested arguments. Generic binders
  encode owner and position; checked standard/nominal trait bounds are merged,
  sorted and deduplicated. Renaming a binder or reordering equivalent constraints
  preserves ABI. Shared IR/bytecode validation rejects foreign/free parameters,
  escaped Self, wrong nominal kinds and invalid standard-enum arity; exported
  top-level functions still require concrete signatures. Format 22/runtime ABI v23
  reject previous products. Tests cover artifact round trips, distinct same-named
  dependency types, malformed signatures and ABI rejection before reload publication
  with the old entry intact. The source_modules example inspects a dependency-owned
  public result type after encoding. Cross-module trait constraints, full semantic
  signature/layout agreement remains under R07/R08; artifact resource bounds
  are accepted under R09.
- R08/R09/R10: LoadedModule is an immutable shared Arc handle; public raw store
  loading and post-load bytecode mutation were removed. Module queries share code.
  Loaded handles and host slots reject cross-runtime use. Format v24 rejects v1–v23;
  required host fingerprints derive from declarations rather than caller options.
  The empty string host-dependency side table was removed. Struct layout handles
  retain executable versions. LoadedModule members share an Arc-owned program;
  retaining an active call on any member keeps all its module instances reachable.
  Root-version dependency pinning is covered across reload. Roots and full instance/
  code lifecycle reclamation remain open. Runtime and embedding load/reload entry
  points now accept whole programs; the old module-loading APIs were removed.
- R07: semantic Struct/Enum/Trait types use DefinitionId, and generic parameters
  use their declaring owner and position. Same-spelled cross-module types and
  shadowed generic parameters are distinct. Implicit Self types belong to a trait;
  impl checking and trait calls share one substitution operation that only replaces
  that owner's Self. Standard Option/Result types use StandardEnum identities.
  Empty constructor inference uses Unknown instead of invented generic names.
  Declaration collection precedes checking, and lowering retains the source origin;
  the origin-free AST lowering and standalone type-check API were removed. Body
  reuse checks module identity as well as declaration text. Calls infer function
  arguments structurally and check bounds; public generic functions are rejected.
  Private function templates now emit reachable concrete instances, deduplicated
  by declaration plus arguments. IR InstanceId replaces HIR IDs as execution call
  targets, and signatures/locals/temporaries use instantiated types. Static trait
  calls on existing concrete implementations use HIR implementation targets.
  Return/break/continue terminate block lowering, preventing later effects and
  unreachable generic calls from being emitted. Later R07 entries add local
  generic impl specialization, applied impl-header trait arguments and executable
  method tables; applied bounds and runtime interface dispatch remain outstanding.
  Generic struct/enum instances
  now have concrete layouts keyed by declaration and arguments; backwards
  contextual constraints for generic-call arguments
  remain outstanding. Explicit Struct constructor arguments now retain owned type
  references and share annotation resolution, arity and bound checks. Tests cover
  nested invalid container constraints, facade imports, distinct phantom layouts,
  caller-owned binders and type queries after unchanged-body reuse. Source, artifact
  and JIT fallback routes share the same concrete layouts.
  Explicit enum paths (`Token<T>::Variant`) now retain a declaration-owned type
  reference on HIR names. Unit and payload constructors share Struct annotation
  resolution and constraint validation. Wrong arity, unknown variants/types,
  invalid bounds and conflicting context reject before code generation. Tests
  cover facade imports, caller binders, distinct concrete instances, lossless
  syntax and type queries after body reuse across snapshot revisions.
  Error-bearing files no longer disable all body reuse. Unchanged functions with
  no owned diagnostics can reuse their facts while erroneous functions recheck;
  unowned or file-level diagnostics conservatively prevent reuse. Navigation tests
  distinguish enum owner, type argument and variant after payload/type errors,
  preserve old snapshots, and verify repair beside a reused correct function.
  Focused invalidation tests also verify spanless/file-level diagnostics and
  zero-width diagnostics inside a body, including the distinction between the
  affected function and an unaffected neighbor.
  Annotated locals now supply Struct type arguments, including
  phantom parameters, and propagate checked context into nested Struct fields.
  Declaration identity must match; fields and bounds retain normal validation.
  HIR rejection cases, source/artifact/JIT fallback execution and the standard-library
  example cover this inference boundary.
  Return annotations now reach tail expressions and explicit return operands;
  blocks, if/match branches, Tuple members and Array elements propagate context.
  Execution fixtures cover these routes across source/artifact/JIT fallback.
  Enum constructors now consume matching declaration-owned context too. Unit
  variants infer phantom arguments; instantiated payload types supply nested
  constructor context. Arity, payload and foreign-declaration failures reject
  codegen; source/artifact/JIT fallback exercises unit equality and nested payloads.
  Concrete local/imported function parameter types now provide constructor context
  before argument checking, including source facades. Extra arguments still get
  checked; shadowing cannot borrow another function's signature. Regressions cover
  arity/type failures and source/artifact/JIT fallback for imported constructors.
  Trait method signatures share that argument-context entry after Self substitution.
  Invalid arguments retain method targets; static trait execution tests cover
  contextual Struct/enum arguments across source/artifact/JIT fallback. Caller-owned
  generic binders now supply context too, including trait Self substitution and
  recursive calls. Same-spelled uninferred callee binders cannot supply context.
  Source/artifact/JIT fixtures cover generic forwarding and Marker<Self> arguments.
  Local generic calls now infer from expected results and pass concrete arguments
  established by preceding operands into later constructors. Source/artifact/JIT
  fixtures cover phantom function results and nested constructors. Later operands
  do not yet constrain earlier constructors; no argument is analyzed twice.
  Assignment RHS inference now consumes the checked target type. Local, field,
  array-index and enum replacement fixtures cover source/artifact/JIT fallback;
  immutable targets and incompatible payloads remain compilation errors.
  Empty Array/Map/Set constructors now consume context in expression analysis.
  The local-binding-only Map/Set type rewrite is deleted. Return, field, argument
  and assignment fixtures execute through source/artifact/JIT fallback, while
  constructor arity, container-kind and element-type mismatches still reject.
  Local container annotations now share the signature HashKey validator. Invalid
  scalar keys, nested key types and unconstrained generic keys reject in HIR;
  properly constrained generic keys remain valid.
  Partial annotations now validate known container constraints independently of
  unknown sibling types: `Map<f32, Missing>` reports both issues, while an unknown
  key alone does not invent a HashKey failure. Validation descends into key/element
  applications too. Regressions cover fields, enum payloads, parameters, returns,
  constants, locals and explicit Struct constructor arguments.
  Applied user-type bounds also check known outer shapes in partial arguments.
  An Array with an unknown element still fails HashKey/numeric bounds and meets
  Iterable. Trait implementation lookup and recursive Comparable checks defer
  while their required member identities remain unresolved; unknown leaf types
  do not produce extra constraint failures.
  Standard bound satisfaction and diagnostic reasons are now shared by calls,
  aggregate applications and operators. Comparable recursively respects caller
  bounds inside Tuple and standard-enum members in every path. Tests accept a
  constrained Tuple binder in both call and nominal arguments, reject the same
  unconstrained binder, and execute concrete layouts through all existing routes.
  Struct fields and enum payloads now update substitutions after each source-order
  member, supplying context to later nested constructors. Known member types no
  longer wait for unrelated binders. HIR and source/artifact/JIT fixtures cover
  phantom payloads, caller-owned binders and rejected member mismatches.
  Function fallthrough now uses a separate normal-completion analysis. Explicit
  returns and loops without reachable breaks no longer synthesize a Unit return;
  if/match type joins exclude branches that cannot complete. Unreachable statements
  retain diagnostics. While loops conservatively retain their zero-iteration path.
  IR joins without predecessors terminate as Unreachable, including loop exits.
  Regressions cover nested loop breaks, missing returns, mixed returning/value
  branches and source/artifact/JIT execution of explicit returns.
  Call arguments and aggregate members stop lowering once a member terminates
  control flow. Later generic calls no longer consume the instance budget, and
  terminating initializers do not emit a local store. Tests cover Tuple, Array,
  enum and Struct members, empty unreachable joins and source/artifact/JIT routes.
  Primitive operators and reflection helpers now propagate terminating operands
  too. Short-circuit joins preserve the path that skips the right operand;
  terminating while conditions and assignment RHS expressions retain their return
  instead of emitting a branch or write. Tests cover both short-circuit outcomes,
  helper suppression and unreachable generic operands across execution routes.
  Assignment target preparation now returns no location when a root or index
  terminates control flow. Recursive projections and host-root preparation propagate
  that result before evaluating further indexes, RHS expressions or writes.
  Nested-index and temporary-root regressions cover assignment and compound
  assignment, with unreachable generic instances forbidden by a zero budget.
  Match wildcard and binding patterns now share an irrefutability predicate.
  Later arms retain independent diagnostics but do not contribute result types,
  completion paths or generated instances. Named and wildcard arms share their
  IR body lowering; regressions cover shadowed generic recursion and bindings
  across source/artifact/JIT execution.
  Normal-completion queries now propagate cancellation separately from their
  boolean facts. Sequential operands are visited in source order and stop after
  termination; cancellation unwinds expression/member lists immediately instead
  of substituting a normal-completion result. Deterministic tests cover empty
  blocks, expression queries and cancellation during an operand sequence.
  Body checking now stops Tuple, Array, match-arm and call-argument traversal at
  cancellation boundaries. Contextual and ordinary argument checking share one
  loop; Array element types merge during traversal without retaining a second
  full element list. Existing partial-type recovery and query cancellation tests
  cover these shared paths.
  Standard container constraint traversal also uses an explicit stack with
  cancellation checks between type nodes. A synthetic 10,000-level Array input
  verifies traversal depth independently of parser limits, and a cancelled input
  produces no constraint diagnostics. Source-order constraint reporting is retained.
  Generic instantiation now reconstructs templates and substitution values with
  an explicit work stack. Inserted types preserve caller binders without applying
  the same substitution again. A 10,000-level template and equally deep replacement
  verify both paths; nominal-owner and ordered-member regressions remain in place.
  Trait Self substitution now shares that reconstruction walk instead of owning
  a recursive copy of every type case. Deep Self replacements preserve their
  own binder, and foreign trait owners remain untouched. Existing impl-check
  and static trait-call regressions exercise the shared operation.
  Other recursive type operations and configurable depth limits remain R15 work.
- R08: bytecode generation now requires an immutable VerifiedIrModule. The IR
  verifier checks instance identities, direct-call signatures, operand types,
  control flow, parameter layout, debug alignment, effects and definite
  initialization across branches and loop backedges. Editing IR invalidates its
  verification handle. Standard intrinsic and numeric representation contracts
  are shared with bytecode validation; intrinsic arity uses HIR declarations.
  Entry block order is preserved, and IDs are bounded before narrowing. Verification
  supports cancellation and bounds its dataflow matrix to 64 MiB. Nominal field
  layouts and scalar host signatures are checked; dynamic interface tables, precise per-point roots and the final
  linked-only runtime boundary remain outstanding.
- R15: IR generation has configurable instance, type-node, type-depth and generated
  instruction limits plus cancellation. Expansion counts nodes while copying,
  including replacement trees. Embedding returns revision-owned structured
  diagnostics and keeps checked analysis usable after failure. Parsing now has a
  configurable per-file ordinary-diagnostic budget (default 256), plus one
  structured limit error. Exhaustion stops grammar recovery while preserving
  the remaining source verbatim in a CST error node. The analysis database and
  embedding engine expose the policy, invalidate all dependent caches on changes,
  preserve existing snapshots and reject limited results before code generation.
  Tests cover nested recovery, exact/zero budgets, Unicode/CRLF losslessness,
  cancellation, equal-revision policy changes and revision-owned embed errors.
  Recursive grammar entries now have a separate configurable nesting budget
  (default 64), shared across expressions, prefixes, types, modules, import trees,
  blocks, if expressions and patterns. Exhaustion reports a positioned resource
  error and preserves the suffix; sibling entries release their budget. Deep
  2,000-level fixtures cover each recursive family, and embedding verifies
  same-revision policy changes, immutable prior facts and code-generation refusal.
  Completed CST nodes also have a configurable depth budget (default 128).
  Builder checkpoints carry subtree positions so binary/postfix wrapping counts
  depth without traversing the tree. Exhaustion stops new grammar work while
  existing ancestors close and the suffix remains lossless. Tests cover long
  fields/calls/indexes/binary/mixed chains, exact depth, sibling width and HIR
  prefix queries with compilation refusal. Embedding acceptance also runs the
  default budgets through full analysis for all ten recursive syntax families
  and a 2,000-term binary chain; the preceding correct function retains its body
  type query while both checked-program and compile entry points reject limits.
  This is default-policy evidence, not a guarantee for arbitrarily raised limits
  or externally constructed HIR.
  Const validation and evaluation now share configurable per-file step/depth
  budgets (100,000/64 by default), with one positioned exhaustion diagnostic.
  Unsupported const types also consume a root step before rejection; zero/small
  budget fixtures verify that invalid declarations cannot bypass the quota and
  exhaustion stops further constant diagnostics while good body facts survive.
  Tests cover exact/zero budgets, short-circuit work, 1,000-level dependencies,
  retained neighboring body facts, cancellation, and same-revision invalidation
  of full and single-body queries while old snapshots remain usable.
  Both const phases now access declarations through checked module-local slots,
  removing repeated whole-table scans from dependency visits and failure reporting.
  A 2,000-constant forward-reference fixture verifies all values and the precise
  owner/location of an arithmetic failure. Explicit const types are collected
  before initializer checks, fixing erroneous value-target diagnostics for forward
  annotated dependencies; inferred initializers can consume those declarations.
  Source/artifact/JIT fallback tests cover declaration order, while cycle and
  initializer errors remain rejected. This removes that scan cost; it is not
  a wall-clock quota or a measured end-to-end performance claim.
  Downstream recursive traversal limits, bounded diagnostic generation beyond
  the final result buffer, and broader resource audits remain outstanding.
  Source/artifact/JIT
  fallback fixtures cover generic values, recursion, numeric overflow, effects,
  constraints and distinct concrete types sharing a runtime representation.
  The existing native JIT still only supports zero-argument scalar entries; this
  does not claim native compilation of parameterized generic instances.
- R09: format v25 retains fixed little-endian encoding, bounded decoding and strict
  trailing-data rejection. Compatibility fingerprints use canonical serialization
  and explicit FNV-1a-64 rather than Debug; content checks cover header metadata.
  Old formats are rejected even if callers request their version. Linked module
  identity and per-table count/depth limits are covered by R09 acceptance.
  Public const ABI values use tagged scalar encoding with explicit float bits and
  UTF-8 string lengths; older Debug-based const ABI artifacts are rejected.
  Language semantics now has its own `kagari-language-v1` identity, independent
  of Rust crate versions. Legacy language identities cannot be opted into at load.
  The artifact-only module identity type was removed. Bytecode, artifact headers
  and loader metadata share the source ModuleIdentity and must agree before load.
  Runtime lookup names remain separate display/entry labels pending R10.
  Dependency fingerprints now derive from actual program members and carry typed
  ModuleIdentity; build options cannot supply them. Loader pins are optional,
  while payload agreement is always checked. Host fingerprints cover all members.
  Source and artifact loads derive the same execution-version fingerprint. Root
  verification summaries are checked against the program, including after a
  recomputed checksum. Per-member debug/function metadata stays in its module.
- R11 prerequisite for R02: the VM and standard equality assertion now call one
  script equality operation instead of Rust Value::PartialEq. Tuples/enums compare
  members, mutable objects compare identity, and unsupported categories trap.
  HeapObjectId now carries unique heap ownership, slot and generation. Collection
  reclaims unreachable slots and increments generations on reuse; foreign/stale
  handles and wrong value tags reject reads, writes, roots and script equality.
  RootedValue replaces naked root IDs; clones share retention and last drop releases
  it. Frames use registered RootSet storage with short accesses. The nonmoving
  mark-sweep baseline traces cycles and deep heap chains with an explicit work stack.
  Module state/results, debug bindings, path arguments and pending dirty records
  participate in tracing. Interpreter and scalar-JIT safepoints schedule collection;
  trap/budget exits release frame roots. Runtime callbacks are local to one thread
  and can retain explicit rooted values. The rooted_values example records pause
  data for a 10,000-object chain, recorded in [performance-baseline.md](performance-baseline.md).
  Enum nominal identity and complete interface/capture ownership remain open;
  this does not mark R11 complete. Host reentry is covered by the R12 evidence above.
- R12/R17: execution frames now carry their owning program member, and module-state
  access uses short borrows. Debug frames and breakpoints distinguish member IDs
  even when local function IDs coincide. BackendFunctionInput can only select a
  function from an immutable linked version; cross-module calls pass through the
  existing interpreter fallback. Root execution sessions now pin the dependency
  program and immutable permissions/host policy. Initialization, entry and backend
  fallback share root budgets and cancellation; nested scopes inherit them and the
  last scope releases version retention. Context resource overrides now apply per
  call without changing runtime defaults. Runtime counters remain cumulative while
  ExecutionCounters reports root usage and peaks. Cooperative cancellation and
  wall-time limits preserve completed effects and release frames; resource
  termination stays recorded until the session ends. HostCallContext now owns a
  scoped borrow guard and exposes synchronous reentry into initialized linked
  functions of the pinned program. Reentry reuses the explicit executor, validates
  argument/result representations and returns an explicit rooted result. Tests
  cover GC during nested calls, retained results, borrow conflicts and cleanup,
  swallowed termination, ordinary nested traps and rejection of other epochs.
  Direct registry invocation bypasses are removed. The session now owns one frame
  stack across interpreter, reentry and VM native scopes. Scope guards unwind only
  their own suffix after traps, cancellation or quarantine. Frames retain their
  loaded version and roots; the old VM frame type and native depth guard are gone.
  The root observer shares debugger state through short borrows, and nested pauses
  contain suspended callers at the actual call instruction. Runtime tests cover
  stack/root ownership and invariant quarantine; direct/encoded interpreter and
  JIT fallback fixtures cover nested breakpoints/traps. Host resource scopes now
  register temporary roots and object leases in the session; callback/path scopes
  release both before dropping their session handle. Borrow ownership is checked
  across runtimes and the unchecked host-frame entry is removed. Path callbacks
  receive the same checked call context for synchronous reentry before commit.
  Lexical visibility and stable debug frame identities remain R17 work.
  Tests cover direct/encoded
  programs, dependency-first initialization/failure caching, stale reloads, old
  dependency calls, malformed program rejection and interpreter/JIT fallback parity.
- R13/R17 prerequisite for R02: interpreter arithmetic, typed-path arithmetic and
  integer abs use checked operations. Existing native i32 add/subtract/multiply/
  negate check each operation, including intermediate overflow, and preserve
  structured resource/trap errors. IR records trapping arithmetic effects. Runtime
  ABI is v11 and JIT helper ABI is v5. Path arithmetic failure produces no
  commit action or dirty record. Heap allocations and standard container growth
  now share resource counters: validation, budget checks and capacity preparation
  precede mutation and accounting commit. Failed operations charge no units;
  removals/GC release live units without refunding cumulative allocation budget.
  Standard array/map removals prepare their Option result before removing an
  entry, and structured allocation errors survive builtin/VM boundaries.
  Direct/encoded programs and existing JIT fallback verify the same resource
  failures, counters and frame cleanup. Typed paths now use fallible preparation
  returning an infallible commit action; the old write/dirty callbacks are removed.
  Old-value read errors propagate. The engine reserves ledger capacity and checks
  its dirty-record quota before committing. Prepared resources drop on rejection;
  heap temporaries survive collection during preparation. Commit panics or execution
  attempts quarantine the runtime and preserve EngineFault through VM/embedding/JIT
  boundaries; cleanup releases frame roots and call depth. Direct/encoded programs
  and existing JIT fallback cover commit faults and subsequent execution rejection.
  Full engine-invariant coverage outside path commits and unified frame/host-borrow
  cleanup remain open; this does not complete R13.
  Initialization now owns a lifecycle guard and version retention with no long
  module-state borrow. Success validates the stored result; every unfinished exit
  records failure and releases retention. Failure cleanup is allowed after runtime
  quarantine, so an initializer commit fault cannot trigger a second panic while
  attempting an ordinary module-state write. Direct/encoded interpreter and JIT
  fallback fixtures cover both entry and initializer faults. Root sessions provide
  cancellation and shared budgets; synchronous callback reentry inherits them.
  Frame stacks, nested debugging and temporary host-resource ownership now share
  the session; the R12 audit is recorded above.
  Const evaluation shares checked arithmetic and
  honors short circuit, with cancellation checks. Narrower integer layouts and the other backend/
  debugger contracts remain open; compile-time quotas are still pending R15.
  Assignment lowering now retains a location before RHS execution and resolves
  its projections afterwards. Tuple updates prepare temporary values before one
  enclosing object/slot commit; shared-object ancestors are not rewritten.
  Source syntax supports `+=`, `-=`, `*=`, `/=` and computed receivers. Tuple
  member replacement requires a writable enclosing slot. Runtime mutation records,
  other mutation observations and full failure-state observation remain pending.
  Line comments are retained as CST trivia, with Unicode/CRLF coverage. The
  standard-library example now runs through the CLI as part of this validation.

Validation: workspace tests pass after the source/analysis changes. Workspace
clippy with `-D warnings` passes after correcting baseline lints and marking the
raw-pointer JIT helper's caller contract unsafe. The complete track remains open.
