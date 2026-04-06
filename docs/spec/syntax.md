# Kagari Syntax Specification Draft

This document defines the initial surface syntax for Kagari.
It is intended to serve as the language-facing specification, independent from any specific parser implementation.

The current draft follows these design constraints:

- Rust-inspired surface syntax where that improves familiarity
- No direct reproduction of Rust's lifetime or borrow system
- `ref` is a parameter-passing mode, not a reference type constructor
- Strong preference for a grammar that is easy to evolve during the early language-design phase

This is a draft specification. Rules in this document may change as the language model, runtime model, and host ABI become more concrete.

## Scope

This document currently covers:

- notation used in the grammar
- lexical structure at a high level
- item, type, statement, and expression grammar
- the grammar position of `ref`
- semantic constraints that are not fully expressible in EBNF

This document does not yet finalize:

- full pattern matching semantics
- trait system and trait impls
- macro systems
- async or coroutine syntax
- full generic constraints
- module resolution semantics
- the final const and static evaluation rules

Trait-system direction is drafted separately in [traits.md](/Users/mikai/CLionProjects/kagari/docs/spec/traits.md).
Reflection direction is drafted separately in [reflection.md](/Users/mikai/CLionProjects/kagari/docs/spec/reflection.md).
Security direction is drafted separately in [security.md](/Users/mikai/CLionProjects/kagari/docs/spec/security.md).
Host interop direction is drafted separately in [host-interop.md](/Users/mikai/CLionProjects/kagari/docs/spec/host-interop.md).
Runtime model direction is drafted separately in [runtime.md](/Users/mikai/CLionProjects/kagari/docs/spec/runtime.md).
Execution model direction is drafted separately in [execution.md](/Users/mikai/CLionProjects/kagari/docs/spec/execution.md).
Module execution direction is drafted separately in [modules.md](/Users/mikai/CLionProjects/kagari/docs/spec/modules.md).

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

The lexical rules below are intentionally simple in this draft. They can be tightened later without forcing a large rewrite of the syntax chapters.

### Whitespace and Comments

Whitespace separates tokens but is otherwise insignificant except where needed to avoid token merging.

The language is expected to support:

- line comments
- block comments

The exact comment syntax is not finalized in this document.

### Identifiers

```ebnf
IDENT ::= XID_START XID_CONTINUE* ;
```

Initial implementation may restrict identifiers to ASCII letters, digits, and `_`, even if the specification later expands to broader Unicode identifier support.

### Keywords

The following keywords are reserved in this draft:

- `as`
- `break`
- `const`
- `continue`
- `crate`
- `else`
- `dyn`
- `enum`
- `false`
- `fn`
- `for`
- `trait`
- `if`
- `impl`
- `in`
- `let`
- `loop`
- `match`
- `mod`
- `mut`
- `pub`
- `ref`
- `return`
- `self`
- `static`
- `struct`
- `super`
- `true`
- `use`
- `where`
- `while`

### Literals

The exact lexical form of literals is still provisional, but the grammar assumes the following token classes exist:

- `INTEGER`
- `FLOAT`
- `STRING`

#### Integer Literals

The current draft accepts:

- decimal integers, such as `0`, `7`, `123`
- binary integers, such as `0b1010`
- octal integers, such as `0o755`
- hexadecimal integers, such as `0xff`
- `_` as a visual separator between digits

#### Floating-Point Literals

The current draft accepts floating-point literals in the following general forms:

- `1.0`
- `0.5`
- `10e3`
- `6.02e23`

Underscore separators are allowed in the digit sequences.

#### String Literals

This draft currently specifies only double-quoted string literals.
Strings support the usual single-character escapes and Unicode escapes of the form `\u{...}`.

### Comments

The current draft reserves the following comment forms:

- line comments beginning with `//`
- block comments delimited by `/*` and `*/`

Whether block comments nest is not yet finalized.

### Operators and Delimiters

The following token families are part of the current draft:

