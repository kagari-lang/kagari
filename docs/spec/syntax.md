# Kagari Syntax Specification

This document defines the initial surface syntax for Kagari.
It is intended to serve as the language-facing specification, independent from any specific parser implementation.

The syntax follows these design constraints:

- Rust-inspired surface syntax where that improves familiarity
- No direct reproduction of Rust's lifetime or borrow system
- a compact grammar that can be extended without changing established source forms

Rust is the default reference for punctuation and grouping where Kagari has the
same source construct. Deliberate differences include `val`/`var` bindings,
`@` attributes, dynamically sized `[T]` arrays, and the absence of Rust borrow
and lifetime syntax. Rust spelling alone does not add an unsupported Kagari type
or runtime behavior.

Rules outside the stated scope are defined by the companion specifications for the relevant language subsystem.

## Scope

This document covers:

- notation used in the grammar
- lexical structure at a high level
- item, type, statement, and expression grammar
- semantic constraints that are not fully expressible in EBNF

This document does not define:

- full pattern matching semantics
- trait system and trait impls
- macro systems
- async or coroutine syntax
- full generic constraints
- module resolution semantics
- the final const evaluation rules

Trait-system rules are defined separately in [traits.md](traits.md).
Reflection rules are defined separately in [reflection.md](reflection.md).
Security rules are defined separately in [security.md](security.md).
Host interop rules are defined separately in [host-interop.md](host-interop.md).
Runtime model rules are defined separately in [runtime.md](runtime.md).
Execution model rules are defined separately in [execution.md](execution.md).
Module execution rules are defined separately in [modules.md](modules.md).

The executable subset currently includes field shorthand, remainder, inherent
method calls, tuple/struct/enum destructuring, `for` over built-in iterable
collections and strings, and `loop` expressions with `break` values.
Collection iteration blocks structural modification through aliases until
the loop exits. Closures use lexical capture; `var` captures share a GC-managed
cell, and `val` captures copy the value under the ordinary value model.
Function types use `fn(T, ...) -> R`.

## Grammar Notation

This specification uses an EBNF-style notation with the following conventions:

- terminals are written in double quotes, such as `"fn"` and `"return"`
- nonterminals are written as bare identifiers, such as `expr` and `function_decl`
- `A ::= B` means "A is defined as B"
- `|` separates alternatives
- `(...)` groups terms
- `?` means zero or one occurrence
- `*` means zero or more occurrences
- `+` means one or more occurrences

Example:

```ebnf
param_list ::= param ("," param)* (",")? ;
```

The notation above means that a parameter list contains one parameter, followed by zero or more comma-plus-parameter repetitions, with an optional trailing comma.

## Lexical Structure

The lexical rules below define the source token classes used by the grammar.

### Whitespace and Comments

Whitespace separates tokens but is otherwise insignificant except where needed to avoid token merging.

The language supports:

- line comments
- block comments

The comment token forms are defined in the comments section below.

### Identifiers

```ebnf
IDENT ::= IDENT_START IDENT_CONTINUE* ;
```

`IDENT_START` is an ASCII letter or `_`; `IDENT_CONTINUE` additionally accepts
ASCII digits. Unicode identifiers are not part of the current source grammar.
An unsupported Unicode scalar is one unknown token spanning all of its UTF-8
bytes. The lossless CST keeps the token and analysis reports a diagnostic; the
parser must never slice through a code point during recovery.

### Keywords

The following keywords are reserved:

- `as`
- `break`
- `const`
- `continue`
- `crate`
- `else`
- `enum`
- `false`
- `fn`
- `for`
- `trait`
- `if`
- `impl`
- `in`
- `loop`
- `match`
- `mod`
- `pub`
- `return`
- `self`
- `struct`
- `super`
- `true`
- `use`
- `val`
- `var`
- `type`
- `where`
- `while`

### Literals

The grammar uses the following literal token classes:

- `INTEGER`
- `FLOAT`
- `STRING`

#### Integer Literals

Integer literals include:

- decimal integers, such as `0`, `7`, `123`
- binary integers, such as `0b1010`
- octal integers, such as `0o755`
- hexadecimal integers, such as `0xff`
- `_` as a visual separator between digits

#### Floating-Point Literals

Floating-point literals include:

- `1.0`
- `0.5`
- `10e3`
- `6.02e23`

Underscore separators are allowed in the digit sequences.

#### String Literals

String literals are double-quoted.
Strings support the usual single-character escapes and Unicode escapes of the form `\u{...}`.

### Comments

The language reserves the following comment forms:

- line comments beginning with `//`
- block comments delimited by `/*` and `*/`; nested block comments are allowed

### Operators and Delimiters

The following token families are part of the source syntax:

- arithmetic operators: `+`, `-`, `*`, `/`, `%`
- logical operators: `!`, `&&`, `||`
- comparison operators: `==`, `!=`, `===`, `!==`, `<`, `<=`, `>`, `>=`
- assignment operators: `=`, `+=`, `-=`, `*=`, `/=`
- range operators: `..`, `..=`
- path and member operators: `::`, `.`
- function and match arrows: `->`, `=>`
- attribute introducer: `@`
- delimiters: `(`, `)`, `{`, `}`, `[`, `]`, `,`, `:`, `;`, `|`

## Grammar

### Module Structure

```ebnf
module          ::= item* EOF ;

item            ::= attribute* item_decl ;

item_decl       ::= use_decl
                  | module_decl
                  | function_item
                  | const_item
                  | struct_item
                  | enum_item
                  | trait_item
                  | impl_block ;

function_item   ::= visibility? function_decl ;

const_item      ::= visibility? "const" IDENT type_annotation? "=" expr ";" ;

struct_item     ::= visibility? struct_decl ;

enum_item       ::= visibility? enum_decl ;

visibility      ::= "pub" ;

attribute       ::= "@" path attribute_args? ;

attribute_args  ::= "(" attribute_arg_list? ")" ;

attribute_arg_list
                ::= attribute_arg ("," attribute_arg)* (",")? ;

attribute_arg   ::= IDENT "=" attribute_value
                  | attribute_value ;

attribute_value ::= literal
                  | path
                  | "[" attribute_arg_list? "]" ;
```

Notes:

- `const` is the syntax for compile-time immutable values.
- attributes provide the extensibility point for features such as reflection and security annotations
- examples of intended uses include `@reflect`, `@requires(...)`, and `@profile(...)`
- `@meta(...)` and names under `@tool::...` are preserved as structured,
  source-positioned analysis metadata. They do not change runtime behavior.
  `@reflect`, `@requires`, and `@profile` are reserved and diagnosed until their
  behavior is implemented; other unqualified names are diagnosed as unknown.
- `pub` and `pub(super)` are the explicit visibility markers in the source syntax
- unmarked declarations are private in their containing scope
- public top-level items form the module's public interface
- `pub(super)` makes an item available to the parent module tree; `pub(crate)` and `pub(in path)` are not part of the source syntax

### Functions

```ebnf
function_decl   ::= "fn" IDENT generic_param_clause? "(" param_list? ")" return_type? where_clause? block ;

generic_param_clause
                ::= "<" generic_param ("," generic_param)* (",")? ">" ;

generic_param   ::= IDENT (":" type_bound_list)? ;

type_bound_list ::= type_bound ("+" type_bound)* ;

type_bound      ::= trait_ref ;

param_list      ::= param ("," param)* (",")? ;

param           ::= IDENT ":" type ;

return_type     ::= "->" type ;
```

Notes:

- `x: T` is an ordinary parameter.
- parameters are local bindings and cannot be rebound.
- functions may declare generic parameters and a trailing `where` clause.
- trait bounds may use parameterized trait references such as `Into<String>`

### Modules and Imports

```ebnf
module_decl     ::= visibility? "mod" IDENT (";" | module_block) ;

module_block    ::= "{" item* "}" ;

use_decl        ::= visibility? "use" use_tree ";" ;

use_tree        ::= use_path use_tail?
                  | "{" use_tree_list? "}" ;

use_tail        ::= "as" IDENT
                  | "::" "*"
                  | "::" "{" use_tree_list? "}" ;

use_tree_list   ::= use_tree ("," use_tree)* (",")? ;

use_path        ::= path ;
```

Notes:

- `mod name;` declares a module through external loading rules defined elsewhere.
- `mod name { ... }` declares an inline module body.
- `use` supports aliasing, globs, and grouped import trees.
- An inline body creates a child module under the declaring module identity.
  Its public declarations can be reached through qualified paths; child source
  ranges retain their position in the physical file.
- A wildcard import expands the target module's public members, including
  members exposed by `pub use`. Local declarations and explicit imports take
  precedence. Conflicting wildcard members are diagnosed. A wildcard target
  must be a module. Relative import roots `self`, `super`, and `crate` resolve
  against the containing module.

### Structs and Enums

```ebnf
struct_decl     ::= "struct" IDENT generic_param_clause? "{" field_list? "}" ;

field_list      ::= field ("," field)* (",")? ;

field           ::= attribute* visibility? field_binding IDENT ":" type ;

field_binding   ::= "val"
                  | "var" ;

enum_decl       ::= "enum" IDENT generic_param_clause? "{" variant_list? "}" ;

variant_list    ::= variant ("," variant)* (",")? ;

variant         ::= IDENT
                  | IDENT "(" type_list? ")" ;

type_list       ::= type ("," type)* (",")? ;
```

