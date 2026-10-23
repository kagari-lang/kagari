# Kagari Tree-sitter grammar

This package owns recoverable source syntax, not static legality, name resolution
or compiler diagnostics. The source contracts are [syntax](../../docs/spec/syntax.md),
[EBNF](../../docs/kagari.ebnf) and the [source parser](../../crates/kagari-syntax/src/parser/grammar/).
It does not import Rust binding, lifetime, reference, `dyn` or prefix-await syntax.
Kagari characters, byte literals, raw Strings and async blocks are supported.

## Build and test

The parent `editors` npm workspace owns installation and its one lockfile. It pins
`tree-sitter-cli` **0.25.10** and native `tree-sitter` **0.25.1**. Generation uses
**ABI 15**. This package needs `node-addon-api` and `node-gyp-build`; the workspace
must also provide `node-gyp` for the explicit build script.

From `editors/`, after the parent installs dependencies:

```sh
npm run generate --workspace tree-sitter-kagari
npm run build --workspace tree-sitter-kagari
npm test --workspace tree-sitter-kagari
```

On Windows, if Python is managed by uv rather than on `PATH`, run `uv python find`
and set `PYTHON` to the returned executable path before building. Native compilation
also requires Visual Studio's C++ build tools. The local build used Python 3.12.12,
Node 22.21.1, node-gyp 11.5.0 and Visual Studio 2022 (17.14); the corpus compiler
was clang 16.0.4. No alternate Node build wrapper or compiler ABI is required.
Before the parent runs a clean workspace installation from `editors/`, PowerShell
can configure the same Python discovery explicitly:

```powershell
$env:PYTHON = uv python find
npm ci
```

`binding.gyp` consumes node-addon-api's headers directly, using the absolute
package directory returned by `require.resolve`, without an external addon gyp
target. Node 22 already supplies Node-API; the header-only C++ wrapper needs no
`node_api.gyp:nothing` library. External gyp targets can emit Visual Studio
projects into a nested `node_modules/node-addon-api` directory when dependencies
are hoisted, creating a package-less shadow that breaks npm dependency resolution.
Keeping generated projects under this package's `build/` avoids that collision.

The direct configuration preserves the exception behavior of node-addon-api
8.9.2's `node_addon_api_except` target: `NAPI_CPP_EXCEPTIONS`, removal of
`-fno-exceptions` on Unix, `/EHsc` and `_HAS_EXCEPTIONS=1` on Windows, and Xcode C++
exception support on macOS. This retains `Napi::Error` translation through the
module registration wrapper rather than silently switching to exception-disabled
error handling. No runtime/package dependency has been removed or replaced.

Generation requires no WebAssembly compiler. Tracked `src/parser.c`,
`src/grammar.json`, `src/node-types.json` and `src/tree_sitter/` headers support
native editor builds without Node or regeneration. There is no grammar Cargo
workspace/crate or unused generated Rust binding.

