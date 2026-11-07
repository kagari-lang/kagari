# LSP benchmark inputs and process driver

`scripts/benchmark_lsp.py` runs the actual analyzer executable. Its initial LSP00
row is **B01.protocol**: fresh process spawn to the fully decoded initialize reply,
and initialize write to that reply. It validates UTF-16/full-sync negotiation,
server identity, null shutdown result and successful exit before retaining a
sample. Initialization is not project readiness. Diagnostic startup and B02-B08
remain explicitly unavailable until their owning implementation phases add them.
The driver does not report semantic, cancellation, indexing or memory acceptance.

From the repository root:

```text
uv run python -m unittest discover -s scripts/tests -p test_lsp_benchmark.py
uv run python scripts/benchmark_lsp.py --check --workload pilot --transport stdio
uv run python scripts/benchmark_lsp.py --check --workload pilot --transport tcp
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
Use `--power-mode` to record the actual machine setting; the default `unreported`
is not a power-policy measurement.

A normal run has seven fresh process groups. Baseline mode re-executes both frozen
images with alternating AB/BA order; it does not compare a fresh image to saved
historical timings. Summary rows retain group samples, median/range, paired
absolute/percentage changes and a deterministic process-group bootstrap interval.
Startup groups do not produce p95/p99. Correctness mode has one process and is
marked `correctness-only`. An ambiguous interval is `inconclusive`, not a pass.
The driver refuses mismatched workloads, locks, toolchains, transports or recorded
environments. A dependency change needs an explicitly reviewed new baseline
lineage; this runner cannot label it a code-only speedup. Shared-host timing is
reporting evidence, not an automatic CI regression gate.

The independent raw peer uses bounded frames, response IDs, pending responses,
notifications and recent history. It services the explicitly supported server
requests, rejects duplicate/unsolicited replies and retains failures instead of
silently dropping them. The Rust process suite additionally samples both TCP
connection directions, malformed input and process lifecycle behavior. No
production codec is reused by these wire oracles.

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
The incomplete member-access witness is an edit operation, not a permanently
broken initial workspace. Current B01.protocol does not open/execute those
semantic schedules. The generated pilot passed `kg check --manifest-path` when
introduced, and both fixed files pass `kg fmt --check`.

Changing a fixture, recipe or expected answer must deliberately regenerate and
review the frozen manifest. A mismatch fails before timing. Temporary generated
inputs, process logs, images and reports stay under `target/`; only reproducible
methods and interpreted evidence belong in tracked documentation.