### Traits

```ebnf
trait_item      ::= visibility? trait_decl ;

trait_decl      ::= "trait" IDENT generic_param_clause? supertrait_clause? "{" trait_member* "}" ;

supertrait_clause ::= ":" type_bound_list ;

trait_member    ::= attribute* method_sig (";" | block) | associated_type_decl ;
associated_type_decl ::= "type" IDENT (":" type_bound_list)? ";" ;

method_sig      ::= "fn" IDENT generic_param_clause? "(" method_param_list? ")" return_type? where_clause? ;
```

Notes:

- trait members are methods and ordinary associated types
- attributes on trait members are the intended hook for future reflection or security-related metadata

### Binding and Field Writeability

Kagari uses `val` and `var` for local bindings and struct fields.

`val` declares a slot that cannot be rebound after initialization.
`var` declares a slot that may be rebound or assigned after initialization.

Example:

```kagari
struct PlayerInfo {
    val id: PlayerId,
    var level: i32,
    pub var title: String,
}
```

Rules:

- `val x = ...` declares a local binding that cannot be rebound
- `var x = ...` declares a local binding that may be rebound
- `val field: T` declares a field that cannot be assigned after initialization
- `var field: T` declares a field that may be assigned through normal field or typed path mutation
- assigning to a `val` local or `val` field is rejected
- field writeability is not Rust borrowing and does not imply exclusive access
- host-backed fields follow the same source-level rule when exposed as Kagari fields, plus host policy checks

### Types

Type applications resolve their base name using the same bindings as named type
annotations. A generic parameter, explicit declaration, or import shadows an
unqualified standard type constructor (`Map`, `Set`, `Option`, or `Result`). An
ambiguous or unresolved explicit binding does not fall back to a standard type.
Explicit empty argument lists are invalid and are preserved during recovery;
`T<>` is never interpreted as `T`. Failed applications retain any known base and
argument declarations for tools, without making the application executable.
Navigation within an unknown argument does not select the enclosing base type.

A local binding annotation supplies type arguments to a Struct initializer with
the same declaration identity, including parameters absent from its fields.
For example, `val marker: Marker<bool> = Marker { value: 42 };` fixes `T = bool`
for `struct Marker<T> { val value: i32 }`. Known field types propagate this context
to nested Struct initializers. Field types and generic bounds are still checked;
an unrelated annotated type cannot supply constructor arguments. Function return
annotations also supply context to tail expressions and explicit return operands.
Context propagates through blocks, if/match branches, Tuple members and Array
elements. Enum constructors also consume matching nominal context, including
unit variants with or without parentheses. Known payload types propagate context
to nested constructors; arity, payload types and generic bounds remain checked.
Concrete parameter types of resolved local and imported functions supply argument
context, including through source facades. Extra arguments are still checked.
Trait methods use the same argument-context rules after substituting the receiver
for `Self`. A local generic function call first infers from its expected result,
then checks arguments in source order, making inferred concrete parameter types
available to subsequent arguments. Later arguments do not yet provide context
backwards to earlier constructors. Caller-owned generic binders are valid context,
including after trait `Self` substitution; unresolved callee binders are not.
Binder ownership, rather than parameter spelling, controls this distinction.
Uninferred binders leave unknown positions in an argument's context without
removing its independently known members. For example, a parameter
`(Token<i32>, T)` supplies `Token<i32>` to the first member of
`(Token::Empty, true)` while inferring `T` from the second member. Unknown
positions must be resolved before a generic call or constructor is accepted;
merely carrying an unknown position through a container does not infer it.
For tooling, failed inference preserves known members and nominal identities;
only unresolved positions become error types after the diagnostic. Such results
remain unavailable to code generation.
Constructor field and payload checks use the same finalized recovery types as
result queries; unresolved positions do not hide conflicts in known siblings.
Member lookup likewise uses a known nominal declaration even if some of its type
arguments are erroneous: absent fields still diagnose, and existing fields retain
their declaration identities and substituted types. Only a wholly unknown or
error receiver suppresses dependent missing-member diagnostics.
Similarly, a partially erroneous Tuple remains a known noninteger index and
produces an invalid-index diagnostic in addition to its member error. An index
whose entire type is unknown or erroneous suppresses that dependent diagnostic.
Comparison and logical expressions retain their bool result type for tooling even
when operands contain errors. Known operand conflicts and equality-capability
failures remain diagnostic; unknown positions alone do not create duplicate
operator errors. Recovery types do not authorize code generation.
Binary expressions check the left operand first and pass its type as RHS context
for arithmetic and comparisons. Logical operators provide bool context instead.
This allows `Token<i32>::Empty == Token::Empty` and contextual generic calls;
explicit RHS type arguments remain authoritative and are checked for conflicts.
Runtime evaluation remains left-to-right and short-circuit behavior is unchanged.
When enclosing context is absent, earlier Array element types supply context to
later elements. An if branch that can complete normally supplies context to its
else branch; prior reachable, normally completing match arms supply context to
later reachable arms. Returning branches do not determine the result type, and
explicit conflicting types still diagnose. This propagation is forward only.
Array element-type merging includes only sequentially reachable, normally
completing elements. If an element always returns, it and the following elements
do not contribute to the array's element join. Following expressions still receive
independent semantic diagnostics even though they will not execute.
An if/while condition must produce bool on paths that complete normally. A
condition that always returns produces no condition value and does not receive
a bool mismatch diagnostic, but its internal expressions remain checked.
If such a condition belongs to an if expression, neither branch contributes a
result type or supplies context to the other branch. Branches still receive
independent semantic diagnostics. Cancellation during condition checking remains
distinct from a condition that cannot complete normally.
The operand of `return` is compared with the function return type only on paths
where that operand completes normally. A nested return has already exited the
function and does not produce an additional outer return value. Its own return
type is still checked; a bare `return` continues to require a Unit return type.
Assignment RHS expressions receive the checked target type as context, including
local, field and index targets. This does not change target writeability checks.
An initializer or assignment RHS is compared against its destination type only
when it can complete normally. A terminating RHS supplies no value for an
ordinary or compound assignment, so no assignment operation type check or final
write occurs. Inner expression diagnostics and invalid-target diagnostics remain;
annotated local types remain available to semantic queries.
Empty Array literals and the resolved standard Map/Set constructors consume the
same expected-type context in every expression position. Constructor arity and
container kind must still match; context does not coerce incompatible elements.
Local container annotations enforce the same `Eq + Hash` requirements as function
signatures, including nested containers and forwarded generic constraints.
Struct fields and enum payloads also update inference in source order: an earlier
member can supply context to a later nested constructor. A member whose type is
already known does not wait for unrelated generic parameters to be inferred.
This also applies within composite members: a field or enum payload declared
`(Token<i32>, T)` preserves the first tuple element's context while inferring
`T` from the second element.

