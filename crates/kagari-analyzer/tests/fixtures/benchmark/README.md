# LSP benchmark inputs and process driver

`scripts/benchmark_lsp.py` runs the actual analyzer executable. The default
`--suite protocol` measures **B01.protocol**: fresh process spawn to the fully decoded initialize reply,
and initialize write to that reply. It validates UTF-16/full-sync negotiation,
server identity, null shutdown result and successful exit before retaining a
sample. Initialization is not project readiness. The implemented suites are:

| Runner/suite | Evidence |
| --- | --- |
| `benchmark_lsp.py --suite protocol` (default) | B01 protocol startup and lifecycle |
| `benchmark_lsp.py --suite diagnostics` | B01 first exact versioned diagnostic; B03 body/signature error and repair latency |
| `benchmark_lsp.py --suite diagnostic-tails` | The same diagnostic contract, with 20 edits per route and per-process nearest-rank p50/p95 |
| `benchmark_lsp.py --suite queries` | B02 warm hover/definition/implementation/completion; B03 affected/unaffected edit queries; B04 cold/warm complete implementation search; B05 fixed offered load and a bounded slow reader |
| `lsp_benchmark_memory.py` | Separate B07 acquisition/analysis component allocation, retained-source/index and externally sampled resident-memory evidence |
| `lsp_benchmark_scopes.py` | B03/B04/B07 component scope selection versus shard construction, unrelated/provider edits, retained old reads and exact source/index retirement; no actor, transport or RSS |

These are measurement methods, not claims that performance targets have passed.
The process runner marks rows outside its selected suite unavailable. B06
controlled cancellation uses the separate verification test described in the
[LSP design](../../../../../docs/lsp-design.md); it is not a process-suite metric.

From the repository root:

```text
uv run python -m unittest discover -s scripts/tests -p test_lsp_benchmark.py
uv run python scripts/benchmark_lsp.py --check --workload pilot --transport stdio
uv run python scripts/benchmark_lsp.py --check --workload pilot --transport tcp
uv run python scripts/benchmark_lsp.py --suite diagnostics --check --workload pilot --transport stdio
uv run python scripts/benchmark_lsp.py --suite diagnostics --check --workload pilot --transport tcp
uv run python scripts/benchmark_lsp.py --suite diagnostic-tails --check --workload pilot --transport stdio
uv run python scripts/benchmark_lsp.py --suite queries --check --workload pilot --transport stdio
uv run python scripts/benchmark_lsp.py --output target/lsp-benchmark/BASELINE --power-mode balanced
uv run python scripts/benchmark_lsp.py --baseline target/lsp-benchmark/BASELINE --output target/lsp-benchmark/CANDIDATE --power-mode balanced
uv run python scripts/benchmark_lsp.py --validate target/lsp-benchmark/CANDIDATE
```

Without `--candidate`, the runner builds with Cargo and obtains the executable
from `compiler-artifact` JSON; it does not trust an old target-directory binary.
Build duration is recorded separately. `--candidate PATH --profile dev` can run a
selected development image for correctness; an explicit image's profile is the
caller's assertion. Performance evidence uses `release`. Both modes copy images
into a new archive under ignored `target/`, retain exact image/input/harness/lock
hashes and source/dirty-patch identity, and never overwrite an earlier archive.
Each process receives an empty, separate generated-document cache through
`LOCALAPPDATA`/`XDG_CACHE_HOME`. Native rendering, validation and first publication
therefore belong to diagnostic startup; OS filesystem caches remain uncontrolled.
The source fingerprint describes the running checkout. With an explicit candidate,
the caller must establish its build provenance; the driver verifies image bytes.
Use `--power-mode` to record the actual machine setting; the default `unreported`
is not a power-policy measurement.