- arithmetic operators: `+`, `-`, `*`, `/`, `%`
- logical operators: `!`, `&&`, `||`
- comparison operators: `==`, `!=`, `<`, `<=`, `>`, `>=`
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
                  | struct_item
                  | enum_item
                  | trait_item
                  | impl_block ;

function_item   ::= visibility? function_decl ;

struct_item     ::= visibility? struct_decl ;

enum_item       ::= visibility? enum_decl ;

visibility      ::= "pub" ;

attribute       ::= reflect_attribute
                  | security_attribute
                  | "@" path attribute_args? ;

reflect_attribute
                ::= "@reflect" attribute_args? ;

security_attribute
                ::= "@requires" attribute_args?
                  | "@profile" attribute_args? ;

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

- attributes provide the extensibility point for features such as reflection and security annotations
- examples of intended uses include `@reflect`, `@requires(...)`, and `@profile(...)`

### Functions

```ebnf
function_decl   ::= "fn" IDENT generic_param_clause? "(" param_list? ")" return_type? where_clause? block ;

generic_param_clause
                ::= "<" generic_param ("," generic_param)* (",")? ">" ;

generic_param   ::= IDENT (":" type_bound_list)? ;

type_bound_list ::= type_bound ("+" type_bound)* ;

type_bound      ::= trait_ref ;

param_list      ::= param ("," param)* (",")? ;

param           ::= param_mode? IDENT ":" type ;

param_mode      ::= "ref" ;

return_type     ::= "->" type ;
```

Notes:

- `ref` is attached to the parameter declaration, not to the type.
- `ref x: T` means that `x` is passed as an alias to the caller's variable slot.
- `x: T` is an ordinary parameter.
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

### Structs and Enums

```ebnf
struct_decl     ::= "struct" IDENT generic_param_clause? "{" field_list? "}" ;

field_list      ::= field ("," field)* (",")? ;

field           ::= attribute* visibility? IDENT ":" type ;

enum_decl       ::= "enum" IDENT generic_param_clause? "{" variant_list? "}" ;

variant_list    ::= variant ("," variant)* (",")? ;

variant         ::= IDENT
                  | IDENT "(" type_list? ")" ;

type_list       ::= type ("," type)* (",")? ;
```

### Traits

```ebnf
trait_item      ::= visibility? trait_decl ;

trait_decl      ::= "trait" IDENT generic_param_clause? "{" trait_member* "}" ;

trait_member    ::= attribute* method_sig ";" ;

method_sig      ::= "fn" IDENT generic_param_clause? "(" method_param_list? ")" return_type? where_clause? ;
```

Notes:

- trait members are currently methods only in this draft
- attributes on trait members are the intended hook for future reflection or security-related metadata

### Types

```ebnf
type            ::= path generic_args?
                  | dyn_type
                  | array_type
                  | tuple_type ;

dyn_type        ::= "dyn" trait_ref ;

array_type      ::= "[" type "]"
                  | "[" type ";" INTEGER "]" ;

tuple_type      ::= "(" type_list? ")" ;

generic_args    ::= "<" type ("," type)* (",")? ">" ;

where_clause    ::= "where" where_predicate ("," where_predicate)* (",")? ;

where_predicate ::= path_segment ":" type_bound_list ;

trait_ref       ::= path generic_args? ;

path            ::= path_segment ("::" path_segment)* ;

path_segment    ::= IDENT
                  | "self"
                  | "super"
                  | "crate"
                  | "Self" ;
```

This draft intentionally treats `ref` as outside the type grammar.
That is, `ref x: Vec<int>` is valid, while `x: ref Vec<int>` and `x: &Vec<int>` are not part of this design.

This draft also treats `dyn Trait` as a dedicated type form rather than as a general path.

### Impl Blocks and Methods

```ebnf
impl_block      ::= inherent_impl
                  | trait_impl ;

inherent_impl   ::= "impl" generic_param_clause? type where_clause? "{" impl_item* "}" ;

trait_impl      ::= "impl" generic_param_clause? trait_ref "for" type where_clause? "{" impl_item* "}" ;

impl_item       ::= attribute* visibility? method_decl ;

method_decl     ::= "fn" IDENT generic_param_clause? "(" method_param_list? ")" return_type? where_clause? block ;

method_param_list
                ::= receiver_param ("," param_list)? (",")?
                  | param_list ;

receiver_param  ::= "self"
                  | "mut" "self"
                  | "ref" "self" ;
```