```ebnf
type            ::= path generic_args?
                  | array_type
                  | tuple_type
                  | function_type
                  | parenthesized_type
                  | qualified_type ;

function_type   ::= "fn" "(" type_list? ")" "->" type ;

array_type      ::= "[" type "]" ;

tuple_type      ::= "(" ")"
                  | "(" type "," (type ("," type)* (",")?)? ")" ;

parenthesized_type ::= "(" type ")" ;

generic_args    ::= "<" generic_arg ("," generic_arg)* (",")? ">" ;
generic_arg     ::= type | associated_type_binding ;
associated_type_binding ::= IDENT "=" type ;
qualified_type  ::= "<" type "as" trait_ref ">" "::" IDENT ;

where_clause    ::= "where" where_predicate ("," where_predicate)* (",")? ;

where_predicate ::= type ":" type_bound_list ;

trait_ref       ::= path generic_args? ;

path            ::= path_segment ("::" path_segment)* ;

path_segment    ::= IDENT
                  | "self"
                  | "super"
                  | "crate"
                  | "Self" ;
```

Kagari does not include Rust reference type syntax such as `&T`, and it does not include caller-slot alias parameters.

Trait names may be used directly as interface types.
Kagari does not expose Rust-style `dyn` trait-object, boxed trait-object, or borrow-dependent trait-object syntax.

The empty tuple type `()` is Kagari's unit type.
As in Rust, `(T)` groups a type and `(T,)` is a one-element tuple type.
It represents the absence of a meaningful value and is the default result type for functions that do not produce a value.
Source code does not need to spell a trailing `()` expression; a block with no tail expression produces `()`.

