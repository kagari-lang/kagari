# Kagari documentation site

This independent npm project builds a static Astro/Starlight language guide:
25 substantive English guide/tool pages covering getting started, the language,
standard library, host embedding, references and a local browser Playground. The learning sequence takes inspiration
from the Rust Book while teaching Kagari's shared-value/GC semantics, not Rust's
moves or borrow checker. [Specifications](../docs/README.md#language-and-execution-specifications)
remain the semantic authority. Local acceptance is **not** a claim that Pages is
enabled or deployed. The Playground uses the existing Rust compiler/interpreter
through the [browser adapter](../crates/kagari-web/README.md), not a WASM backend.

## Build and validate

Use Node **>=22.12.0**. The locally validated toolchain is Node 22.21.1/npm 10.9.4;
Node 24 is the CI reference. From this directory:

```sh
npm ci --engine-strict
npm run build:pages
```

Install the Rust `wasm32-unknown-unknown` target and the exact wasm-bindgen CLI
0.2.129 first; see the [product instructions](../crates/kagari-web/README.md#reproducible-products-and-focused-validation).
`build:pages` explicitly builds both WASM modes and a native artifact fixture, then
runs the site build and exact 25-route checks. Products use the workspace dev
profile, default target and parallelism. Every child is bounded; failures/timeouts
are reported, never replaced with cached success. CI acquires tools separately.

With fresh verified products, `npm run build` performs only the site build;
`npm run check -- --expected-pages 25` validates without rebuilding or invoking
Cargo/Rust tools. Astro content rendering never starts Cargo.

`build` runs Astro type/content checking and a clean static build, with each Astro
child limited to 180 seconds. The runner disables telemetry only in its child
process environment (`ASTRO_TELEMETRY_DISABLED=1`); it does not modify any user's
global Astro settings. A failed build cannot reuse a previous successful record.

`check` does **not** rebuild. It requires a successful build record with matching
SHA-256 input and output digests. Inputs include site source/configuration/scripts,
the npm lockfile, the maintained grammar and allowed `.kgr`/`.rs` example sources,
plus the Rust source/manifests, Cargo.lock and target/toolchain configuration. The
record retains verified WASM inputs, tool versions, recipe, source-free dependency
graph and output hashes. Checks also compare both canonical products/native fixture
and the exact deployed source bindings and Worker modules; substitution fails.
Any changes require a new build. Output includes HTML, search chunks and assets;
editing/removing a built file is rejected as well. A check never refreshes the
record to make stale output look valid. Publish only this exact validated `dist/`.

The checker parses HTML using pinned `parse5` (not HTML regexes), resolves local
routes and heading IDs, checks the project base, assets, canonical URLs, titles,
descriptions, source includes, rendered highlighting and search/sitemap products.
It also runs bounded script self-tests for broken links/anchors/assets, lexical
scopes, source traversal/symlinks and fence validation. It verifies repository
source-link targets locally; remote network link checking is not required.

Useful focused commands:

```sh
npm test
npm run check -- --expected-pages 25
```

The optional expected count asserts the current finite manual scope. Update the
inventory and acceptance count together when deliberately adding pages. `check`
normally derives its routes from that inventory. `npm test` needs installed
dependencies, but does not require `dist/`.

`build:pages` runs the focused native artifact-fixture test, not the Cargo test
suite. Ordinary `build`, `check` and `npm test` run no Cargo tests or browser tests.
A static site build does not prove every Rust/Kagari example compiles or establish browser search,
theme, keyboard or mobile behavior. Reuse the canonical examples' owning semantic
tests for those contracts; browser acceptance is separate. Do not start
an unbounded server as part of validation.

## Local browser acceptance and preview

After build and output checks:

```sh
npm run check:browser
# Optional explicit installed browser:
npm run check:browser -- --executable-path "C:/Program Files/Google/Chrome/Application/chrome.exe"
```

The runner tries managed Playwright Chromium, then installed Chrome/Edge/Chromium.
It does not download a browser; install one separately if none is available. It
starts its own loopback-only server on an ephemeral port, serves only a
symlink-free snapshot of `dist` beneath `/kagari/`, blocks external requests and
closes the browser/server in teardown. The matrix has a 150-second deadline and
the process a 180-second last-resort bound. Missing browsers, stale inputs,
changed output and failed assertions return failure, not a skipped acceptance.
Reports and screenshots go beneath ignored `../target/site-browser/`, never dist.

Checks exercise the homepage, a long chapter/table of contents, Rust highlighting
and actual clipboard copy, nested navigation/reload, lazy Pagefind search and
result navigation, persistent dark/light themes, mobile sidebar, keyboard skip
link/search access and custom 404 recovery. HTTP probes cover base redirects,
missing routes, methods and traversal rejection. Screenshots support inspection;
this is not a visual-regression suite, full accessibility audit, cross-browser
matrix or proof of live Pages behavior. Windows Chrome was used for local
acceptance; Linux/macOS discovery and Node 24 CI remain separately unverified.

For interactive human review, run `npm run preview` and open
`http://127.0.0.1:4321/kagari/` (or the port Astro reports). This foreground command
runs until you stop it with Ctrl+C; automated validation uses the bounded runner
instead. Do not expose the preview server as a production service.

## Playground ownership and acceptance

`src/components/Playground.astro` renders the accessible textarea and controls;
`src/playground/ui.js` owns presentation only. The maintained quickstart example
supplies initial source. The editor uses ordinary text, not a second syntax grammar.
Run (or Ctrl/Command+Enter) lazily imports `/kagari/wasm/playground/client.js`;
normal guide visits and merely opening the Playground do not fetch the compiler.
Raw Worker/client/protocol modules are deployed beside `wasm/source/` bindings.
No npm-installed browser runtime or third-party editor is needed.

The [adapter/Worker contract](../crates/kagari-web/README.md#worker-client) owns
scheduling, generations, output credit and cancellation. UI renders text, preserves
integer strings, and selects diagnostic UTF-16 ranges only for the submitted source
snapshot. Textarea newline normalization happens before submission. Editing disables
stale locations. Reset keeps source; leaving the page disposes the Worker. Source
is not uploaded or persisted by the application. Hard stop destroys the instance;
it does not promise destructors, cleanup of external resources or rollback.

After building/checking the exact output, run from this directory:

```sh
node scripts/playground-browser.mjs
node scripts/wasm-worker.mjs
```

The Playground harness owns its bounded server/browser, validates `/kagari/` URLs,
blocks external traffic, and covers lazy downloads, real execution, diagnostic
selection, text-safe output, repeated runs, cancel/reset/hard stop, keyboard/mobile
controls and truncation. It also records separate adapter initialization,
construction, source compilation and execution timings plus raw/gzip/Brotli sizes.
Reports under `../target/site-playground-browser/` are disposable; meaningful
measurements and limitations belong in maintained documentation, not only reports.
Worker tests separately own backpressure, malformed/stale protocol and fatal
recovery; labelled wrappers do not prove Rust panic unwinding. Neither runner is
an exhaustive browser/platform matrix or live Pages acceptance.

### Measured development baseline

On 2026-10-18, `node scripts/playground-browser.mjs --samples 3` measured the
verified workspace **dev-profile** products. This is an untuned first-delivery
baseline, not a performance gate or a claim about optimized release products.
The large on-demand download and several-second source setup remain limitations;
ordinary documentation does not pay these costs.

| Product (JS + WASM) | Raw bytes | gzip bytes | Brotli bytes |
| --- | ---: | ---: | ---: |
| Source-enabled (deployed) | 22,948,638 | 4,137,600 | 2,943,666 |
| Artifact-only (separate) | 12,251,819 | 2,296,430 | 1,655,720 |

Source WASM alone is 22,935,196 bytes; artifact WASM is 12,240,514 bytes.
Each file was compressed separately using Node zlib gzip level 9 / Brotli quality
5, then summed. Declarations and fixtures are excluded. These are compressed-byte
estimates, **not** observed HTTP transfer sizes or a guarantee of Pages encoding.

Workload (result `i32 "499500"`, ten `drive(1000)` slices):

```text
fn main() -> i32 { var sum = 0; for i in 0..1000 { sum += i; } sum }
```

| Sequential sample | Initialization (ms) | Session construction (ms) | Compilation (ms) | Execution (ms) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 76.6 | 1,132.4 | 3,412.9 | 14.6 |
| 2 | 60.8 | 1,115.4 | 3,463.3 | 14.5 |
| 3 | 60.4 | 1,138.4 | 3,431.2 | 14.3 |

Initialization includes import, local JS/WASM fetch and WASM initialization.
Construction measures `new BrowserSession()` separately. Compilation measures
synchronous `compile(source)` including adapter serialization, excluding response
JSON decoding. Execution includes `start`/`drive` and bridge/JSON decoding but no
compilation, UI, Worker messaging or scheduling delays. This is not end-to-end
Run latency, nor evidence of interpreter performance relative to another language.

Environment: Windows x64 10.0.26300; Intel Core i9-12900K / 24 logical CPUs;
Node 22.21.1, zlib 1.3.1-470d3a2; headless Chrome 155.0.8059.82; Rust/Cargo 1.99.0
(rustc b940084d7, LLVM 23.1.1), wasm-bindgen 0.2.129; wasm32-unknown-unknown;
default Cargo parallelism, no recorded build-environment overrides. Source mode
uses default/source features, artifact mode disables defaults. Verified existing
products were reused; original compilation-cache state was not recorded. Samples
used fresh contexts/sessions with HTTP cache disabled and `no-store`, without
warmup discards, in one browser process after UI tests. OS/filesystem and browser
WASM code caches were uncontrolled; these are **not cold-cache measurements**.

Selected Chromium coverage passed; other browsers, cold Linux CI build budgets,
release-profile costs and live deployment remain unverified. Any optimization
follow-up should first profile session construction/source compilation and set a
workload/target, not infer a runtime bottleneck from aggregate initialization.

## Pages workflow and publication

[`.github/workflows/pages.yml`](../.github/workflows/pages.yml) validates every PR,
main push and manual dispatch, without path filters. The build has read-only
repository permissions and no stored checkout credentials. It uses Node 24,
engine-strict locked installation, the visible high-threshold audit, build and
explicit bounded Rust/target/binding-tool acquisition, WASM products and
25-page output checks, then uploads that exact `site/dist/` with no rebuild.
Official actions are pinned by reviewed commit SHA; workflow contract tests parse
YAML rather than matching permission-related substrings.

Deployment is **disabled by default**. The separate deploy job requires a
successful build, `refs/heads/main`, a non-PR event, and the repository Actions
variable `PAGES_DEPLOY_ENABLED` set to `true`. GitHub Actions string comparisons
are case-insensitive, so `True` and `TRUE` also opt in; unset, `false`, `1` and
whitespace-padded values do not. Only that job gets
`pages: write` and `id-token: write`; it executes no checkout or site code.
Main concurrency cancels superseded runs and a read-only head-SHA check rejects
manual reruns of old main revisions. Deployment consumes this run's checked
`github-pages` artifact, not output rebuilt with privileged credentials.

After **separate publication authorization**, a repository administrator must:

1. Confirm the intended project URL is `https://kagari-lang.github.io/kagari/`.
2. Select **GitHub Actions** as the repository Pages source and restrict the
   `github-pages` environment's deployment branches to `main`.
3. Set `PAGES_DEPLOY_ENABLED=true` and run the workflow on the current main SHA.
4. Inspect the actual deployment and repeat nested-route, assets, search and 404
   checks at the public URL. Local output checks do not prove live publication.

No settings changes, push or public activation are performed by local validation.
Leave the variable unset (or set it to `false`) to keep builds non-publishing.
The [roadmap](../docs/implementation-roadmap.md#language-documentation-website)
owns outstanding CI/live acceptance and upstream dependency follow-up.

## Content and links

`src/content/docs/` contains only substantive public guide pages. `docs/spec/`
remains the normative semantic owner. Do not copy the internal documentation tree,
publish placeholders, advertise a reservation package as an installable compiler,
or imply an unreleased feature is available.

`src/navigation.mjs` owns the **shipped** route inventory, group, title and
description. Add each page and its metadata in one change; absent pages must not
appear in navigation. Frontmatter title/description must match the inventory.
Starlight owns layout, search, theme selection, mobile navigation, edit links and
copy controls. Its generated 404 is not a guide-inventory entry. The checker
recognizes Starlight's synthetic `/404/` canonical on `404.html` only; ordinary
links to nonexistent `/404/` routes still fail. Starlight prefixes sidebar config
links with Astro's base, so those config links deliberately omit `/kagari`.
Content authors use `page()` instead.

Use the stable aliases at every MDX nesting depth:

```mdx
import { page, source } from '@site/links.mjs';
import SourceExample from '@components/SourceExample.astro';

<a href={page('start/quickstart#read-the-complete-program')}>Read the example</a>
<a href={source('docs/spec/syntax.md')}>Syntax specification (repository)</a>
```

`page(path)` adds `/kagari/` and the trailing slash before an optional heading
fragment. `source(path)` links to `https://github.com/kagari-lang/kagari/blob/main/`.
Repository links and Starlight edit links are external source URLs, not local Pages
routes. Ordinary Markdown links must also resolve in the generated output under
`https://kagari-lang.github.io/kagari/`; never assume a root deployment.
The origin/base constants are centralized in `src/links.mjs` and used by config
and checks. The configured URL is a target, not evidence of public activation.

## Canonical source examples

Use the build-time component instead of copying executable examples:

```mdx
import SourceExample from '@components/SourceExample.astro';

<SourceExample
  path="examples/syntax/control-flow.kgr"
  command="cargo run -p kagari-cli -- run examples/syntax/control-flow.kgr"
  result="main returns 42; the non-JIT CLI exits successfully without printing that value."
/>
```

`path` is required; `command` and `result` are optional truthful display metadata,
not commands executed by the website. Commands are described relative to the
repository root. Show both for a standalone runnable program; label excerpts,
fragments and invalid examples explicitly. `result` can describe a value or
observable effect and must not pretend a value is printed when it is not.

The first complete host program is `crates/kagari/examples/typed_native.rs`.
`scoped_host.rs` and the reload tests own the more advanced boundary examples;
benchmark examples are not beginner embedding programs.

Only regular `.kgr` and `.rs` files beneath `examples/` or
`crates/kagari/examples/` are readable. Paths are repository-relative with forward
slashes. Traversal, absolute paths, encoded paths and symlink/junction components
are rejected, even if a symlink points back into an allowed root. Missing files
fail the build. The component emits a canonical source link and the checker
compares rendered code to the original file. Include only canonical tracked
examples, not disposable developer files. Vite injects the config-derived checkout
root for prerendering; no developer-specific path is committed or needed at runtime.

## Highlighting and dependencies

`src/grammar.mjs` reads
[`editors/vscode/syntaxes/kagari.tmLanguage.json`](../editors/vscode/syntaxes/kagari.tmLanguage.json)
directly and adapts **only** the language name (`kagari`) and alias (`kgr`). There
is no copied grammar or second lexical implementation. Shiki scope tests include
comments, strings, interpolation holes, pattern rest and struct update. They do
not infer a type from capitalization or claim semantic IDE analysis.

Every fence needs a known language: normally `kagari`/`kgr`, `rust`, `sh` or `text`.
Astro's parsed Markdown/MDX fence hook rejects unknown or omitted labels; checks
also inspect rendered code blocks. SourceExample selects Kagari or Rust by file
extension. The homepage's Rust import is explicitly an excerpt, not a complete host.

Direct development dependencies are exact-pinned and transitive dependencies are
locked in `package-lock.json`:

| Dependency | Version |
| --- | --- |
| `astro` | 7.3.8 |
| `@astrojs/starlight` | 0.42.6 |
| `@astrojs/mdx` | 8.0.3 |
| `@astrojs/markdown-remark` | 7.3.2 |
| `@astrojs/check` | 0.9.10 |
| `typescript` | 6.0.3 |
| `shiki` | 4.5.0 |
| `parse5` | 8.0.1 |
| `playwright` | 1.64.1 |
| `yaml` | 2.9.1 |

The Astro 7/Starlight 0.42 upgrade replaces the vulnerable original selection;
it is not a forced peer resolution or an override. Astro 7.3.8 and MDX 8.0.3
support Node >=22.12.0 and share Starlight's Astro ^7.2.10 / Markdown Remark
^7.3.0 peer ranges. Newer Astro 7.4.0 and MDX 8.1.0 require Node >=22.22.0.
Markdown Remark 7.3.2 is the latest patch compatible with local Node 22.21.1:
7.3.3 pins Prism 4.1.0, which also requires Node >=22.22.0, whereas 7.3.2 pins
compatible Prism 4.0.2. TypeScript 6.0.3 stays within Astro check's supported
^5.0.0 / ^6.0.0 peer range rather than selecting unsupported TypeScript 7.

`parse5` is direct so checks use a maintained HTML parser. Markdown Remark is
direct for the supported `unified({ remarkPlugins })` processor configuration
and parsed fence hook, not a hoisted transitive import or deprecated option.
The processor, Starlight Code component, docs loader/schema and shared Shiki
registration retain their existing site-facing APIs with this selection.
Playwright is locked for bounded browser acceptance. `yaml` parses the Pages
workflow for executable permission, trigger and exact-artifact contract tests.

### Dependency security boundary

The selected tree has no reported critical, high or low advisories. It is **not
fully audit-clean**: `npm audit` reports 10 moderate package entries, all tracing
to the single underlying
[GHSA-rj75-hqrm-r3gf / CVE-2026-104844](https://github.com/advisories/GHSA-rj75-hqrm-r3gf)
selector-parser CPU-exhaustion advisory. `npm audit --audit-level=moderate` fails;
`npm audit --audit-level=high` passes without suppressing the moderate findings.
These are distinct results, not a zero-vulnerability claim.

The published dependency path is Starlight 0.42.6 → astro-expressive-code 0.44.2
→ rehype-expressive-code / expressive-code 0.44.2 → @expressive-code/core 0.44.2
→ postcss-nested 6.2.0 → postcss-selector-parser 6.1.4. The latest stable
Expressive Code core still requires postcss-nested ^6.0.1, which requires
selector-parser ^6.1.1. Registry inspection found no patched parser 6.x or nested
6.x release; the advisory's patched parser is 7.1.6. npm's proposed Starlight
0.21.5 downgrade is not compatible with this patched Astro 7 stack. Do not force
a cross-major parser override or downgrade the framework to hide the report.

The applicable boundary was checked against the published
[Expressive Code core 0.44.2](https://www.npmjs.com/package/@expressive-code/core/v/0.44.2)
and [postcss-nested 6.2.0](https://www.npmjs.com/package/postcss-nested/v/6.2.0)
implementations: core's `scopeAndMinifyNestedCss` runs its nested-CSS PostCSS
preprocessor on plugin/theme styles; nested's `parse` synchronously calls
`parser(...).processSync(rawSelector)`. This is a build-time CSS preparation path,
not a public request handler. The site supplies repository-controlled configuration
and styles, uses static output with no server adapter, and exposes no remote CSS
input. Guide code is highlighted at build time. The browser Playground accepts
Kagari source in a textarea and executes it in a local Worker/WASM instance; it
never passes user text to MDX, Shiki, CSS parsing or a server. All output and
diagnostics use DOM text APIs, not HTML. No remote theme/plugin input, network
provider, arbitrary JS callback, source upload or secret is added. Input/output
bounds and hard stop do not constitute hard compiler/heap quotas or protection
against every browser resource-exhaustion scenario.

The advisory explicitly states that ordinary build-time use on trusted sources
is not affected; the attack needs synchronous parsing of attacker-supplied
selectors (for example a flat `.a.a.a...` selector) in a request path. This supports
a narrowly scoped build-time risk assessment, **not** an assertion that the parser
is patched. Untrusted PRs can change build configuration/MDX and already execute
build code, so their builds must remain bounded and unprivileged; do not treat
site sources or plugins as sandboxed input. The local runner bounds each Astro
child to 180 seconds. Any future dynamic CSS service, remote theme/plugin input,
server rendering or new browser host capabilities require a new review of this
boundary. The current local-only compiler boundary was reviewed as described above.
The [roadmap](../docs/implementation-roadmap.md#language-documentation-website)
tracks the remaining upstream-patch follow-up. This is an explicitly accepted
build-time limitation, not authorization for publication.

Dependencies are website build/development tools only. Installation accesses npm
and runs dependency installation hooks; the generated site has no API keys,
external search service, paid service, analytics or Vela dependency. Rust builds
happen only in explicit tooling/CI, never in the deployed static host.
No Vela code, CSS, prose, images or other assets are copied. `node_modules/`,
`.astro/` and `dist/` are ignored disposable products, not source artifacts.
