# Frontend benchmarks

This guide owns the current AST and HIR benchmark procedure. Recorded observations
belong in [Performance Measurements](performance-baseline.md); they apply only to
their recorded images, inputs and environment. The runners validate evidence and
report measurements, not performance acceptance budgets. Historical Rowan migration
thresholds are not current policy.

Run commands from the repository root. Use workspace release profiles, default
Cargo parallelism and an otherwise idle machine for performance campaigns. Keep
build time separate from measured operations. Do not run builds, tests or other
benchmarks concurrently with timing; record machine/toolchain, features, environment,
cache state and workload alongside any result. These are manual campaigns, not the
small correctness checks used for routine local validation (see [testing](testing.md#frontend-benchmark-checks)).

The `kagari-hir` targets `ast_baseline`, `hir_baseline` and `hir_memory` are explicit
Cargo `[[bench]]` targets with `harness = false` and `test = false`. Their custom
drivers accept Cargo's injected `--bench` argument.

## AST: neutral same-lock comparisons

The [runner](../scripts/benchmark_ast.py) uses the
[AST driver](../crates/kagari-hir/benches/ast_baseline/main.rs) and fixed
[workload recipes](../crates/kagari-hir/benches/ast_baseline/workloads.rs).
By default, it builds with
`cargo build --release --locked -p kagari-hir --bench ast_baseline` and discovers
the executable from Cargo's artifact messages:

```text
uv run python scripts/benchmark_ast.py
```

Bench executable paths are hashed; the runner never assumes a fixed examples output
path. `--executable PATH` retains the explicit-executable bypass: it skips the build
and artifact discovery. `--runs N` selects a positive number of process groups
(default seven). A candidate-only run characterizes that image without a
comparison claim. To compare a later build, pass the earlier run's archived
executable to `--baseline-executable PATH`, with its sibling `metadata.json` intact.
The default run builds the candidate; rebuild it yourself when using `--executable`.
Keep the baseline archive unchanged.

Comparisons require identical `Cargo.lock` SHA-256, toolchain and recorded
environment/configuration, plus the frozen baseline image hash. Dependency changes
are unsupported: there is no Rowan-removal exception, `--confirm` mode, migration
budget or acceptance report. Changed workloads/evidence and invalid accounting are
still errors; absence of a performance gate does not relax correctness checks.

Each process runs `small`, `medium`, `large`, `trivia`, `syntax_corpus`, `recovery`,
`limits` and `declarations`. Every timing stage has three warmup batches and eleven
measured batches, with calibrated iteration counts. The runner summarizes process
medians and ranges; paired runs alternate candidate/baseline order and report the
median of per-pair candidate/baseline ratios, not a ratio of pooled operation samples.

- `lex_drop` measures lexing and teardown; `parse_drop` measures fresh parsing,
  including lexing and final destruction.
- `parse_and_drop_split` separates construction and destruction with extra timer
  reads. It is diagnostic, not a replacement for the complete parse lifecycle.
- The first five workloads also measure `parse_hir_drop` and `field_read`.
  `parse_hir_drop` calls `lower_module`: parsing, structural root-module HIR,
  source maps and teardown, **not** name/type checking, complete inline/package
  preparation or executable lowering. `field_read` reads preselected expression
  fields; node discovery is excluded, so it is not complete AST traversal.

Input loading, source/line-index construction and correctness walks are outside
the timed operations. Do not subtract independently measured lexer/parse/HIR rows
to manufacture isolated phase costs.

Allocation diagnostics run separately with `--memory`; counting is inactive during
AST timing. The [memory probe](../crates/kagari-hir/benches/ast_baseline/memory.rs)
checks four equal lifecycles, including the extra `inline_retention` workload. Its
`parsed`, `with_view`, `parse_released` and `released` checkpoints describe parser
and independently retained view ownership, excluding pre-existing source backing.
Final parser/view live bytes must be zero. Requested bytes are not RSS.

Runs are archived under timestamped `target/ast-baseline/` directories: candidate
image, exact lockfile, harness copies/hashes, input text/hashes, metadata, raw timing
and memory reports, `summary.json` and `memory-summary.json`. A paired archive embeds
baseline metadata but does not copy the baseline executable again; retain the
original archive. AST corpus loading uses a checkout path compiled into each image,
so retain that checkout and its fixtures too. The runner checks image hashes and
workload identity, but an archived binary alone is not a self-contained corpus or
proof of which source/configuration built it. Build with the recorded defaults and
preserve provenance; do not point at an unrelated stale executable.

## HIR commands and defaults

The [HIR runner](../scripts/benchmark_hir.py) builds both release bench targets with
`cargo build --release --locked -p kagari-hir --bench hir_baseline --bench hir_memory`,
discovers their paths from Cargo's artifact messages, archives them and executes
only the frozen copies. `hir_baseline` is the timing target and has no
allocator wrapper. `hir_memory` is the separate System-allocator-instrumented target;
its durations are never speed evidence. Selecting `--mode timing` or `--mode memory`
limits measurements, not the two-target build.

```text
uv run python scripts/benchmark_hir.py --check --workload plain --size 96
uv run python scripts/benchmark_hir.py --output target/hir-before
```

The first command is a small correctness check; the second is a full manual campaign.
Output directories must be new directories under `target/` and are never overwritten.
Without `--output`, the runner uses a timestamped `target/hir-benchmark/` directory.

| Option | Default and scope |
| --- | --- |
| `--workload` | All of `plain`, `generic`, `dependency_edit`; repeat to select families. |
| `--size` | Timing: 24, 96 and 384 for plain/generic; dependency edits: only 96. Memory and `--check`: 96. Explicit 24/384 requires excluding `dependency_edit`. |
| `--mode` | `both`; alternatives are `timing` and `memory`. Memory defaults to representative size 96; an explicit other size is labeled nonrepresentative. |
| `--runs` | Seven independent process samples per case; positive overrides allowed. `--check` uses one. |
| `--timeout` | 600 seconds per subprocess, including the build, not a total campaign timeout. |
| `--check` | No warmup or performance summary/comparison; cannot combine with `--baseline` or `--compare`. |

For direct driver inspection, both executables accept `--workload`, `--size`,
`--inputs` and `--check`; their defaults are **plain/96**, not the Python runner's
full campaign. `--inputs` prints a manifest without executing analysis. A timing
process performs one discarded suite warmup, then one measured suite; every route
gets fresh databases. Memory and correctness processes have no warmup. HIR samples
are operation blocks across fresh processes, not AST-style calibrated batches or
individual-operation latency percentiles.

## HIR workloads and query routes

The deterministic [workloads](../crates/kagari-hir/benches/hir_baseline/workloads.rs)
use real shared standard-library declarations plus generated user sources:

- `plain`: one physical file of arithmetic-chain functions and an explicit root
  `main`.
- `generic`: the same scales, with a trait, implementation and generic helper calls.
- `dependency_edit`: five physical files/logical modules, no inline modules: root,
  shared provider, two consumers and an independent module. Size 96 counts the
  shared function plus 32/32/31 functions; root `main` is additional.

Manifest source bytes cover original physical **user** sources, excluding provider
sources and edit revisions. The input hash includes length-framed filenames,
original and edited text, generation recipe and route names. Read the manifest
rather than treating the size label as bytes or total provider-inclusive functions.

[Timing routes](../crates/kagari-hir/benches/hir_baseline/timing.rs) keep different
public entry points independent; see [HIR query ownership](architecture/hir.md#query-work-reuse-and-publication).

| Route | Measured work and preparation |
| --- | --- |
| `staged` | Install providers, then declarations, signatures after declarations, snapshot after signatures, `check_program`, result release and database release on one database. Later phases benefit from earlier preparation. |
| `snapshot` | Direct cold-database snapshot, without preliminary declaration/signature queries; install and teardown are separate. |
| `checked` | Independent direct snapshot plus `check_program` and teardown. The check validates the reachable checked closure, not execution or code generation. |
| `warm` | Preparation snapshot, then an unchanged snapshot on the same database with the first result retained; report file-result sharing separately. |
| `body_edit` / `signature_edit` | Retain old snapshot, update source, query edited snapshot, check the new program and release results/database. Preparation and source edit are separate metrics. |
| `selected` | Declaration-only preparation and owner lookup, then `body` for `consumer_a::f0`. No full snapshot prewarms this query. |
| `selected_warm` / `selected_signature_edit` | Prepare the selected result, then query it unchanged or after the dependency signature edit, retaining the first result. Full snapshot/program validation happens only after the selected measurements. |
| `lifecycle` | Install, old snapshot/program check, body edit, new snapshot/program check, release old aliases, release new aliases, release analysis database. |

All timing sizes include staged, snapshot, checked and warm routes. At size 96,
plain adds body edit and lifecycle, generic adds lifecycle, and dependency edits
add both edits, all three selected routes and lifecycle. Memory measures only the
lifecycle route. This is a bounded workload set, not an exhaustive invalidation or
language-feature matrix.

For example, changing `shared::value` from `i32` to `i64` must make both consumers'
`saved` expression become `i64`, while the independent module stays `i32` and the
retained old result still reports `i32`. A body-only edit changes the literal from
1 to 9 without changing the signature; plain/generic lifecycle edits change the
first `x + 0` to `x + 9`. Assertions check actual source/revision changes, diagnostics,
function/module counts, representative types, edited lowered literals and checked
scalar facts, and old-result validity. These are
analysis benchmarks; they do not execute the changed arithmetic.

Selected-result construction counts are provenance of the returned result, not
necessarily work done by the current query: a shared result can retain its earlier
counts. Reuse observations remain in raw reports but are not cross-image semantic
equality requirements in the [result validator](../scripts/hir_benchmark_results.py).

## Timing, retention and cache interpretation

Shared catalog initialization is reported separately as `provider_setup_ns`.
Source database setup and empty analysis database creation precede measured query
blocks; provider installation into each database is measured separately. Correctness
walks and JSON reporting are outside timing blocks. Cold means **fresh analysis
database**, not cold OS caches: timing processes warm code/allocator state, the
catalog is shared within the process, and there is no OS cache flushing.

`lifecycle/measured_sum_ns` is the sum of nine measured operation blocks, **not
contiguous wall time**. Validation runs between blocks. Likewise, the staged route
is conditioned on its earlier phases, not a decomposition of an independently timed
direct snapshot. Do not sum medians or subtract independent routes to claim an
end-to-end latency or isolated checker cost.

The [shared lifecycle](../crates/kagari-hir/benches/hir_baseline/scenarios.rs) keeps
old/new snapshots and checked programs together, then drops all explicit old aliases,
all explicit new aliases, and finally the analysis database. Its memory checkpoints
are `baseline`, `old_snapshot`, `both_snapshots`, `old_released`, `new_released` and
`database_released`. Database caches can retain facts after callers release their
results; a small result-drop delta does not prove those facts have been freed.

The [allocation target](../crates/kagari-hir/benches/hir_baseline/memory.rs) records
process-cumulative successful requests, requested bytes, frees, freed bytes and live
bytes. Successful reallocations count a new request and an old-layout free, including
in-place reallocations. Only the peak window resets, to baseline live bytes; live and
cumulative counters do not reset. Summaries include raw counters and within-process
baseline deltas. Those deltas are not candidate-minus-baseline-image differences.
These are **requested allocator bytes/counts, not RSS**, allocator overhead or a
speed proxy. Peaks track logical requested-live bytes, not temporary backing-storage
overlap inside `System.realloc`; this synchronous probe is not a concurrent heap profiler. Validation allocations can enter the counters; JSON/report allocation
starts after the last checkpoint.

Source backing, the shared catalog and process facilities remain alive through
`database_released`. Residual live bytes or baseline deltas therefore need not be
zero and are not inherently a leak. Unlike the narrower AST parser/view window,
this checkpoint does not mean every process owner has been released.

## HIR archives and comparisons

The Windows 2026-11-07 baseline was recorded before relocation from `examples/` to
`benches/`. Changed harness paths, tooling and target fingerprints invalidate those
old HIR runner archives for current comparisons. Retain their historical results;
regenerate a baseline only for the next performance campaign, with no compatibility
reader or full rerun for relocation alone.

After freezing a baseline, make the intended production change without changing
the dependency lock, harness, inputs or measurement environment, then run:

```text
uv run python scripts/benchmark_hir.py --baseline target/hir-before --output target/hir-after
```

This builds the candidate, copies both images from the frozen baseline archive and
runs fresh serial AB/BA pairs for each case (A = baseline, B = candidate). Match the
baseline's workload/size/mode selection explicitly if it was not the default. Timing
ratios are candidate/baseline per process pair; lower is faster. The old saved timings
are not substituted for freshly executed baseline samples.

To inspect already saved campaigns without building or executing either image:

```text
uv run python scripts/benchmark_hir.py --compare target/hir-before target/hir-after
```

This compares the candidate samples from each saved campaign as **unpaired medians**,
even if a campaign originally contained pairs. It is not fresh paired evidence.
Selection/sample options do not apply to `--compare`; it uses saved cases and modes.

Both paths require matching machine/toolchain/environment, resolved build profiles
and features, Cargo configuration fingerprints, lockfile, harness/tooling hashes,
sample protocol and case inventory. Manifests and substantive semantic evidence
must agree. Production source hashes may differ, but source/toolchain/environment
changes during a campaign are rejected. Invalid, incomplete or mutated archives
are not performance evidence.

Archives retain both target images with SHA-256, source fingerprints before/after,
Git revision/status and dirty-diff hash, harness/tooling fingerprints, Cargo artifact
configuration/build output and duration, manifests, raw reports, per-process samples,
`run.json` and (except for checks) `summary.json`. Fingerprints are provenance, not a
source backup: preserve the actual patch for dirty/untracked source. Keep the entire
archive intact, including images, even for saved-only comparison.

Summaries retain raw vectors, median, median absolute deviation (MAD), min/max and,
for paired timing, ratio vectors/statistics. Fewer than five process samples and
MAD above 10% of a timing median produce descriptive/noise warnings, not acceptance
thresholds; zero baseline durations make ratios undefined. Memory has no timing
ratio. Investigate noise before making small-difference claims. Preserve conclusions,
method and limits in tracked documentation; ignored archives are supporting evidence,
not the only durable record. A correctness check or a completed campaign does not
establish a speedup, a memory budget pass or exhaustive CI coverage.