Ordinary associated types use `type Item;` in traits and `type Item = T;`
in trait implementations. Trait arguments can bind outputs (`Reader<Item = i32>`),
and types can project them (`Self::Item`, `R::Item`, `<R as Reader>::Item`).
A `where` target may be a generic parameter or its associated projection.
See [traits.md](traits.md#ordinary-associated-types) for the authoritative rules;
[the EBNF](../kagari.ebnf) defines their complete grammar.

### Impl Blocks and Methods

```ebnf
impl_block      ::= inherent_impl
                  | trait_impl ;

inherent_impl   ::= "impl" generic_param_clause? type where_clause? "{" impl_item* "}" ;

trait_impl      ::= "impl" generic_param_clause? trait_ref "for" type where_clause? "{" trait_impl_item* "}" ;
trait_impl_item ::= attribute* method_decl | associated_type_def ;
associated_type_def ::= "type" IDENT "=" type ";" ;

impl_item       ::= attribute* method_decl ;

method_decl     ::= visibility? "fn" IDENT generic_param_clause? "(" method_param_list? ")" return_type? where_clause? block ;

method_param_list
                ::= receiver_param ("," param_list)? (",")?
                  | param_list ;

receiver_param  ::= "self"
```

Notes:

- the language distinguishes inherent `impl` from `impl Trait for Type`
- method receivers do not introduce Rust-style borrowing or caller-slot aliasing
- method receivers use the ordinary parameter value model

### Blocks and Statements

```ebnf
block           ::= "{" stmt* expr? "}" ;

stmt            ::= binding_stmt
                  | assign_stmt
                  | expr_stmt
                  | return_stmt
                  | if_stmt
                  | while_stmt
                  | loop_stmt
                  | for_stmt
                  | break_stmt
                  | continue_stmt
                  | block ;

binding_stmt    ::= binding_kind IDENT type_annotation? init_expr? ";" ;

binding_kind    ::= "val"
                  | "var" ;

type_annotation ::= ":" type ;

init_expr       ::= "=" expr ;

assign_stmt     ::= place_expr assign_op expr ";" ;

assign_op       ::= "="
                  | "+="
                  | "-="
                  | "*="
                  | "/=" ;

expr_stmt       ::= expr ";" ;

return_stmt     ::= "return" expr? ";" ;

if_stmt         ::= if_expr ;

if_expr         ::= "if" condition block ("else" (if_expr | block))? ;

while_stmt      ::= "while" condition block ;

loop_stmt       ::= "loop" block ;

for_stmt        ::= "for" pattern "in" expr block ;

break_stmt      ::= "break" expr? ";" ;

continue_stmt   ::= "continue" ";" ;

condition       ::= binding_condition
                  | expr ;

binding_condition
                ::= "val" pattern "=" expr ;
```

Kagari keeps Rust-like blocks and control-flow shape while using Kotlin-like `val` and `var` binding declarations.

- `val x = ...` declares a local binding that cannot be rebound.
- `var x = ...` declares a local binding that may be rebound.
- function parameters are local bindings and cannot be rebound.
- `const` declares a compile-time immutable value.
- writeability of fields, host paths, and host APIs is controlled by `val`/`var`, type rules, and host policy.
- `for` syntax is Rust-like and iterates over values accepted by the language iterable protocol.

Function return checking distinguishes normal completion from explicit `return`.
The tail expression (or Unit when absent) must match the declared result only when
control can reach the end. Each explicit return operand is checked independently.
When a call argument or aggregate member returns from the enclosing function,
later arguments/members and the enclosing call/construction are not executed.
If the callee expression itself exits, explicit arguments are likewise skipped.
HIR records this termination explicitly rather than inventing a callable target;
argument source still receives independent diagnostics, and a normally produced
non-callable value remains invalid.
Those unreachable generic calls do not create concrete instances.
For script functions, a terminating argument supplies neither a parameter value
nor a generic inference constraint. Such calls do not require missing generic
arguments to be inferred, but arity checks and independent argument diagnostics
still apply. An argument with a normally completing path must match the parameter
type on that path.
Script function, trait method and standard-library parameter type comparisons
share this completion rule. This includes concrete standard parameters such as
the bool condition and String message of debug assertions; arity remains checked
even when argument evaluation will exit the enclosing function.
Math min/max/clamp and abs constrain only operands that can produce values;
debug assert_eq likewise compares only produced operand types. Other operands
still receive their own constraint diagnostics. A call terminated while evaluating
its operands requires no concrete result layout, since no result is produced.
Enum payload arguments use the same parameter comparison rule. A constructor
terminated by a payload does not require otherwise missing generic arguments or
a concrete enum layout. Explicit type arguments, payload arity and independently
invalid normally completing payload values remain checked.
Struct field initializers follow the same value-production rule: terminating
fields supply no generic inference constraint or assigned value. A construction
that exits during field evaluation requires no missing generic arguments or
result layout. Missing, duplicate and unknown fields, and independently invalid
normally completing field values, still receive diagnostics.
Unary negation and logical not constrain their operands only on normally
completing paths. Inner errors remain visible. Negation with no produced operand
has no inferred numeric result. IR registers expression result layouts through a
single normal-completion boundary, including nested tuples and arrays.
Binary operands likewise contribute type constraints only when they produce a
value. Known invalid counterpart types remain errors even when the operation
cannot execute. A terminating left operand does not supply RHS inference context.
Logical operators retain bool results on short-circuit paths; ordinary arithmetic
with an absent operand has no inferred numeric result.
The same termination rule applies to operators and runtime helper arguments.
A short-circuit operator can still complete along the path that skips its right
operand. A return during a while condition or assignment RHS exits the function
before the loop branch or assignment commit.
If evaluation of an assignment target's receiver or index returns, no target
location is produced. Remaining indexes, the RHS and the final write are skipped
for both ordinary and compound assignment.
An index expression must produce an integer only on normally completing paths.
When index evaluation always exits, an Array retains its known element type;
a Tuple has no selected member type. Invalid receiver kinds and Tuple binding
writeability remain checked, including reflection index writes.
A field read whose receiver cannot complete has no produced receiver or member
value. It does not resolve a field target or result layout. Missing member names
and receiver expression errors still diagnose; normally completing receivers
retain the usual member existence and type checks.
Index reads likewise require a normally produced receiver before applying
receiver/index compatibility rules. An index expression still receives its
independent semantic diagnostics when the receiver always exits, but it does
not execute and the read has no inferred element value.
A match scrutinee that cannot complete normally supplies no value to compare
against patterns, and no arm contributes to the match result type. Arm source
still receives independent diagnostics. When the scrutinee has a completing
path, pattern compatibility and arm result joins remain checked.
Match arms are considered in source order. A wildcard or binding pattern matches
every remaining value, so subsequent arms cannot contribute result types or
completion paths and do not generate concrete generic instances. Their source
still receives independent name and expression diagnostics.
Branches that return or otherwise cannot complete do not contribute a value type
to an if/match join. Unreachable source still receives semantic diagnostics.
An unconditional loop completes only through a reachable break in that loop;
breaks in nested loops do not exit it. While-loop completion conservatively
includes the zero-iteration path, without constant-condition evaluation.

### Expressions

```ebnf
expr            ::= range_expr ;

range_expr      ::= logic_or_expr (range_op logic_or_expr)? ;

range_op        ::= ".."
                  | "..=" ;

logic_or_expr   ::= logic_and_expr ("||" logic_and_expr)* ;

logic_and_expr  ::= equality_expr ("&&" equality_expr)* ;

equality_expr   ::= compare_expr (("==" | "!=" | "===" | "!==") compare_expr)* ;

compare_expr    ::= additive_expr (("<" | "<=" | ">" | ">=") additive_expr)* ;

additive_expr   ::= multiplicative_expr (("+" | "-") multiplicative_expr)* ;

multiplicative_expr
                ::= unary_expr (("*" | "/" | "%") unary_expr)* ;

unary_expr      ::= ("!" | "-") unary_expr
                  | postfix_expr ;

postfix_expr    ::= primary_expr postfix_op* ;

postfix_op      ::= call_suffix
                  | field_suffix
                  | index_suffix ;

call_suffix     ::= "(" arg_list? ")" ;

arg_list        ::= arg ("," arg)* (",")? ;

arg             ::= expr ;

field_suffix    ::= "." IDENT ;

index_suffix    ::= "[" expr "]" ;

primary_expr    ::= literal
                  | path
                  | explicit_enum_path
                  | if_expr
                  | parenthesized_expr
                  | tuple_expr
                  | array_expr
                  | struct_expr
                  | closure_expr
                  | loop_expr
                  | match_expr
                  | block ;

loop_expr       ::= "loop" block ;

parenthesized_expr
                ::= "(" expr ")" ;

tuple_expr      ::= "(" expr "," expr_list? ")" ;

expr_list       ::= expr ("," expr)* (",")? ;

array_expr      ::= "[" expr_list? "]" ;

struct_expr     ::= path generic_args? "{" field_init_list? "}" ;

explicit_enum_path ::= path generic_args "::" IDENT ;

field_init_list ::= field_init ("," field_init)* (",")? ;

field_init      ::= IDENT
                  | IDENT ":" expr ;

closure_expr    ::= "|" closure_param_list? "|" closure_body ;

closure_param_list
                ::= closure_param ("," closure_param)* (",")? ;

closure_param   ::= IDENT (":" type)? ;

closure_body    ::= expr
                  | block ;

match_expr      ::= "match" expr "{" match_arm_list? "}" ;

match_arm_list  ::= match_arm ("," match_arm)* (",")? ;

match_arm       ::= pattern match_guard? "=>" match_body ;

match_guard     ::= "if" expr ;

match_body      ::= expr
                  | block ;

literal         ::= INTEGER
                  | FLOAT
                  | STRING
                  | "true"
                  | "false" ;
```

### Expression Notes

- A `match` guard is evaluated after its pattern binds names. It must be `bool`;
  a false guard continues to the next arm, even for an otherwise irrefutable
  pattern. Guards are evaluated only for matching patterns.
- `if val pattern = expr` and `while val pattern = expr` evaluate `expr` once
  per condition check. On a match, names are visible only in the then branch or
  loop body. A failed match selects `else` or exits the loop.
- `|` pattern alternatives share one binding scope. Each alternative must bind
  the same names with the same types. Integer range patterns accept `i32`
  literal or local scalar `const` bounds; `..` excludes the upper bound and
  `..=` includes it.
- `range_expr` models the common `a..b` and `a..=b` forms.
- Both bounds are evaluated once, left to right, and must be `i32`. A range
  produces a fresh `[i32]` array in ascending order; `..` excludes its end and
  `..=` includes it. A start above the end produces an empty array. Materializing
  the range consumes ordinary execution budget and allocation resources.
- half-open forms such as `..b`, `a..`, and `..` are outside the current grammar.
- closure syntax is included at the surface level; capture behavior is specified in the non-grammatical constraints section.
- struct literals permit field shorthand such as `Point { x, y }`.
- Enum constructors accept `Token<i32>::Empty`, `Token<i32>::Empty()` and
  `Token<i32>::Data(7)`. The explicit arguments belong to the enum declaration,
  use normal annotation resolution and bound checks, and supply payload context.
  Qualified and imported enum paths use the same declaration identity. Explicit
  arguments override contextual inference; incompatible enclosing types reject.
- Struct literals accept explicit type arguments, such as `Marker<i32> { value: 7 }`.
  These arguments use annotation name resolution, arity and bound checks, and
  supply the field context even when an enclosing expression expects another type;
  incompatible enclosing types are diagnosed. The parser recognizes constructor
  arguments only when the closing `>` is followed by `{`, preserving comparisons.

Unknown annotation members retain error types without suppressing independent
constraints on known members. For example, `Map<f32, Missing>` reports the invalid
Eq + Hash as well as the unknown type. An unknown key by itself does not establish
an Eq + Hash violation; nested known key and element applications are still checked.
User-type bounds follow the same rule when the outer type determines the result:
`[Missing]` satisfies identity-based Eq + Hash and Iterable, but not numeric
bounds. Recovery holes do not disprove a structural protocol; known siblings
still enforce their requirements. General trait implementation lookup waits for
the member types it needs to resolve.
Standard bounds use the same rules in function calls and user-type applications.
For example, a caller's `T: PartialEq` also satisfies PartialEq for `(T, i32)`;
an unconstrained `T` does not establish that recursive requirement.

### Patterns

Patterns use the same grouped, tuple, alternative and range punctuation as Rust,
within Kagari's supported pattern kinds.

```ebnf
pattern         ::= or_pattern ;

or_pattern      ::= range_pattern ("|" range_pattern)* ;

range_pattern   ::= primary_pattern range_pattern_tail? ;

range_pattern_tail ::= ".." range_bound
                     | "..=" range_bound ;

range_bound     ::= literal | path ;

primary_pattern ::= "_" | literal | path | tuple_struct_pattern
                  | struct_pattern | tuple_pattern | parenthesized_pattern ;

parenthesized_pattern ::= "(" pattern ")" ;

tuple_pattern   ::= "(" ")"
                  | "(" pattern "," (pattern ("," pattern)* (",")?)? ")" ;

pattern_list    ::= pattern ("," pattern)* (",")? ;

tuple_struct_pattern
                ::= path "(" pattern_list? ")" ;

struct_pattern  ::= path "{" field_pattern_list? "}" ;

field_pattern_list
                ::= field_pattern ("," field_pattern)* (",")? ;

field_pattern   ::= IDENT
                  | IDENT ":" pattern ;
```

`(p)` groups a pattern; `(p,)` is a one-element tuple pattern. Kagari does not
currently accept all of Rust's reference, slice and rest patterns.

### Place Expressions

Some language rules need a narrower notion than general expressions.
For example, an assignment target must name a storage location rather than a temporary value.

```ebnf
place_expr      ::= path place_suffix*
                  | postfix_expr place_suffix+
                  | parenthesized_place_expr ;

parenthesized_place_expr
                ::= "(" place_expr ")" ;

place_suffix    ::= "." IDENT
                  | "[" expr "]" ;
```

This category is used by semantic rules, even where the grammar above still permits a broader `expr`.

## Non-Grammatical Constraints

The following rules are part of the language design, but cannot be fully expressed in EBNF alone:

### Closure Capture Semantics

- closures use lexical scope
- closures may implicitly capture outer local bindings
- a closure parameter may omit its type only when a contextual `fn(...) -> ...`
  type supplies that parameter type
- captured `var` bindings that may be assigned by the closure are represented through a shared environment slot
- captured bindings that are only read may be captured by value or by handle according to the runtime value model
- object-like values follow the ordinary value model when captured; if the value is a shared object handle, the closure and outer scope observe the same underlying object
- each `for` iteration introduces a fresh loop binding for capture purposes

Examples:

```kagari
val x = 1;
val read = || x;          // captures value 1

var n = 0;
val inc = || { n = n + 1; };
inc();
inc();                    // n is now 2
```

### Rebinding Rules

- assigning to a local variable requires that the local binding be declared with `var`
- assigning to a function parameter is rejected
- assigning to a `const` item is rejected
- assigning to a `val` field is rejected
- assigning to a `var` field is allowed, subject to type and host policy
- modifying the internal state of an object is distinct from rebinding the variable that refers to that object
- ordinary object mutation follows type, field, and host policy

### Ordinary Parameter Semantics

Ordinary parameters use the language's ordinary value model:

- primitive scalar values are copied
- object-like script values are passed as ordinary values according to the runtime object model
- parameters are not rebindable storage slots

The exact runtime meaning of object values is specified outside this syntax document.

Associated constant and type-family syntax and constraints are specified in
[traits.md](traits.md#generic-associated-types); the authoritative grammar is
[the EBNF](../kagari.ebnf).

## Future Language Extensions

The following areas are outside this syntax specification:

- package visibility and `pub(in path)`
- lifetime/const-parameterized associated types and associated type defaults
- extended pattern grammar
- higher-kinded parameters and advanced trait solving
- host-exposed type syntax

## Parser Guidance

This document is the language-facing syntax specification.
The parser implementation does not have to mirror these rules one-for-one.
The [syntax coverage audit](../syntax-coverage.md) records which EBNF forms
have parser witnesses, known implementation gaps, or no focused test yet.

When parser implementation begins:

- the parser grammar may be normalized for the chosen parsing strategy
- precedence handling may be encoded structurally rather than textually
- additional recovery-oriented productions may be introduced without changing the source-language syntax

## Parser diagnostic budget

Parsing accepts a per-file `ParseLimits::max_diagnostics` (default 256). This is
an ordinary-diagnostic budget: exactly that many diagnostics are allowed; the
next diagnostic produces one additional `KG_COMPILE_LIMIT_EXCEEDED` error at
its source location and stops grammar recovery. Zero accepts valid source and
stops at the first error. The unparsed suffix is retained verbatim in a CST error
node, including trivia and line endings; later declarations have no semantic
facts. Previously parsed declarations remain available to tools. Cancellation
still returns cancellation rather than a successful partial parse.

`AnalysisDatabase::set_parse_limits` and `KagariEngine::set_parse_limits` apply
this policy to subsequent queries and compilation. Changing the policy invalidates
cached declarations, signatures and bodies even when source revisions are equal;
existing snapshots remain immutable. Any limit error prevents code generation.
This diagnostic budget does not bound lexing, source size, syntax depth, or diagnostics
generated by name resolution and type checking; those require separate limits.

`ParseLimits::max_nesting` separately limits simultaneously active recursive
entries for expressions, prefix expressions, types, modules, import trees, blocks,
if expressions and match patterns (default 64). Fixed precedence helpers do not
consume additional entries; entries are released when they return, so sibling
expressions do not accumulate depth. Zero rejects any such entry. Exceeding the
budget reports `KG_COMPILE_LIMIT_EXCEEDED` for `parser nesting` at the next token,
then preserves the unparsed suffix using the same stop mechanism. This resource
error is additional to the ordinary diagnostic budget. Changing either parser
limit invalidates dependent queries. Hosts should choose conservative limits for
their thread stack size; raising this limit does not make parsing stackless.

`ParseLimits::max_tree_depth` bounds completed CST node depth (default 128).
Tokens do not count; leaf nodes have depth one. The parser tracks subtree depths
as it builds nodes, including checkpoint wrappers used by iterative binary and
postfix parsing. The first completed node deeper than the budget reports
`KG_COMPILE_LIMIT_EXCEEDED` for `syntax tree depth` and stops further grammar
work. Remaining tokens are retained verbatim. Already open ancestors still close,
so an error-bearing CST can exceed the budget by those enclosing nodes; it is
never accepted for code generation. Wide sibling lists do not accumulate depth.
This check complements the recursive-entry budget, which stops recursive descent
before nodes finish. Both are required; downstream traversals over externally
constructed HIR and generic expansion retain their own resource obligations.

## Option/Result propagation

`expr?` is a postfix operator, at the same parsing level as calls, member access
and indexing. It unwraps success or returns failure from the nearest function or
closure. It applies only to the built-in Option and Result types; see the
[standard-type contract](builtins.md#option-and-result) for type checking and
explicit conversion rules. There is no exception-handler syntax.