A normal run has seven fresh process groups. Baseline mode re-executes both frozen
images with alternating AB/BA order; it does not compare a fresh image to saved
historical timings. Summary rows retain group samples, median/range, paired
absolute/percentage changes and a deterministic process-group bootstrap interval.
Startup groups do not produce p95/p99. Correctness mode has one process and is
marked `correctness-only`; it still executes the selected suite's complete frozen
schedule. Its recorded durations and percentiles are not formal timing evidence.
An ambiguous interval is `inconclusive`, not a pass.
The driver refuses mismatched suites, cache policy, workloads, locks, toolchains, transports or recorded
environments. A dependency change needs an explicitly reviewed new baseline
lineage; this runner cannot label it a code-only speedup. Shared-host timing is
reporting evidence, not an automatic CI regression gate.

The independent raw peer uses bounded frames, response IDs, pending responses,
notifications and recent history. It services the explicitly supported server
requests, rejects duplicate/unsolicited replies and retains failures instead of
silently dropping them. The Rust process suite additionally samples both TCP
connection directions, malformed input and process lifecycle behavior. No
production codec is reused by these wire oracles.

The diagnostics suite currently requires `--workload pilot`. Its frozen schedule
is generated by `lsp_benchmark_diagnostics.py` and covered by the separate
`diagnostic_scenario_sha256` in `manifest.json`; the original source recipe/hashes
are unchanged. It opens the app root, API root and an unaffected CRLF module.
The app overlay appends `diagnostic-sentinel.kgr`, an unrelated, fixed return-type
error that stays present through every edit. Its positive diagnostic proves that
analysis has completed: the server's immediate stale-diagnostic clear on an edit
cannot masquerade as a completed repair.

| Step | Edit and required evidence | Metric |
| --- | --- | --- |
| Open | App version 1 makes `baseline() -> i32` return `true`; require its exact return-type error at lines 16–19 and the sentinel at lines 29–31, plus versioned empty API/unaffected publications | B01 open-to-first correct diagnostic |
| Startup repair | Restore `baseline` in app version 2 and require only the sentinel diagnostic | Preparation, retained in raw samples |
| Body error | App version 3 repeats the body-only error and requires both exact diagnostics | B03 body error |
| Body repair | Restore `baseline` in app version 4 and require only the sentinel diagnostic | B03 body repair |
| Signature error | API version 2 changes `default_value()` from `i32`/`7` to `bool`/`true`; require a clean API publication and both exact app diagnostics at unchanged app version 4 | B03 signature error |
| Signature repair | Restore API version 3; require its clean publication and only the sentinel at app version 4 | B03 signature repair |

Line/character coordinates in the oracle are zero-based UTF-16. Every error must
match code, severity, source, full message and range; additional errors fail the
witness; diagnostic array ordering is immaterial. Empty startup or edit-time
clears cannot count as completed analysis. Missing notifications
time out after 30 seconds and fail the run. The unaffected module must retain its
accepted empty version 1 state; identical publications may be deduplicated, so the
driver audits all received notifications without demanding redundant messages.

Raw samples retain each edit's content hash, before-write/after-decode monotonic
timestamps, exact observed notifications and bounded wire history. The archive
also retains all edit texts in `edit-inputs.json`; validation rechecks original
source, edit and executable bytes. Primary latency includes the complete 150 ms
debounce policy, acquisition, scheduling, analysis and delivery. The policy is
metadata only; the driver does not subtract it or label a derived remainder as
measured analysis time. One operation per process group yields median/range,
not query-like tail percentiles. No production measurement RPC is introduced.

For diagnostic timing, freeze the implementation first and add `--suite diagnostics`
to both baseline and candidate commands. Protocol and diagnostic archives are
separate lineages. Reports predating explicit suite/cache metadata need a new
baseline archive; existing files are never rewritten into comparable evidence.

`--suite diagnostic-tails` retains the same startup/error oracles and repeats all
four error/repair routes twenty times per process. It derives nearest-rank p50/p95
for each route from those observations, then summarizes the seven process groups
by median/range. Startup still has only one observation per process. The separate
`diagnostic_tail_scenario_sha256` freezes this schedule. Choose the same suite for
both sides of a comparison; a single-cycle diagnostic archive is not a tail-suite
baseline.

## Query and implementation-index process suite

