# Kagari browser adapter

`kagari-web` builds an actual `cdylib` plus a native-testable `rlib`. Only
`wasm32-unknown-unknown` exports wasm-bindgen bindings. Default `source` forwards
`kagari/source`; `--no-default-features` has no compiler/frontend and **no JS
`compile` method**. Both use `kagari` with defaults disabled and `web` enabled,
validated bytecode, `PreparedProgram`, and SDK owned interpreter execution.

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

## Reproducible products and focused validation

Run from the repository root with installed `wasm32-unknown-unknown`. Use the
default target directory and workspace dev profile. Bound each build/install to
180 seconds; report a timeout instead of blindly retrying. Crate and CLI are both
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
browser acceptance covers both products; Worker/site integration and broader CI
remain separately tracked in the roadmap.