Notes:

- the language distinguishes inherent `impl` from `impl Trait for Type`
- `mut self` means the local receiver binding may be rebound
- `ref self` aliases the caller-visible receiver slot rather than introducing Rust-style borrowing

### Blocks and Statements

```ebnf
block           ::= "{" stmt* expr? "}" ;

stmt            ::= let_stmt
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

let_stmt        ::= "let" mutability? IDENT type_annotation? init_expr? ";" ;

mutability      ::= "mut" ;

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

if_stmt         ::= "if" condition block ("else" (if_stmt | block))? ;

while_stmt      ::= "while" condition block ;

loop_stmt       ::= "loop" block ;

for_stmt        ::= "for" pattern "in" expr block ;

break_stmt      ::= "break" expr? ";" ;

continue_stmt   ::= "continue" ";" ;

condition       ::= let_condition
                  | expr ;

let_condition   ::= "let" pattern "=" expr ;
```

This draft keeps Rust-like blocks and control-flow shape, including `if let`, `while let`, and `for` forms.

Notes:

- `mut` applies to the variable binding, not to the type.
- `let x = ...` declares a non-rebindable local binding.
- `let mut x = ...` declares a rebindable local binding.
- `mut` does not imply Rust-style mutable borrowing or borrow checking.
- `mut` does not by itself determine whether object contents may be modified through methods or other operations.
- `for` syntax is Rust-like, but the iterable protocol is not yet specified in this document.

### Expressions

```ebnf
expr            ::= range_expr ;

range_expr      ::= logic_or_expr (range_op logic_or_expr)? ;

range_op        ::= ".."
                  | "..=" ;

logic_or_expr   ::= logic_and_expr ("||" logic_and_expr)* ;

logic_and_expr  ::= equality_expr ("&&" equality_expr)* ;

equality_expr   ::= compare_expr (("==" | "!=") compare_expr)* ;

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

arg             ::= "ref" place_expr
                  | expr ;

field_suffix    ::= "." IDENT ;

index_suffix    ::= "[" expr "]" ;

primary_expr    ::= literal
                  | path
                  | readonly_expr
                  | parenthesized_expr
                  | tuple_expr
                  | array_expr
                  | struct_expr
                  | closure_expr
                  | match_expr
                  | block ;

readonly_expr   ::= "readonly" "(" expr ")" ;

parenthesized_expr
                ::= "(" expr ")" ;

tuple_expr      ::= "(" expr "," expr_list? ")" ;

expr_list       ::= expr ("," expr)* (",")? ;

array_expr      ::= "[" expr_list? "]" ;

struct_expr     ::= path generic_args? "{" field_init_list? "}" ;

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

- `range_expr` currently models the common `a..b` and `a..=b` forms.
- half-open forms such as `..b`, `a..`, and `..` may be added later.
- closure syntax is included at the surface level; capture behavior is specified in the non-grammatical constraints section.
- struct literals permit field shorthand such as `Point { x, y }`.

### Patterns

The initial `match` design should start with a deliberately small pattern language.

```ebnf
pattern         ::= "_"
                  | literal
                  | path
                  | tuple_struct_pattern
                  | struct_pattern
                  | tuple_pattern ;

tuple_pattern   ::= "(" pattern_list? ")" ;

pattern_list    ::= pattern ("," pattern)* (",")? ;

tuple_struct_pattern
                ::= path "(" pattern_list? ")" ;

struct_pattern  ::= path "{" field_pattern_list? "}" ;

field_pattern_list
                ::= field_pattern ("," field_pattern)* (",")? ;

field_pattern   ::= IDENT
                  | IDENT ":" pattern ;
