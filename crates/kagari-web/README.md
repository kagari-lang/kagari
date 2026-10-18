# Kagari browser adapter

`kagari-web` builds an actual `cdylib` plus a native-testable `rlib`. Only
`wasm32-unknown-unknown` exports wasm-bindgen bindings. Default `source` forwards
`kagari/source`; `--no-default-features` has no compiler/frontend and **no JS
`compile` method**. Both use `kagari` with defaults disabled and `web` enabled,
validated bytecode, `PreparedProgram`, and SDK owned interpreter execution.

## Platform model and references

The reviewed baseline is Rust 1.99.0 with Cargo resolver 3. Follow
[Cargo's additive feature model](https://doc.rust-lang.org/cargo/reference/features.html#feature-unification),
not separate desktop/browser language semantics. Rust
[machine-sized integers](https://doc.rust-lang.org/reference/types/numeric.html)
remain suitable for internal storage; script `u64` counters use checked storage
boundaries. WASM [i64 values](https://webassembly.github.io/spec/core/syntax/types.html)
do not require 64-bit pointers. There are no script `isize`/`usize` aliases.

The [Rust target documentation](https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html)
does not promise working filesystems, threads or clocks just because std compiles.
Use [web-time 1.1.0](https://docs.rs/web-time/1.1.0/web_time/) for measurement and
[wasm-bindgen](https://wasm-bindgen.github.io/wasm-bindgen/examples/hello-world.html)
for the maintained Rust/JS ABI (crate/CLI 0.2.129). Native/AOT loading, WASI,
threads/shared memory, arbitrary JS callbacks and external async providers are not
browser-adapter capabilities. Source input is a single virtual module, not browser
filesystem/package acquisition.

[Worker termination](https://developer.mozilla.org/en-US/docs/Web/API/Worker/terminate)
is immediate, not graceful cancellation. Keep Kagari's owned-execution cleanup,
roots and generation checks for normal exits, and discard fatal instances instead
of inventing panic recovery. These references justify ownership mechanisms, not
speed claims or weakened script/host contracts.

## JavaScript API and protocol v1

After `await init(...)` from the generated ES module:

| Call | Return / behavior |
| --- | --- |
| `new BrowserSession()` | Installs the standard library and `web::print(String)`. Construction failure throws a JSON response string. |
| `compile(source: string): string` | Source feature only. Compile `memory://main.kgr`, validate, load; status `ready` or `error`. |
| `load_artifact(bytes: Uint8Array): string` | Decode, validate, load; status `ready` or `error`. |
| `start(): string` | Queue zero-argument `ProgramEntry::Main`; status `runnable` or `error`. No body executes yet. |
| `drive(slice: number): string` | Integer `u32`, 1 through 100000; status `runnable`, `waiting`, `complete`, or `error`. |
| `cancel(): void` | Request cancellation; keep driving to receive `KG_RUNTIME_CANCELLED`. Idle is a no-op. |
| `reset(): string` | Cancel/drop the normal owner, drain retired executions, release loaded runtime; status `idle` or cleanup `error`. |
| `drain_output(): string` | Drain without driving; status `output`. |
| `free(): void` | wasm-bindgen destructor; retires normal owners but cannot report cleanup errors. Prefer explicit `reset()` first. |

All returned strings parse as the same JSON envelope in both feature modes:

```json
{"version":1,"status":"complete","output":["hello 😀"],"output_truncated":false,"result":{"tag":"u64","value":"18446744073709551615"},"error":null,"diagnostics":[],"diagnostics_truncated":false}
```

Every JSON response drains pending output, including error/reset responses.
`cancel()` does not drain. Each output element is exactly one `web::print` call
(no newline is added). There is no arbitrary JS callback registration: the Rust
callback copies bounded text into an `Arc<Mutex<...>>` queue; JS sees it only after
runtime borrows end. For example:

```text
use web::print;
fn main() -> u64 {
    print("hello 😀");
    18446744073709551615u64
}
```

`result` is non-null only on successful completion. Tags:

- `unit`: no additional fields; `bool`: boolean `value`.
- `i32`, `i64`, `u64`: decimal **string** `value`, never JS Number.
- `f32`, `f64`: string `value`; Rust's shortest display, including `NaN`, `inf`,
  `-inf` and `-0`. Consumers choose whether/how to turn these into JS numbers.
- `string`: string `value`, boolean `truncated`.
- `opaque`: `kind` only (`tuple`, `range`, `array`, `map`, `set`, `enum`, `struct`,
  `gc_handle`, `interface`, `closure`, `cell`, `host_root`, `host_path_view`,
  `ephemeral`). No graph traversal, heap addresses or retained JS object handles.

Values remain rooted through conversion, then the root and terminal execution
owner are released. No recursive serialization or script formatting methods run.

`error` is null or `{code, severity:"error", message, truncated}`. SDK errors retain
SDK codes; adapter errors use `WEB_*` (e.g. `WEB_BUSY`, `WEB_SOURCE_LIMIT`,
`WEB_ARTIFACT_DECODE`, `WEB_ARTIFACT_VALIDATION`, `WEB_NO_PROGRAM`, `WEB_NO_RUN`,
`WEB_SLICE_LIMIT`). An unsuccessful compile/load leaves the previous program intact.
An active run rejects compile/load/start rather than replacing or running concurrently.
Terminal script errors release the active owner; driver API errors retain it for reset.

Compilation failures also expose `diagnostics`: bounded records with `code`,
`severity` (`error`/`warning`), `message`, `location`, `labels`, `notes`, `truncated`.
A location is `{origin, revision, byte_start, byte_end, start, end}`. `revision` is a
decimal string; each position is `{offset, line, character}`. Byte ranges are UTF-8,
end-exclusive; offsets/columns are UTF-16 and all positions are zero-based.
`origin` comes from the retained source snapshot, not a guessed file name. Unknown,
stale or unrepresentable locations are null. CRLF counts as two absolute UTF-16
units; the LF inside CRLF maps to the preceding line's end column. Labels have
`{location, message}`. For `// 😀\r\nfn main() -> i32 { missing }`, the missing-name
start is byte 28, UTF-16 offset 26, line 1, character 19.

## Limits and lifecycle

- Source: reject above 256 KiB UTF-8; artifact input: reject above 16 MiB.
- Output per drain: at most 256 messages / 64 KiB UTF-8 total, 4 KiB per message.
  Excess is discarded with `output_truncated:true`; UTF-8 characters are not split.
- Result strings: 16 KiB UTF-8; diagnostic/error text: 4 KiB per field, codes: 128
  bytes. At most 32 diagnostics, each with 8 labels and 8 notes. Truncation is visible.
- No adapter work queue: calls are synchronous, with one active run. The Worker
  must bound its inbound queue and cumulative displayed output; drain limits are
  not a total-run output quota. Reject invalid JS slice numbers before calling:
  wasm-bindgen's `u32` ABI coerces JS numbers, it does not validate their integrality.
- Fresh source sessions bound retained source history. Input/response limits are
  not hard compiler/heap quotas (the JS-to-WASM copy also precedes Rust admission).
- Yield `runnable` through later browser tasks, not an endless Promise microtask
  chain. No external async providers are installed; `waiting` is reported without
  a JS wake callback. Cancellation/reset remains available; future async providers
  need an owned wake protocol rather than busy polling or reentrant JS callbacks.
- Reset is normal SDK cancellation/retirement, **not** panic recovery. Rust panic,
  fatal WASM fault or unrecoverable allocation failure requires terminating and
  replacing the Worker/instance. Hard termination does not run cleanup or roll back
  completed effects. Compilation and long native operations are not preemptible.
- Runtime measurements use locked `web-time 1.1.0` and require browser
  `Performance.now()`. Clock precision/sleep behavior is browser-dependent; script
  deterministic time/random inputs remain SDK defaults, not ambient JS inputs.

## Worker client

[`PlaygroundClient`](../../site/src/playground/client.js) owns a module Worker;
construction alone performs no download. Deploy `client.js`, `protocol.js` and
`worker.js` together under `wasm/playground/`, beside `wasm/source/` bindings.
Trusted application code may override `workerURL` and `productURL`.

- `run(source)` rejects invalid/busy admission synchronously, otherwise returns a
  terminal-event Promise. There is one run and no queued work.
- `cancel()` requests cooperative cancellation once. `reset()` coalesces requests,
  waits for cancellation/cleanup, then returns idle. `hardStop()` destroys and
  replaces the Worker immediately; `dispose()` destroys it permanently.
- `onEvent` receives `progress`, `result`, `reset`, `fatal`, `stopped`, `disposed`.
  Output is a delta, including terminal output. Render text as text and retain
  tagged integer strings. Asynchronous observers hold response credit, not runtime
  cleanup; never await `run` or `reset` inside an observer.
- Worker/run generations and ordered response acknowledgements reject stale
  replies. Deferred UI work must recheck generations before rendering. Fatal
  faults retire the instance before notification; the next run uses a fresh one.
- Drives use browser tasks, not a microtask spin. Waiting parks until cancellation;
  there is no external async provider or wake callback. Cumulative run output is
  limited to 256 messages / 64 KiB, with sticky visible truncation. Only one
  unacknowledged response is allowed; saturated progress cannot flood the UI.

Run `node site/scripts/wasm-worker.mjs` for focused Node and real-browser lifecycle
coverage. Waiting, non-yielding compilation and fatal-fault cases use explicitly
labelled test-only adapter wrappers, not production fault hooks. These cases do
not claim panic unwinding or exact browser GC/loan counters; existing SDK tests
own normal root, loan, reentry and pinned-generation cleanup contracts.

## Reproducible products and focused validation

Run from the repository root with installed `wasm32-unknown-unknown`. Use the
default target directory and workspace dev profile. The product runner budgets
each Cargo build/fixture test to 240 seconds, binding generation and graph checks
to 30 seconds, and tool probes to 10 seconds. The complete product stage is bounded
to 16 minutes; explicit CLI installation in CI is separately bounded to 15 minutes.
These budgets are not measured cold-CI durations. Report timeouts instead of
blindly retrying. Nested Unix commands share the supervisor's process group;
Windows uses process-tree termination. Crate and CLI are both
locked to **wasm-bindgen 0.2.129**; discover `wasm-bindgen --version` first. If absent:

```sh
cargo install wasm-bindgen-cli --version 0.2.129 --locked --root target/wasm-tools
```

The following uses that local CLI (use the verified PATH binary instead if already
installed). Generate the source bindings **before** the next build overwrites the
shared Cargo `.wasm` path:

```sh
cargo build -p kagari-web --target wasm32-unknown-unknown --locked
target/wasm-tools/bin/wasm-bindgen target/wasm32-unknown-unknown/debug/kagari_web.wasm --target web --out-dir target/wasm-browser/source --out-name kagari_web
cargo build -p kagari-web --no-default-features --target wasm32-unknown-unknown --locked
target/wasm-tools/bin/wasm-bindgen target/wasm32-unknown-unknown/debug/kagari_web.wasm --target web --out-dir target/wasm-browser/artifact --out-name kagari_web
cargo test -p kagari-web --lib --locked
cargo test -p kagari-web --no-default-features --lib --locked
cargo tree -p kagari-web --no-default-features --target wasm32-unknown-unknown --edges normal
```

Each output directory contains `kagari_web.js`, `kagari_web_bg.wasm` and TypeScript
signatures. The source-free production graph excludes source/syntax/HIR/package,
compiler/MIR/codegen and dynamic loading dependencies.

Generate just the native, current source-free fixture with:

```sh
cargo test -p kagari-web --lib tests::generate_browser_artifact -- --exact
```

It writes **`target/fixtures/browser-basic.kbc`**, after validating and executing
it natively through this adapter. Load its bytes in the artifact-only browser
product, call `start()`, and `drive(1000)` until terminal. Expected output is
`["artifact hello 😀"]`; expected result is
`{"tag":"u64","value":"18446744073709551615"}`. Only the same installed bounded
`web::print` callback is required, with no unsupported external provider.

For bounded product orchestration and freshness checks, run from `site/`:

```sh
node scripts/wasm-products.mjs
node scripts/wasm-products.mjs --verify
node scripts/wasm-browser.mjs
```

The runner discovers an installed Chromium browser or accepts `--executable-path`.
It owns its loopback server/browser, rejects stale products and writes disposable
reports beneath `target/wasm-browser/reports/`. `--mode source` or `--mode artifact`
selects affected cases. No browser or binding tool is silently downloaded.
Native tests and linking alone do not establish browser execution. Selected real
browser acceptance covers both products and the Worker-backed site Playground.
Broader CI/platform and live Pages acceptance remain separately tracked in the
[roadmap](../../docs/implementation-roadmap.md#browser-wasm-execution). The
[site guide](../../site/README.md#playground-ownership-and-acceptance) owns UI,
exact-product publication checks and measured dev-profile browser costs.