The query suite also requires `--workload pilot`. Its
`query_scenario_sha256` freezes semantic sites, expected answers and the operation
schedule. After exact diagnostic readiness it performs one cold implementation
search, then twenty warmups and two hundred measured requests for each of hover,
definition, implementation and completion. Every answer is checked independently:
types, destination URIs/ranges, complete implementation coverage and plain-text
completion replacement edits. Per-method p50/p95 use nearest-rank observations
inside each process, not percentiles of seven group medians.

The offered-load routes each send forty hover/completion requests at 40 ms
intervals, edit every fourth offer and run an implementation query alongside
them. The slow route delays the client reader by 40 ms per decoded frame. Raw
records retain scheduled/written/received times, admitted document versions,
semantic answers, result hashes and canonical JSON payload sizes. Client pending
IDs include already-decoded responses; they are not server queue depth. Server
admission time and internal queue latency are not measured. The final four edit
routes check affected and unaffected hover results, cross-file definition and
the exact diagnostic set after body/signature changes and repairs.

For a timing campaign, use `--suite queries` on both baseline and candidate
commands above, without `--check`. Wire timings establish observable latency and
answers; selected-body work counters remain separate verification evidence and
are not inferred from a fast response or a cached snapshot's provenance.

## Controlled running cancellation

Build before sampling; use the Cargo-reported release test executable for seven
fresh process groups. This focused command is the corresponding single-group
entrypoint (PowerShell):

```powershell
$env:KAGARI_LSP_CANCEL_SAMPLES = '20'
cargo test --locked -p kagari-analyzer --lib --features verification --release tests::features::cancellation::running_selected_body_cancellation_reports_terminal_before_worker_retirement -- --exact --nocapture
```

Copy `target/lsp-benchmark/B06-running-cancel.json` after each process into a new
archive before the next group overwrites that scratch report. Retain executable,
source, toolchain, environment and raw sample metadata alongside each copy. The
test defaults to one sample for ordinary correctness feedback; only groups with
at least twenty samples emit nearest-rank p50/p95.

A real selected-body entry gate holds the worker while the client cancels. The
terminal must be `RequestCancelled` while its worker still owns the credit; release
then proves matching completion and retirement. All durations use one monotonic
clock. Client cancel-to-credit completion is an upper bound on server retirement
and includes the controlled pause; post-release retirement is separate. This is
controlled cancellation evidence, with no unpaused or large-body intraloop p95
claim. Queued cancellation and an actual canceled index continuation remain T1
correctness witnesses rather than fabricated timing samples.

## Memory component runner

From the repository root:

```text
uv run python scripts/lsp_benchmark_memory.py --output target/lsp-benchmark/MEMORY --power-mode balanced
```

This separate runner accepts only `--candidate PATH`, required `--output PATH`
and `--power-mode VALUE`; it has no `--check`, `--baseline`, suite or transport
option. Without a candidate it builds the release `lsp_memory` bench with the
`verification` feature and selects Cargo's reported artifact. An explicit
candidate must be that release/verification bench executable, not the analyzer.
The output must be a new directory under `target/`. Build time, image hash,
source/lock identity, machine and cache policy are recorded separately.

One fresh process exercises one thousand edits using the fixed
`analysis/retention-app.kgr` and `analysis/retention-api.kgr` fixtures, with
signature changes, provider replacement, close/reopen and retained old snapshots.
Six owned checkpoints cover the prepared native baseline, edits 100/500/1,000
and retirement. The allocator reports requested/freed/live layout bytes and peak
live bytes; source-content leases and compact index rows have distinct counters.
The external sampler adds Windows working set/OS peak/private commit or Linux
RSS/high-water measurements. These quantities are not interchangeable, and
checkpoint sampling is not a continuous resident-memory trace. The probe runs
acquisition and analysis components without the actor runtime or LSP transport.
Its pauses and I/O are not latency evidence; one group does not establish a
paired regression, speedup or performance-target acceptance.

The independent Node transcript uses Microsoft's pinned
`vscode-languageserver-protocol/node` directly, with no VS Code host or mocked
`vscode` module. After building the analyzer and installing editor dependencies,
run from the repository root in PowerShell:

```powershell
$env:KAGARI_TEST_ANALYZER = (Resolve-Path target/debug/kagari-analyzer.exe).Path
node --test editors/tests/lsp.test.mjs
```

