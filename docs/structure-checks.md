# Rust Structure Checks

The repository uses a strict, syntax-aware check for the mechanical rules in
[AGENTS.md](../AGENTS.md). It runs independently of Cargo, including while crate
wiring is incomplete. Existing violations fail exactly like new violations;
there is no grandfathering baseline, changed-files-only mode or warning-only mode.
Effective LOC has a bounded exception policy. Re-exports are forbidden by
default and require an exact, reviewed whitelist entry; the initial whitelist
is empty.

## Commands

```text
uv run --locked scripts/check_structure.py
uv run --locked scripts/check_structure.py --json
uv run --locked scripts/check_structure.py --self-test
```

The script uses Python 3.11 or newer. `uv` installs the pinned Tree-sitter packages
from the adjacent script lockfile; no workspace build or global Python package
installation is required. CI runs the regression suite followed by the full audit
as a separate required-to-succeed job, alongside the existing Rust job. Branch
protection settings are managed separately on the repository host.

The default root is the repository containing the script. `--root PATH` selects
another Git working tree. The check reads tracked and non-ignored untracked `.rs`
files, including working-tree edits, and skips deleted files. Ignored Cargo output
is not scanned. Tracked generated sources are checked; a generated-file comment
does not disable rules. Cargo manifests supply target and dependency names.

Exit status is `0` for a scan with no unapproved findings, `1` for violations and `2` for a tool/input
failure. Text diagnostics contain path, one-based line, one-based UTF-8 byte column,
rule and explanation. JSON contains `files`, a sorted `findings` array and visible
`exceptions` with their evidence and reasons. LOC findings include `effective_loc`;
re-export findings include their `declaration`. The checker never modifies Rust
source files.

## Enforced Rules

| Rule | Rejected source | Required change |
| --- | --- | --- |
| `wildcard-import` | Production `use x::*`, grouped globs, local enum globs and glob re-exports | Name imported items explicitly or qualify variants. |
| `parent-traversal` | Production `super::super::` chains, including grouped imports | Import from the owning module through `crate::...`. |
| `qualified-path` | Production paths at use sites with three or more components, paths starting at `crate`, `std`, `core`, `alloc` or a Cargo dependency, and leading `::` | Import the item or a meaningful short module name. |
| `reexport-whitelist` | Every `pub use`, including restricted visibility and test-only scopes | Import directly from the owner or document one exact file/declaration whitelist entry. Library roots and `mod.rs` receive no exemption. |
| `effective-loc` | Any scanned Rust file exceeding 1200 effective LOC | Split by responsibility, including test files. |
| `function-spacing` | Adjacent functions or methods without a separating blank line, including trait signatures and tests | Add a blank line before the following function's comments/attributes. Blank lines inside them do not count. |
| `item-spacing` | Adjacent definitions involving a struct, enum, union, trait, impl block, inline module or extern block, including pairs with functions | Add a blank line before the following definition's comments/attributes. Compact import, out-of-line module, type alias and constant groups are outside this rule. |
| `parse-error` | Rust syntax the pinned parser cannot parse, including missing tokens | Fix invalid syntax or investigate/update parser support; never silently skip the file. |

Explicit imports are the intended place for full paths:

```rust
use std::{fmt, sync::Arc};
use crate::error::RuntimeError;

fn render(error: Arc<RuntimeError>) -> fmt::Result {
    // Implementation omitted.
    Ok(())
}
```

Short `fmt::Display`, `io::Result`, `hir::Expr`, `Type::method` and a single
`super::item` are allowed. Generic argument paths are checked independently.
Qualified trait calls such as `<Type as Trait>::method`, attributes and derive
paths are excluded from the verbose-path rule. A short imported alias is allowed;
the checker does not resolve names or determine whether an alias is meaningful.

Re-export authorization is independent of file location. An intentional public
boundary must explain its owner and consumers in a whitelist entry. Moving a
re-export into `lib.rs`, `mod.rs` or a Cargo-declared library root does not approve
it. The whitelist does not authorize forwarding layers or unrelated visibility
growth; review those decisions against module ownership.

## Test Scope