The Node entrypoint follows the upstream
[tree-sitter-javascript v0.25.0 binding](https://github.com/tree-sitter/tree-sitter-javascript/tree/v0.25.0/bindings/node):
`node-gyp-build` loads a matching packaged `prebuilds/` binary, or the local
`node-gyp` build. No prebuilt binaries are committed here; installations without a
matching prebuild require a native C/C++ toolchain and Node headers. The N-API
language external uses Tree-sitter's upstream language type tag, not a raw pointer
or the incompatible historical language wrapper. Both CommonJS and this interface
are covered by the native tests:

```js
import Parser from 'tree-sitter';
import Kagari from 'tree-sitter-kagari';
const parser = new Parser();
parser.setLanguage(Kagari);
const query = new Parser.Query(Kagari, '(identifier) @variable');
```

## Scanner ownership and recovery

`src/scanner.c` handles recursive block comments, cooked text boundaries,
identifier holes and whole raw tokens. It has **no persistent state**: create
returns `NULL`, serialization writes zero bytes, and deserialization is a no-op.
The LR parse stack selects the lexical context. Raw delimiters are counted only
within one scan (0..255 hashes), so incremental reuse cannot retain stale hash or
brace state after edits. The recovery sentinel refuses all-symbols-valid scans.

- Whole nested comments distinguish ordinary/documentation/unterminated forms.
  A missing closing delimiter extends through EOF, not just the next newline.
- Cooked `string_content` includes LF/CRLF, indentation and doubled braces;
  comments within text stay text. Escapes are `escape_sequence` nodes, including
  ASCII hexadecimal, Unicode underscores and physical line continuations.
  Scalar values still belong to the shared compiler decoder.
- `string_literal` has no holes. `interpolated_string` starts with the same quote
  and has at least one `interpolation`. Each hole has field `name` (an
  `identifier`), optional adjacent `interpolation_format` (`:` and `?`), and
  a closing brace. The scanner rejects reserved words; immediate delimiters and
  the boundary sentinel reject whitespace/comments. Expressions and old `f`
  prefixes are not accepted. Literal-only contexts exclude interpolated strings.
- `raw_string_literal` is one token including delimiters. Backslashes and braces
  are inert; a quote followed by fewer hashes stays content. Extra closing hashes
  are subsequent tokens. The token retains physical line endings and byte spans.
- Byte characters use `byte_char_literal`, with ASCII `byte_char_content` or
  `byte_char_escape_sequence`. Cooked `byte_string_literal` uses ASCII
  `byte_string_content` and `byte_escape_sequence`; byte hexadecimal escapes span
  x00..xFF and Unicode escapes are rejected. Braces never open holes. These nodes
  reuse character boundary guards and multiline/continuation scanning.
  `raw_byte_string_literal` reuses raw delimiter scanning with ASCII validation;
  raw backslashes stay literal. Byte strings construct arrays and occur only in
  expression literals. Pattern and attribute contexts share scalar literals,
  retaining byte characters but excluding cooked/raw byte strings (including
  nested attribute arrays). Range endpoint types remain a compiler check, not
  an extension to u8 ranges. No new persistent scanner state is introduced.
- Only EOF ends unclosed cooked/raw text, producing `unterminated_string_end` or
  `unterminated_raw_string`. Following lines may legally be content; no recovery
  to their apparent declarations is promised. Missing delimiters may additionally
  produce Tree-sitter `ERROR`/`MISSING` nodes.
- Raw byte strings use `unterminated_raw_byte_string` at EOF. Validity consumers
  reject all four explicit unterminated kinds (including
  `unterminated_block_comment`) as well as `hasError`. Highlighting still
  classifies their retained text as string/comment content.

No catch-all item/expression/token production conceals unsupported syntax.
Ordinary incomplete declarations, holes and delimiters use Tree-sitter's error
recovery. The compiler remains authoritative for invalid escape values, numeric
ranges, legal receivers, trait async support, assignment places and other static
restrictions; syntactic recognition is not executable support.

## Node API for editor queries

`src/node-types.json` is the generated, exact query API. Important categories:

- Declarations: `function_item`, `method_item`, `const_item`, `type_alias_item`, `struct_item`,
  `enum_item`, `enum_variant`, `trait_item`, `impl_item`, `module_item`, `use_item`,
  `associated_type`, `associated_const`, `field_declaration`, `visibility`,
  `attribute`, `attribute_arguments`, `attribute_argument`.
  Module `type_alias_item` requires `=` and exposes `name` and target `type` fields;
  generic parameters and an optional where clause (before `=` or after the target,
  not both) are retained as children.
  Trait/impl `associated_type` remains distinct. Native opaque `type Name;`
  declarations are not ordinary script module items.
- Names: `identifier`, `type_identifier`, `path`, `type_path`, `generic_path`.
  Declaration/member names have `name` fields; ordinary bindings have a `pattern`
  field, including a `path` for a simple name. `function_item` and
  `method_item` have `parameters` and `body`; calls have `function`/`arguments`;
  field access has `value`/`field`. Constructor expressions use field `type`.
- Types: `generic_type`, `generic_parameters`, `type_parameter`,
  `generic_arguments`, `associated_type_binding`, `qualified_type`, `array_type`,
  `tuple_type`, `parenthesized_type`, `function_type`, `never_type`,
  `trait_reference`, `type_bounds`, `where_clause`, `type_annotation`.
- Statements/expressions: `binding_statement`, `assignment_statement`,
  `expression_statement`, `return_statement`, `break_statement`,
  `continue_statement`, `while_statement`, `for_statement`, `binding_condition`,
  `block`, `binary_expression`, `range_expression`, `unary_expression`,
  `cast_expression`, `call_expression`, `field_expression`, `index_expression`,
  `propagation_expression`, `await_expression`, `closure_expression`,
  `closure_parameters`, `closure_parameter`, `if_expression`, `loop_expression`,
  `match_expression`, `match_arm`, `match_guard`, `struct_expression`.
- Patterns: `or_pattern`, `range_pattern`, `wildcard_pattern`, `tuple_pattern`,
  `parenthesized_pattern`, `tuple_struct_pattern`, `struct_pattern`, `field_pattern`,
  `rest_pattern`.
- Lexical nodes: `integer_literal`, `float_literal`, `boolean_literal`,
  `string_literal`, `string_content`, `escape_sequence`, `interpolated_string`,
  `raw_string_literal`, `interpolation`, `interpolation_format`, character nodes,
  byte nodes, the comment kinds and the four explicit invalid boundary kinds above.

Keywords and operators are anonymous tokens, including `val`/`var`, `.`/`await`,
`===`/`!==`, bitwise/compound operators, generic angles and closure/pattern bars.
Shifts are single adjacent tokens; generic `>` closers remain independently
lexable. Condition productions reserve bare braces for their control-flow bodies
while preserving ordinary expressions inside parentheses and call arguments.
Block-like final expressions retain the `block.tail` field; the same constructs
before another statement remain statements. Unparenthesized range endpoints do
not recursively accept another range. An open range before a `for` body does not
consume that body's brace as its endpoint.

### Struct update expressions

Struct expressions permit one terminal `..base` with an ordinary expression
operand, with or without preceding named/shorthand fields. A comma may separate
explicit fields from the base, but no comma may follow the base. For
`Point { x: 1, ..Default::default() }`, the query tree is:

```text
struct_expression
  type: path(identifier: Point)
  field_initializer
    name: identifier: x
    value: integer_literal: 1
  base: struct_update_base
    ..
    value: call_expression
      function: path(identifier: Default, identifier: default)
      arguments
```

The sketch omits other anonymous punctuation. The marker belongs to
`struct_update_base`, not `range_expression` or `rest_pattern`; its `value` may
itself be any ordinary expression, including a range or nested struct expression.
The nominal type/access rules remain the compiler's responsibility. Pattern rest
still has no operand and may have a trailing comma.

Missing operands, repeated/nonterminal bases, trailing base commas and `..=`
markers recover through ordinary `ERROR`/`MISSING` nodes, not a permissive valid
production. Recovery may omit the `base` child; clients must not assume a complete
update node in malformed source. Bind a constructed value outside a string before
interpolating its name; update expressions are not permitted inside holes. Existing Zed operator/name captures and lexical
TextMate rules cover this syntax without semantic guesses about `Default` or
other referenced names.

### Declaration patterns and struct rest

Ordinary `val`/`var` declarations share the existing pattern nodes used by match,
for and binding conditions. The declaration root excludes an unparenthesized
alternative; grouped and nested alternatives still use `or_pattern`. Named-field
struct patterns permit one final `rest_pattern` (`..`), optionally followed by a
comma. Tuple/slice rest remains unsupported. Complete field coverage, binding
consistency and irrefutability belong to static checking, not this editor grammar.

For `val (left, Point { x: renamed, .. }) = pair;`, the query tree is:

```text
binding_statement
  pattern: tuple_pattern
    path(identifier: left)
    struct_pattern
      type: path(identifier: Point)
      field_pattern
        name: identifier: x
        pattern: path(identifier: renamed)
      rest_pattern: ..
  value: path(identifier: pair)
```

The sketch omits anonymous punctuation except the rest token. `field_pattern`
without a `pattern` child is shorthand. The same nodes recover during incomplete
edits; recovery is not a claim that malformed patterns are valid declarations.
See the runnable [binding example](../../examples/syntax/binding-patterns.kgr).

## Validation scope

The corpus establishes concrete tree shapes at lexical/recovery boundaries. Native
unit tests load the actual Node addon, compile queries, parse
selected shared syntax examples and a small representative set, assert ambiguity
and unsupported-syntax boundaries, and compare complete incremental/fresh trees
(including anonymous tokens, fields, errors, ranges and positions) through quote,
hole/block-brace, pattern delimiter/rest, struct update marker/operand/delimiter
and nested-comment-terminator edits with LF and CRLF.

The complete examples inventory, adapter capture contracts, Zed integration and
platform matrix belong to the parent/CI. They are not run by this package's local
tests.

The parent guide records [validation scope](../README.md#validation-scope);
[the roadmap](../../docs/implementation-roadmap.md#toolchain-distribution-and-modes)
retains pending CI/platform acceptance. Native interface tests verify
default ESM imports, `setLanguage` and `Parser.Query`; a native parser passing
these tests is not evidence of a WASM build or editor installation.
