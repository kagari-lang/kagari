# Language documentation website execution plan

## Status and authority

Status: active; implementation and one commit per phase authorized on 2026-10-18.
DW01-DW03 are complete; DW04 is next. Pushes, GitHub settings and public activation are
not authorized.
The [roadmap](implementation-roadmap.md#language-documentation-website) owns activation
and queue placement. This plan owns DW01-DW05, content scope, remaining decisions,
acceptance and the sole progress ledger. The implementation authorization does not
permit repository settings, pushes or public deployment.

Build a Kagari language documentation website similar in structure to Vela's
Astro/Starlight site. Reuse mature presentation and deployment mechanisms, not
Vela's language semantics, Rust APIs or execution model. Use Rust's documentation
as the reference for learning sequence, precise explanations and separation of
manual/reference/library responsibilities, not as a feature checklist. Examples and
navigation below describe planned products, not files or public endpoints already
delivered.

## Outcome and finite scope

Deliver a static, searchable user-facing site with:

- A useful homepage, learning path, sidebar, section outlines, light/dark themes,
  mobile navigation, copyable code blocks and source/edit links.
- A bounded first manual for script authors and Rust embedders, explaining only
  current supported behavior and explicitly identifying limitations.
- Kagari code highlighting from the existing editor grammar, with no second
  independently maintained grammar or semantic classification implementation.
- A locked, independently buildable website and an unprivileged PR validation path.
- GitHub Pages deployment configuration for the default project URL, with actual
  public activation separated from local/build acceptance and authorization.

Initial delivery is English, consistent with repository documentation rules.
Keep routing/content organization compatible with Starlight localization, but do
not publish empty translated pages or imply Chinese coverage. Chinese translation
is a separately selected scope, not a reason to duplicate the whole manual now.

Excluded: browser Playground/WASM compilation, runtime ports, server-side execution,
new language/SDK features, remote example execution, a custom documentation engine,
full generated standard-library API/rustdoc mirroring, versioned release sites,
translations, custom domains/DNS, analytics, external search services, marketplace
publication and wholesale migration of internal engineering documents. Do not couple
this site to Vela's Rust/WASM dependency chain or change Vela's checkout.

## Baseline and inspected evidence

Planning baseline: clean `1d489aa3`, inspected on 2026-10-18. Kagari has `docs/`,
`examples/`, editor adapters and `.github/workflows/ci.yml`, but no `site/` or Pages
workflow. Remote origin is `https://github.com/kagari-lang/kagari.git`. GitHub Pages
settings, deployment permissions, domain configuration and live site status were
not queried; none is assumed enabled. No site dependencies/builds were run here.

Vela's local site was inspected read-only. Its configuration uses Astro 6/Starlight
0.40, Markdown/MDX content collections, English and Chinese routes, Shiki/TextMate
highlighting and minimal theme CSS. It has 144 Markdown/MDX pages. Its Pages workflow
waits for successful main/master push CI, checks out that exact revision, builds a
Rust Playground/WASM package and uploads `site/dist`. This is useful structural
evidence, not a requirement to copy its page count, versions, dependency graph,
workflow trigger or unverified language claims. No visual browser review or Vela
build was performed.

| Existing Kagari owner | Website use / boundary |
| --- | --- |
| [Goals](project_goal.md), [syntax](spec/syntax.md), [traits](spec/traits.md), [values](spec/value-semantics.md) | Define the language being taught. Website prose must not silently change or add semantics. |
| [Builtins](spec/builtins.md), [collections](spec/collection-access.md), [native declarations](spec/standard-declarations.md) | Authoritative library/registration contracts. A guide is not a second manually curated complete API inventory. |
| [Embedding](spec/embedding-api.md), [host interop](spec/host-interop.md), [activation](spec/module-activation.md) | Define host/heap, root/borrow and reload boundaries. Verify concrete Rust snippets against runnable current APIs; the embedding specification alone is not an exact API-name guarantee. |
| [Examples](../examples/README.md), [SDK examples](../crates/kagari/examples) | Reuse executable sources and existing test owners rather than maintaining website-only copies. Source paths and runnable commands must be checked against actual targets before publication. |
| [TextMate grammar](../editors/vscode/syntaxes/kagari.tmLanguage.json) | Single maintained lexical grammar used by Shiki; register stable `kagari` and `kgr` aliases. Do not copy a divergent grammar into site/src. |
| [Editor guide](../editors/README.md), [tool contracts](spec/toolchain-tools.md) | Explain actual installation and CLI capabilities; distinguish syntax adapters from semantic IDE or debugger features. |
| [Roadmap](implementation-roadmap.md), [review](review.md), active plans | Development information, not user-manual chapters or implemented-feature promises. Link selectively to repository sources; do not automatically publish or index the whole directory. |
| [CI](../.github/workflows/ci.yml) | Keep existing Rust/editor checks intact. Add finite website validation/deployment without reproducing the workspace test matrix. |

## Architecture and content ownership

### Static presentation, separate development dependencies

Use Astro with the maintained Starlight integration. Prefer stock accessible layout,
search, sidebar and code-block components; add only small Kagari branding/layout
adjustments. No custom component framework, client-side router or runtime service
is required. Use MDX only for pages needing supported components or source includes.

Planned owner layout:

```text
site/
  package.json
  package-lock.json
  astro.config.mjs
  README.md
  src/content.config.ts
  src/content/docs/       # user-oriented guides, not copied specifications
  src/styles/            # bounded branding/theme adjustments
  scripts/               # focused links, grammar and content checks
.github/workflows/pages.yml
```

This is an independent npm project, not a new Cargo crate and not a merger with
`editors/`' existing workspace/lockfile. Website dependencies are development/build
tools only. At DW01 select a mutually compatible supported Astro/Starlight/Shiki and
Node toolchain, record exact versions and commit one site lockfile. Node 24 is the
reference workflow candidate, subject to package engine verification. Do not copy
Vela's caret ranges as reproducibility evidence or upgrade editor dependencies as
part of site setup. `npm ci` must work without Vela files or installed global tools.

Reuse the tracked grammar directly from its actual editor owner. If Shiki requires
normalized IDs/aliases, adapt registration metadata in one place, not the token
rules. Literal/comment/string/interpolation and new destructuring/update syntax
must stay lexical; a name's capitalization does not prove a type or constructor.
Code fences use `kagari`/`kgr`, Rust, shell or text appropriately.

### One semantic authority, distinct teaching material

`docs/spec/` continues to own normative behavior. `site/src/content/docs/` owns
explanation order, small scenarios, tutorials, caveats and cross-links. Both can
explain the same feature, but do not hand-copy complete specifications or maintain
a second conflicting signature/bound/availability table. A language change updates
its spec, relevant example and affected guide together.

For first delivery, the reference landing page links to tracked GitHub specifications
and source APIs, with a visible distinction between external repository reference
and website tutorial. Do not build a generalized importer that rewrites all internal
Markdown links. If later publishing full specs as site pages is selected, use a
single-source build projection with explicit route/metadata ownership, not manual
copies; that migration is outside this delivery.

Prefer a small source-include component using ordinary build-time imports when an
existing runnable `.kgr` or Rust example is shown. Its title/source link must identify
the actual owner. Standalone programs need documented invocation and expected result;
partial fragments and deliberately invalid examples must be clearly labeled, not
passed off as executable full programs. Do not introduce a new snippet language or
duplicate the compiler to validate prose. Reuse existing owner tests, adding a small
example-selection check only where the site introduces genuinely new contracts.

Current documentation drift must not be propagated. Verify commands against current
Cargo targets and CLI behavior, not older example-index text. Kagari has no functional
published toolchain release: do not advertise `cargo install kagari-cli` or the
reservation package as a working installation path. Explain source builds until an
actual functional distribution exists. No benchmark superiority or production-ready
claim without current supporting evidence.

### Rust-inspired information architecture and writing style

Follow the distinction made by [The Rust Programming Language](https://doc.rust-lang.org/book/)
and [The Rust Reference](https://doc.rust-lang.org/reference/): a sequential learning
manual teaches readers to use concepts; a reference precisely answers a specific
semantic question. Neither should become a source-code walkthrough or replace
library API documentation. This changes organization/style, not the chosen
Astro/Starlight stack or the 24-page budget; separate books/sites are not required.

| Reader need | Rust reference model | Kagari application |
| --- | --- | --- |
| Learn the language progressively | The Book | Start + Language form the guide, from a running program through values, functions, data models, error propagation and abstractions. |
| Resolve an exact behavior question | The Reference | Link to the single authoritative docs/spec owners, with grammar/rule/limit references rather than a copied tutorial-shaped specification. |
| Discover library behavior and APIs | Standard-library documentation | Keep the library/prelude guide distinct from syntax; link declaration/contract owners and existing examples. Full API generation remains deferred. |
| Embed, install and operate tools | Task-specific tooling guides | Embedding and CLI/editor pages follow the actual Kagari host/tool workflow, not Rust's compiler or Cargo implementation. |

Teach shared-value semantics early, in the types/values chapter before relying on
aliasing in structs, collections or captures. Use the place occupied by ownership
in Rust's learning path to explain Kagari's actual model: val versus var, scalar
copies versus shared objects, GC-managed script values, and explicit host access.
Do not add borrow-checker/lifetime/move chapters or imply that GC eliminates host
borrow/root rules. Return to those concrete boundaries in the embedding guide.

A normal guide chapter follows this proportional pattern:

1. State the reader's task and any prerequisite concepts.
2. Show one small, complete example where practical, with its invocation and result.
3. Explain why the result occurs, using the values/control flow the reader just saw.
4. Introduce a related variation or a common invalid use with a verified diagnostic
   when it teaches a distinct rule, not a catalog of every error spelling.
5. State relevant limitations and link the precise specification and next chapter.

Build on a few small recurring scenarios rather than switch to unrelated code
for every paragraph. Use descriptive headings, explicit prerequisites, short
paragraphs and cross-links. Define terms at their first use and keep slot/binding,
object/identity, interface and host-view vocabulary consistent. Sidebar order
follows conceptual dependencies, not crate names, EBNF production order or the
historical implementation sequence. Functions precede generic abstractions;
concrete data/patterns and Option/Result establish examples before advanced traits.

Reference-oriented material instead states admissible forms, typing/access rules,
evaluation order, observable errors and supported limits precisely. Library pages
explain public behavior/bounds and examples, not hidden compiler plumbing. Do not
mix planned APIs into current examples. Use a short "Compared with Rust" note only
where it prevents a likely misconception, especially shared mutation, Default,
struct update and host borrowing; do not make every chapter a Rust comparison.

Adopt the teaching mechanism, not Rust prose, diagrams or examples verbatim. Any
nontrivial reused asset needs its own license/attribution review. Rust's current
edition/release model, ownership guarantees and supported feature breadth are not
Kagari promises. Every example and diagnostic still comes from Kagari's current
contracts and executable evidence.

### Bounded first manual

Freeze this 24-page first-delivery budget at DW01; combine closely related topics
rather than add speculative empty chapters. Every shipped route has substantive
content, a relevant example or concrete operation, and useful next/reference links.
Navigation labels/slugs may be refined before DW02 without changing subject scope.

| Group | Planned pages | Primary current owners |
| --- | --- | --- |
| Start (4) | Homepage; quickstart; source installation and CLI; editor setup | Root README, current CLI, editor guide, runnable examples |
| Language (10) | Variables/constants; types, values and shared identity; operators/control flow; functions/closures; structs/enums; patterns/destructuring; Option/Result/propagation; generics/traits/interfaces; modules/packages; Default/struct update | Syntax, traits, values, package and builtin specs; corresponding executable examples |
| Standard library (3) | Library/prelude overview; collections and access; strings and iteration | Builtins, collection access and installed declarations |
| Embedding (4) | First Rust host; native functions/types; ownership/GC/scoped access; hot reload | Current SDK examples, embedding, host interop and activation specs |
| Reference (3) | CLI/diagnostic usage; status and limits; specification/source index | Tool/error contracts, goals, roadmap and tracked specifications |

In particular, explain static typing versus interface erasure, val rebinding versus
shared-object mutation, Array versus Vec versus read-only views, pattern rest versus
update base, ordinary core Default versus trait default methods, and old-generation
calls versus new publication. Do not import Vela's dynamic type hints, struct-field
declaration defaults, HostAccess API or execution budgets into Kagari documentation.

The status page summarizes user-relevant implemented/unsupported boundaries and
points to the roadmap. It is not a second phase ledger. Do not include development
review findings, agent instructions, entire architecture reports or performance
probe transcripts in the main navigation/search corpus.

## URLs, deployment and trust boundary

Default target, assuming the public repository keeps its current identity:

```text
site: https://kagari-lang.github.io
base: /kagari
public entry: https://kagari-lang.github.io/kagari/
```

This is a proposed deployment target, not a verified live URL. Keep base handling
centralized using Astro/Starlight mechanisms. Test nested links, assets, search
index/chunks, sitemap/canonical URLs and 404 behavior under `/kagari/`, not just at
a local root. Never use local `F:` paths or depend on a developer's checkout layout.
GitHub source/edit links are external repository URLs, not Pages-relative paths.

Use a Pages build/deploy workflow with separate job permissions:

- PRs run locked install, checks and static build without write/OIDC permissions;
  fork PR content is never executed in a privileged deploy context.
- Main-branch pushes and explicit workflow dispatch can deploy a successfully
  validated main revision once activation is authorized. Manual dispatch from
  another branch must not silently publish it.
- Build uses contents:read; deployment alone receives pages:write/id-token:write,
  the github-pages environment and a deployment concurrency group. Use supported
  pinned/reviewed actions consistent with repository policy.
- Upload the exact validated static output and deploy that artifact without a
  separate checkout/rebuild. Failure must not replace the last good site.
- Do not introduce pull_request_target or privileged workflow_run processing of
  untrusted artifacts. Existing Rust/editor CI remains independently mandatory;
  site-only validation does not run full Cargo suites or the editor/native matrix.
- Trigger/path selection includes site sources/lock/scripts, relevant docs/examples,
  the shared grammar and Pages workflow. Avoid a permanently pending required check
  caused by path-filtering a workflow that branch protection expects on every PR.

No credentials belong in source or frontend bundles. Static deployment requires no
runtime API key or paid service. Enabling Pages with GitHub Actions, environment
approval/protection changes, branch protection, DNS/custom domains and actual pushes
are remote administrative actions requiring separate authorization. If deployment
is not authorized, finish local/CI-ready delivery and explicitly leave live-publication
acceptance pending; do not claim a live site or authorize a push through this plan.

## Mature references and adaptation

Primary documentation consulted on 2026-10-18:

- [Starlight getting started](https://starlight.astro.build/getting-started/):
  Markdown/MDX content, navigation and mature documentation components.
- [Astro GitHub Pages deployment](https://docs.astro.build/en/guides/deploy/github/):
  static output, site/base configuration, committed package lock and Actions artifact
  deployment.
- [The Rust Programming Language](https://doc.rust-lang.org/book/) and
  [The Rust Reference introduction](https://doc.rust-lang.org/reference/): learning
  manual versus precise language reference, with separate library/tooling owners.
  Reuse the explanation structure without adopting Rust ownership or copying prose.
- [Vela](https://github.com/realm-labs/vela): the inspected local site supplies the
  reference organization and minimal styling. Record its exact source revision and
  inspect license/attribution before reusing nontrivial implementation assets at DW01.

Use these established mechanisms rather than a custom generator. Vela's 144-page
bilingual manual and Rust/WASM Playground have different scope and semantics;
Kagari's initial site needs neither. Package/action versions are frozen against
actual compatibility at DW01, not inferred from mutable documentation snippets.
No local file path or generated Vela output becomes a documentation dependency.

## Phase order and acceptance

### DW01 — Freeze site, content and publication contracts

Reconfirm the baseline and current examples/specs; inspect Vela's reusable assets,
revision and license. Select compatible pinned tools, locale decision, 24-page
route list, Rust-inspired guide/reference/library separation, shared-value teaching
sequence, source-include policy, grammar adapter and workflow permissions/triggers.
Record the finite content-to-spec/example mapping and focused checks here, not in a
second progress queue. Acceptance: ownership, build boundaries, remote authorization
and unsupported claims are explicit before implementation. No empty public pages.

### DW02 — Build the static site foundation and shared highlighting

Create the independent site project/lockfile, content schema, real homepage and
quickstart, normal navigation/search/theme, shared grammar registration and minimal
branding. Keep not-yet-written routes out of the shipped sidebar rather than add
placeholders. Include repository/source/edit links and local build instructions.
Acceptance: locked clean build, valid nested `/kagari/` assets, rendered Kagari/Rust
blocks, focused highlighting tests and no dependency on runtime, analyzer or Vela.

### DW03 — Deliver the bounded language and library manual

Complete the language/standard-library groups plus installation/editor and
CLI/status/reference pages. Apply the Rust-inspired learning order and chapter
style above without importing Rust-only semantics. Reuse current examples, including binding patterns and
Default/update. Check code explanations, type/error boundaries, links, navigation,
search labels and contents. Acceptance: every shipped route is useful and consistent
with its authoritative owner; no placeholder chapter, false installation path,
unsupported library inventory or duplicate maintained specification.

### DW04 — Complete host/reload guidance and Pages automation

Complete the four embedding pages against actual SDK examples, with precise host
ownership, borrow/root cleanup and generation-pin behavior. Add PR build checks and
main-only deploy automation with least privilege. Preserve existing CI. Document
repository Pages setup separately from authorized execution. Acceptance: site build
and selected host/example checks pass; workflow/permission review confirms no PR
publication path and deployment consumes the checked artifact. No remote setting,
push or public activation without authorization.

### DW05 — Validate, hand off and retire the plan

Complete local static links/anchors/assets, source includes, syntax captures and
selected semantic example validation. Inspect a finite browser matrix: homepage,
long language page, code-heavy host page, nested route, search, light/dark, mobile
navigation and keyboard access. Where authorized and available, deploy and check
those boundaries at the public project base; otherwise record live acceptance as
pending in the roadmap instead of claiming it passed.

Move lasting site build/content/maintenance contracts into site/README and the
appropriate documentation owner. Remove the completed plan and update indexes;
Git history retains phase decisions and command results. Keep untranslated locales,
Playground, broader API generation and publication limits in the existing roadmap,
not an expanding delivery checklist. Completion does not imply CI/browser/platform
checks that were not actually run.

## Verification and tooling

Planning/documentation-only checkpoints need local links, content and diff checks,
not Cargo or npm installation. Implementation uses locked local Node/npm tools,
bounded commands and ignored `target/` for temporary logs/screenshots. Site output,
node_modules and framework caches are disposable/ignored, not tracked products.

At DW01 freeze a small maintained script interface for implementation. The build
must perform Astro type/content checks before producing fresh static output;
`npm run check` then validates that output plus local source/content/grammar rules
without rebuilding it. CI uploads this same checked output. Never validate a stale
or nonexistent dist directory, or rebuild after output validation. Exact script
names below are planned acceptance commands, not commands available in the current
repository:

```text
# From site/, after it exists:
npm ci
npm run build
npm run check
```

Checks must inspect built HTML and assets, not just Markdown regexes. Detect absent
internal routes/anchors, wrong base-prefixed URLs and missing source includes;
separate genuine external references from local Pages links. Keep external network
link checking optional/bounded so a remote outage does not masquerade as a broken
local site. Use a few Shiki assertions for strings/comments/interpolation and
patterns/update syntax; do not duplicate the editor's complete grammar matrix.
Check unknown fence languages and actual highlighted output. Placeholder detection
is a guard, not proof that prose is correct.

Reuse affected existing example/test owners and type-check/run only the small
selection used by new pages. Do not trust old test target spellings in indexes
without discovery. A website build does not establish that Rust/Kagari snippets
compile; an example compilation does not establish mobile/search/accessibility.
For unchanged canonical example files, retain prior evidence rather than rerunning
all language tests. Do not run a full-workspace suite or complete feature/backend/
editor matrix locally. Broader checks belong to CI.

For Rust implementation changes, if any become genuinely necessary, apply the
repository structural review and required workspace Clippy/checks; the site itself
must not require Rust production changes. Otherwise run affected website checks,
local documentation links and `git diff --check`; no gratuitous Rust rebuilds.
After failures repair related owners and rerun affected cases only. Record actual
failing commands, cause and repair here; do not disable assertions or hide broken
links to make a public build appear green. Later phase commits are coherent and
Conventional, with `Plan-Phase: DW0N` trailers when commits are authorized.

## Progress ledger

- [x] DW01: freeze tools, content routes, ownership and publication boundary.
- [x] DW02: static site foundation, shared syntax highlighting and real entry pages.
- [x] DW03: bounded language/library manual and reference navigation.
- [ ] DW04: embedding/reload guide and least-privilege Pages workflow.
- [ ] DW05: selected local/browser acceptance and documentation handoff.

### DW01 — activation and frozen delivery

Activated from `1d489aa3` with the existing three planning files; those edits belong
to this task. Vela reference revision is `5c511b8112b7aebb515d020b6b27fd722485dbd4`.
Its root license is MIT, copyright 2026 realm-labs. Implement fresh configuration
using official Astro/Starlight mechanisms and the Kagari-owned grammar; no Vela
prose, CSS, components or other substantial assets will be copied, so no Vela
runtime/local-checkout dependency or copied-asset attribution is introduced.

Frozen tool selection: Astro 6.4.6, Starlight 0.40.0, MDX 6.0.2, Astro check 0.9.9,
TypeScript 6.0.3 and Shiki 4.2.0. Registry metadata confirms Astro accepts Node
>=22.12.0; local Node 22.21.1/npm 10.9.4 meet that requirement, and CI uses Node 24.
MDX 6.0.2 aligns the markdown peer dependency with Starlight; do not select 6.0.3
without rechecking its changed peer. The lockfile freezes transitive resolution at
DW02. Playwright 1.64.1 is selected for the concrete bounded DW05 browser checks,
using a locally available Chromium/Edge or explicitly acquired test browser. No
browser server/process may be left running. No dependency install has run at DW01.

Frozen routes and current source mappings:

| Route | Semantic/example owner |
| --- | --- |
| `/` | project_goal, current source-build status |
| `start/quickstart` | CLI; examples/syntax/control-flow.kgr |
| `start/installation` | Cargo targets, CLI help, reservation limitation |
| `start/editors` | editors/README and adapter installation owners |
| `language/variables` | syntax statements; binding-patterns example |
| `language/values` | value-semantics identity; data-model example |
| `language/control-flow` | syntax expressions/control flow; control-flow example |
| `language/functions` | syntax functions/captures; closures example |
| `language/data-model` | structs/enums/tuples; data-model example |
| `language/patterns` | checked patterns; binding-patterns/match examples |
| `language/error-handling` | Option/Result/propagation; result-option/try examples |
| `language/traits` | traits spec; where-bounds/associated-types examples |
| `language/modules` | packages/modules specs; inline-modules and package-workspace |
| `language/default-updates` | Default/update specs; struct-update example |
| `library/overview` | installed builtin/prelude contract |
| `library/collections` | collection-access/value semantics; collection-access example |
| `library/strings-iteration` | builtins/Iterable; string-interpolation/iterators examples |
| `host/overview` | current runtime_initialization Rust example |
| `host/native` | native declarations; typed_native/scoped_host examples |
| `host/ownership` | host-interop/root contracts; scoped_host example |
| `host/reload` | module-activation; current SDK reload test/API owner |
| `reference/cli` | actual CLI and toolchain-tools/error-reporting |
| `reference/status` | goals/roadmap supported boundaries, no duplicated ledger |
| `reference/specifications` | explicit external links to tracked spec/source owners |

Only complete routes enter navigation; DW02 ships index/quickstart only. Include
canonical sources through a small build-time SourceExample component with a
repository-relative path and optional truthful command/result metadata; resolve
only allowed tracked example roots, never arbitrary local files. Do not copy files
or add a snippet DSL. Link helpers centralize `/kagari/`; Markdown links must still
resolve in generated output, not depend on an assumed root deployment.

Frozen checks: build performs Astro type/content checks then generates output;
check consumes that output for HTML links/anchors/assets/base, source includes,
page/title/fence validity, focused shared-grammar assertions and content guards.
DW03 requires 20 non-host pages; DW04 completes exactly 24. Reuse selected existing
CLI/script checks for control flow, destructuring and Default/update plus current
runtime_initialization/typed_native/scoped_host examples, not a workspace sweep.
DW05 uses a bounded browser runner for desktop/mobile, search, themes, keyboard,
code/copy and 404/nested routing. Actual live deployment remains pending unless
separately authorized. Pages deployment additionally requires an explicit repository
opt-in variable `PAGES_DEPLOY_ENABLED=true`, main ref and a non-PR event; default
validation cannot publish even if somebody later pushes the branch.

DW01 changes documentation only. Registry engine/peer inspection is compatibility
evidence, not installation/build acceptance. Local document links and diff checks
precede this phase commit. No carried implementation failure or performance claim;
full workspace/feature/backend/editor matrices are not part of local site validation.

### DW02 — static foundation and shared grammar

Implemented a standalone locked Astro/Starlight project, homepage/quickstart only,
shared grammar aliases, centralized base/source links, restricted canonical source
includes, stock search/theme/navigation and DOM-based generated-output validation.
Checks reject stale source/output records, broken routes/anchors/assets, unknown
fences and mismatched included source; tests also cover traversal/symlink rejection.
No copied grammar, Vela asset or runtime/compiler change.

Material tool-selection correction: initial DW01 reference versions installed and
built, but audit reported critical/high framework/image dependencies. Replaced them
before committing with Astro 7.3.8, Starlight 0.42.6, MDX 8.0.3, Markdown Remark
7.3.2, Astro check 0.9.10, Shiki 4.5.0 and parse5 8.0.1; TypeScript 6.0.3 and
Playwright 1.64.1 unchanged. Registry peers/engines and engine-strict npm ci confirm
local Node 22.21.1 compatibility; no overrides or peer bypass. Newer patches require
Node 22.22+, so do not apply them under the older local runtime without revisiting
the toolchain. Lockfile owns complete resolution.

Audit at high threshold passes, not fully clean: 10 moderate package entries share
GHSA-rj75-hqrm-r3gf in the upstream Expressive Code/nested CSS/parser 6.x chain.
The primary advisory excludes ordinary trusted build-time use; this site has no
public/request-time CSS parsing or user-uploaded theme service. Accept this narrow
build-time risk with bounded, unprivileged PR builds, no silent cross-major override,
and an upstream-patch follow-up in site/README. Moderate-threshold audit still fails;
this must remain visible at handoff and requires renewed review for dynamic inputs.

Passed locked engine-strict installation, Astro type/content/static build, generated
2-page/404 links/assets/base/search/sitemap/source/highlight checks and 15 script
tests. Initial dependency download timed out and succeeded on bounded retry;
nonfatal Windows cleanup and optional Starlight collection warnings do not establish
missing rendered pages. No carried build/test error. Browser/live Pages and CI Node
24 remain unrun. Source-file whitespace/diff review precedes the phase commit.

### DW03 — bounded language/library manual

Completed all 20 non-host routes: ten language chapters, three library guides,
installation/editor setup and three references, plus the existing two entry pages.
Integrated route metadata and progression links. Shared-value/GC semantics precede
abstraction; examples explain values/effects instead of internal crate mechanics.
No copied spec, fabricated diagnostic transcript or unshipped-host route. Independent
source/spec review found no concrete semantic or API/output error.

Passed Astro type/content/build and generated checks for 20 pages/404, 19 canonical
source includes, metadata/base/anchors/assets/search/sitemap, and all 15 script
tests. Navigation count failed temporarily before metadata integration and passed
afterward. CLI metadata/build/help verified current targets. Selected checks/runs
cover control flow, binding patterns, Default/update, package-workspace, strings,
collections, iteration and the source/artifact command round trip. Help currently
returns usage status 2; fmt --check on the canonical control-flow file reports
existing KG_FORMAT_DIFFERENCE/status 1, correctly documented without modifying the
example. No full Rust or editor suite, dependency change or browser claim.

No carried website build/test failures. DW04 owns four host pages and workflow;
DW05 still owns actual browser/live-publication limits and final handoff.