Import and path rules apply to production scopes. Re-export authorization applies
to every parsed scope, including tests. The checker follows
inline and out-of-line modules, literal `#[path = "..."]` attributes and Cargo
test targets. A module whose `cfg` expression cannot be enabled with `test=false`
is treated as test-only. Examples and benchmarks use production rules.

For example, `cfg(all(test, feature = "extra"))` is test-only;
`cfg(any(test, feature = "extra"))` is not. Unrelated flags remain unknown so a
platform or feature does not accidentally exempt production source. `cfg_attr`
is not interpreted as a test exemption. Files reachable from both test and
production roots receive production checks. Unknown/unreachable entrypoints are
also checked as production. Tests never receive an LOC, spacing, parse-error or re-export exemption.

## Effective LOC

The count removes Rust line comments, documentation comments and nested block
comments, then counts nonblank physical lines. Mixed code/comment lines count
once. String and raw-string contents remain intact: `//` or `/*` inside a literal
are data, not comments. Nonblank lines in multiline strings, including source
fixtures, count toward the file limit. LF and CRLF have the same result.

Exactly 1200 effective lines pass; 1201 produce a finding unless a justified,
bounded exception applies. The default threshold is not configurable from the
command line. A generic debt entry cannot waive it.

## Justified Exceptions

Record deliberate exceptions in
[`scripts/structure-exceptions.toml`](../scripts/structure-exceptions.toml).
Review the exception together with the affected code and its design evidence.
No existing finding is exempt merely because it predates the checker.

An LOC exception names one file, explains why keeping it cohesive is preferable
to splitting, links a nonempty repository Markdown document containing evidence,
and sets a concrete upper bound. For example, an exhaustive table whose ordering
is reviewed as one unit can justify a larger file; unrelated implementations
accumulated in one file cannot.

```toml
[[exceptions]]
path = "crates/example/src/table.rs"
rule = "effective-loc"
max_loc = 1280
reason = "The ordered exhaustive table is reviewed against one specification."
evidence = "docs/table-layout-rationale.md"
```

A re-export whitelist entry names one file and one exact declaration. The rationale
must explain the public or internal API responsibility, intended consumers, and
why that boundary is clearer than direct owner imports.
Keeping a cohesive flat module as an API boundary can be reasonable; shortening
an inconvenient import by adding a forwarding layer is not sufficient evidence.

```toml
[[exceptions]]
path = "crates/example/src/api.rs"
rule = "reexport-whitelist"
declaration = "pub use crate::model::Model;"
reason = "This flat module owns the documented SDK model surface."
evidence = "docs/sdk-boundary-rationale.md"
```

These examples are illustrative and are not active exemptions. Evidence paths
must name existing repository Markdown files. The checker validates required
fields, file scope and exact declarations (ignoring whitespace), and rejects
duplicates, stale/ambiguous entries and unknown policy fields. LOC above the
reviewed ceiling fails again. Re-export changes require re-review; an exception
does not waive other rules, such as wildcard imports. Remove stale exceptions
when a split or API change makes them unnecessary. Approved findings remain
visible in both text and JSON reports.

The tool cannot prove that a natural-language justification is sound. That is a
code-review responsibility: include concrete responsibility/consumer evidence
and the alternatives considered, not only "too expensive to fix" or "CI fails".
Changing a justification or increasing a limit is a design decision to review,
not automatic permission for unrelated growth. Other rules have no per-file
exception mechanism; justified changes to those rules need a focused checker,
test and documentation change.

## Review Boundary

Tree-sitter parses source syntax; it does not compile code or expand macros.
Macro definitions and invocations contain token trees rather than resolved Rust
items/expressions, so their contents are excluded from import/path/spacing/re-export
checks. Their source still contributes to LOC. Literal strings are likewise not
reinterpreted as Rust code. Macro hygiene, macro-generated imports, `include!`,
conditional `#[path]` produced by `cfg_attr`, and build-generated source need
manual structural review. Do not hide handwritten implementation in macros,
strings or ignored directories to bypass the check.

The check also does not judge dependency cycles, architectural responsibility,
function complexity, unnecessary visibility or local imports with insufficient
justification. Those remain checkpoint review duties. Parser/tool failures are
reported as failures rather than an empty successful audit.
