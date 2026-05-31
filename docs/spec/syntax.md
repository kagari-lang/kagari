# Kagari Syntax Specification

This document defines the initial surface syntax for Kagari.
It is intended to serve as the language-facing specification, independent from any specific parser implementation.

The syntax follows these design constraints:

- Rust-inspired surface syntax where that improves familiarity
- No direct reproduction of Rust's lifetime or borrow system
- a compact grammar that can be extended without changing established source forms

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
IDENT ::= XID_START XID_CONTINUE* ;
```

The portable identifier subset is ASCII letters, digits, and `_`.
Implementations may accept broader Unicode identifiers only when they preserve the same token boundaries.

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
- block comments delimited by `/*` and `*/`

Block comments do not nest.

### Operators and Delimiters

The following token families are part of the source syntax:

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

- `const` is the syntax for compile-time immutable values.
- attributes provide the extensibility point for features such as reflection and security annotations
- examples of intended uses include `@reflect`, `@requires(...)`, and `@profile(...)`
- `pub` is the only explicit visibility marker in the source syntax
- unmarked declarations are private in their containing scope
- public top-level items form the module's public interface
- Rust-style scoped visibility forms such as `pub(crate)` and `pub(super)` are not part of the source syntax

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

trait_decl      ::= "trait" IDENT generic_param_clause? "{" trait_member* "}" ;

trait_member    ::= attribute* method_sig ";" ;

method_sig      ::= "fn" IDENT generic_param_clause? "(" method_param_list? ")" return_type? where_clause? ;
```

Notes:

- trait members are methods
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

```ebnf
type            ::= path generic_args?
                  | array_type
                  | tuple_type ;

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

Kagari does not include Rust reference type syntax such as `&T`, and it does not include caller-slot alias parameters.

Trait names may be used directly as interface types.
Kagari does not expose Rust-style `dyn Trait`, `Box<dyn Trait>`, or borrow-dependent trait object syntax.

The empty tuple type `()` is Kagari's unit type.
It represents the absence of a meaningful value and is the default result type for functions or module initialization paths that do not produce a value.
Source code does not need to spell a trailing `()` expression; a block with no tail expression produces `()`.

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

if_stmt         ::= "if" condition block ("else" (if_stmt | block))? ;

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

arg             ::= expr ;

field_suffix    ::= "." IDENT ;

index_suffix    ::= "[" expr "]" ;

primary_expr    ::= literal
                  | path
                  | parenthesized_expr
                  | tuple_expr
                  | array_expr
                  | struct_expr
                  | closure_expr
                  | match_expr
                  | block ;

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

- `range_expr` models the common `a..b` and `a..=b` forms.
- half-open forms such as `..b`, `a..`, and `..` are outside the current grammar.
- closure syntax is included at the surface level; capture behavior is specified in the non-grammatical constraints section.
- struct literals permit field shorthand such as `Point { x, y }`.

### Patterns

The core `match` grammar uses a deliberately small pattern language.

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

This keeps `match`, binding conditions, and destructuring usable without adopting Rust's full pattern grammar.
More advanced pattern forms are language extensions.

### Place Expressions

Some language rules need a narrower notion than general expressions.
For example, an assignment target must name a storage location rather than a temporary value.

```ebnf
place_expr      ::= path place_suffix*
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

## Future Language Extensions

The following areas are outside this syntax specification:

- visibility and module public-interface semantics
- associated items beyond methods
- extended pattern grammar
- closure capture semantics
- extended generic constraints and `where` predicates
- host-exposed type syntax

## Parser Guidance

This document is the language-facing syntax specification.
The parser implementation does not have to mirror these rules one-for-one.

When parser implementation begins:

- the parser grammar may be normalized for the chosen parsing strategy
- precedence handling may be encoded structurally rather than textually
- additional recovery-oriented productions may be introduced without changing the source-language syntax