An absent explicit executable fails instead of silently skipping. Both stdio and
TCP must negotiate full synchronization, process ordered document notifications,
reject unsupported requests, and exit after shutdown with input still open.
The stdio route additionally checks semantic queries and versioned diagnostic
error/repair behavior against the shared editor fixture.
All waits have deadlines and cleanup owns only the spawned analyzer process.

`app.kgr` and `api.kgr` are fixed semantic cores. The generator in
`scripts/lsp_benchmark_inputs.py` adds real public function chains and call sites,
not irrelevant text padding. `manifest.json` freezes canonical manifest hashes
and every generated source/manifest hash. The shapes are:

| Shape | Packages | Physical user files | Generated function bodies | Schedule |
| --- | ---: | ---: | ---: | --- |
| pilot | 2 | 20 | 1,980 | Fresh startup; later semantic rows use the shared fixed sites |
| workspace | 4 | 200 | 19,600 | Larger fan-out with the same generator |
| churn | 2 | 20 | 1,980 | 1,000 fixed-size literal edits; checkpoints 100/500/1,000 |

The exact byte/line counts and generated call-reference count are in each
materialized `inputs.json`; the manifest hash also covers expected answers and
operation schedules. One generated file deliberately uses CRLF; the fixed source
includes Unicode, aliases, trait overrides/defaults and generic implementations.
The initial workspace is not permanently broken by an incomplete member-access
witness. B01.protocol does not open/execute semantic schedules; each semantic
suite applies only its own frozen schedule. The separate memory runner uses its
smaller retention fixtures, not the generated `churn` shape.
The generated pilot passed `kg check --manifest-path` when
introduced, and both fixed files pass `kg fmt --check`.

Changing a fixture, recipe or expected answer must deliberately regenerate and
review the frozen manifest. A mismatch fails before timing. Temporary generated
inputs, process logs, images and reports stay under `target/`; only reproducible
methods and interpreted evidence belong in tracked documentation.

## Acquisition component observation

LSP01 adds one ignored Rust measurement using the same frozen pilot, the actual
bounded acquisition port and its dedicated worker. It measures submit-to-capture
completion while alternating an unsaved dependency body's literal. Every result
must contain the expected overlay, two isolated package contexts and twenty
physical source files. This covers discovery, package scopes, source registration
and retained origins; it does not measure HIR analysis, diagnostics or protocol
readiness and does not create a production benchmark RPC.

Prepare a fresh, hash-validated input directory and run from the repository root:

```powershell
$env:KAGARI_LSP_ACQUISITION_BENCH = Join-Path (Get-Location) ('target/lsp-acquisition-' + [guid]::NewGuid().ToString('N'))
@'
from pathlib import Path
import hashlib, json, os, sys
sys.path.insert(0, str(Path.cwd() / 'scripts'))
from lsp_benchmark_inputs import materialize, object_digest
root = Path(os.environ['KAGARI_LSP_ACQUISITION_BENCH'])
value = materialize('pilot', root)
for name, expected in value['file_sha256'].items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == expected
metadata = {'manifest_sha256': object_digest(value)}
(root / 'measurement-manifest.json').write_text(json.dumps(metadata), encoding='utf-8')
'@ | uv run python -
if ($LASTEXITCODE -ne 0) { throw 'Frozen input preparation failed' }
cargo test --locked -p kagari-analyzer --lib workspace::tests::benchmark::acquisition_frozen_pilot -- --ignored --nocapture
```

This command deliberately records the workspace test profile (development
optimization), with build time excluded. Each of seven sequential groups owns a
fresh worker/database, performs twenty warmups and reports nearest-rank p50/p95
from two hundred captures. The OS file cache is warm after materialization; the
first capture is only cold with respect to the worker's source databases. Report
machine, toolchain, power policy and overlapping work alongside the JSON output.
The maximum charged-input byte value samples completed captures, not in-flight
peak memory, allocator usage or RSS. Worker shutdown must release all charged
input leases. These initial component observations are not release performance
gates or evidence of a speedup; later comparisons must keep profile, inputs and
sampling consistent.