```

This keeps `match`, `if let`, and `while let` usable without immediately committing Kagari to Rust's full pattern grammar.
More advanced pattern forms can be layered in later.

### Place Expressions

Some language rules need a narrower notion than general expressions.
For example, a `ref` argument must name a caller-visible storage location rather than a temporary value.

```ebnf
place_expr      ::= path place_suffix*
                  | parenthesized_place_expr ;

parenthesized_place_expr
                ::= "(" place_expr ")" ;

place_suffix    ::= "." IDENT
                  | "[" expr "]" ;
```

This category is used by semantic rules, even where the grammar above still permits a broader `expr`.

## `ref` Parameter Passing

The `ref` feature in Kagari is a parameter-passing mode.
It is not intended to model Rust-style borrowing or C++-style reference types.

Example:

```kagari
fn swap(ref a: int, ref b: int) {
    let t = a;
    a = b;
    b = t;
}

swap(ref x, ref y);
```

The grammar-level consequences are:

- function parameters may be declared with `ref`
- call arguments may be marked with `ref`
- `ref` is not part of `type`
- `ref` aliases the caller's variable slot rather than constructing a new reference-typed value

## Non-Grammatical Constraints

The following rules are part of the language design, but cannot be fully expressed in EBNF alone:

### `ref` Argument Restrictions

- a `ref` argument must be a `place_expr`
- a `ref` argument cannot be a literal
- a `ref` argument cannot be a computed temporary value
- a `ref` argument cannot be the result of a function call

### `ref` Lifetime and Escape Restrictions

- a `ref` parameter is valid only during the dynamic extent of the call
- a `ref` parameter cannot be returned
- a `ref` parameter cannot be stored into heap-allocated script objects
- a `ref` parameter cannot be captured by closures
- a `ref` parameter cannot survive across suspension or reload boundaries

### Closure Capture Semantics

- closures use lexical scope
- closures may implicitly capture outer local bindings
- a non-rebindable binding is captured by value
- a binding declared with `mut` is captured through a shared environment slot
- object-like values follow the ordinary value model when captured; if the value is a shared object handle, the closure and outer scope observe the same underlying object
- `ref` bindings, `ref` parameters, and `ref self` cannot be captured by closures
- each `for` iteration introduces a fresh loop binding for capture purposes

Examples:

```kagari
let x = 1;
let read = || x;          // captures value 1

let mut n = 0;
let inc = || { n = n + 1; };
inc();
inc();                    // n is now 2
```

### Rebinding Rules

- assigning to a local variable requires that the local binding be declared with `mut`
- assigning through a `ref` parameter requires that the caller-provided binding be rebindable
- modifying the internal state of an object is distinct from rebinding the variable that refers to that object
- ordinary object mutation does not by itself require `mut` unless the operation is specified as rebinding the slot

### Aliasing Policy

This draft does not yet finalize whether the same caller slot may be passed to multiple `ref` parameters in one call.

Example open question:

```kagari
foo(ref x, ref x)
```

The language may either:

- reject this statically
- permit it with explicitly defined left-to-right write behavior

The recommended current direction is to reject it in early versions to keep aliasing rules simple.

### Ordinary Parameter Semantics

This draft assumes ordinary parameters use the language's ordinary value model:

- primitive scalar values are copied
- object-like script values are passed as ordinary values according to the runtime object model
- `ref` is required only when aliasing the caller's variable slot is desired
- `mut` affects whether a local or caller slot may be rebound, not whether a value is considered mutable in the Rust sense

The exact runtime meaning of object values is specified outside this syntax document.

## Future Work

Likely future sections include:

- visibility and export semantics
- trait-oriented syntax if adopted
- associated items beyond methods
- extended pattern grammar
- closure capture semantics
- extended generic constraints and `where` predicates
- host-exposed type syntax

## Parser Guidance

This document is the language-facing syntax specification.
It should not be treated as a promise that the parser implementation must mirror these rules one-for-one.

When parser implementation begins:

- the parser grammar may be normalized for the chosen parsing strategy
- precedence handling may be encoded structurally rather than textually
- additional recovery-oriented productions may be introduced without changing the source-language syntax
