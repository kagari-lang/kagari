# Kagari documentation site

This independent npm project builds a static Astro/Starlight language guide.
The foundation ships a homepage and quickstart (DW02); the active phase and
remaining content/publication work belong to the
[documentation-site plan](../docs/documentation-site-plan.md), not a second ledger
here. Local build acceptance is **not** a claim that Pages is enabled or deployed.
No workflow, remote settings, browser runner or Rust/SDK changes are owned here.

## Build and validate

Use Node **>=22.12.0**. The locally validated toolchain is Node 22.21.1/npm 10.9.4;
Node 24 is the CI reference. From this directory:

```sh
npm ci
npm run build
npm run check
```

`build` runs Astro type/content checking and a clean static build, with each Astro
child limited to 180 seconds. The runner disables telemetry only in its child
process environment (`ASTRO_TELEMETRY_DISABLED=1`); it does not modify any user's
global Astro settings. A failed build cannot reuse a previous successful record.

`check` does **not** rebuild. It requires a successful build record with matching
SHA-256 input and output digests. Inputs include site source/configuration/scripts,
the npm lockfile, the maintained grammar and allowed `.kgr`/`.rs` example sources.
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
npm run check -- --expected-pages 2
```

The optional expected count is a phase acceptance assertion, not a permanent
two-page limit. Later phases can use 20 or 24 after adding their real pages to the
inventory. `check` normally derives its routes from that inventory. `npm test` needs
installed dependencies, but does not require `dist/`.

No browser tests or Cargo tests are run by these commands. In particular, a site
build does not prove a Rust/Kagari example compiles or establish browser search,
theme, keyboard or mobile behavior. Reuse the canonical examples' owning semantic
tests for those contracts; browser acceptance is separate DW05 work. Do not start
an unbounded server as part of validation.

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
Playwright is locked for later bounded browser acceptance; this phase does not
acquire a browser.

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
input or online code runner. Code is highlighted at build time; browsers receive
HTML/CSS and the stock UI/search scripts, not a selector-parsing service.

The advisory explicitly states that ordinary build-time use on trusted sources
is not affected; the attack needs synchronous parsing of attacker-supplied
selectors (for example a flat `.a.a.a...` selector) in a request path. This supports
a narrowly scoped build-time risk assessment, **not** an assertion that the parser
is patched. Untrusted PRs can change build configuration/MDX and already execute
build code, so their builds must remain bounded and unprivileged; do not treat
site sources or plugins as sandboxed input. The local runner bounds each Astro
child to 180 seconds. Any future dynamic CSS service, remote theme/plugin input,
server rendering or browser compiler requires a new review of this boundary.
Parent-plan review owns acceptance of this remaining moderate finding and its
upstream follow-up; local checks do not authorize publication.

Dependencies are website build/development tools only. Installation accesses npm
and runs dependency installation hooks; the generated site has no API keys,
external search service, paid service, analytics, Rust build or Vela dependency.
No Vela code, CSS, prose, images or other assets are copied. `node_modules/`,
`.astro/` and `dist/` are ignored disposable products, not source artifacts.
