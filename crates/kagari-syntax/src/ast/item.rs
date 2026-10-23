//! Declarations, attributes and visibility spellings.
//!
//! [`Item`] classifies direct declarations; it is not an additional CST wrapper.
//! Bodies, lists and use trees retain their own nesting. See [`crate::ast`] for
//! node diagram conventions and recovery behavior.

use crate::{
    ast::{
        expr::{BlockExpr, Expr, Literal},
        misc::{
            FieldList, GenericParamList, Name, ParamList, Path, TraitBoundList, TraitRef,
            VariantList, WhereClause,
        },
        support,
        traits::AstNode,
        ty::TypeRef,
    },
    kind::SyntaxKind,
    syntax_node::SyntaxNode,
};

use rowan::NodeOrToken;

ast_node!(
    /// The root syntax node for one parsed source text.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `fn total() { 1 }`:
    ///
    /// ```text
    /// SourceFile
    /// +-- FnDef (fn total() { 1 }) [node] -> items()
    /// `-- Eof "" [token]
    /// ```
    ///
    /// `items()` filters direct declarations, skipping trivia, EOF and recovery nodes.
    /// The syntax root contains no source file ID or revision. `module_documentation()`
    /// reads leading `//!` line-comment trivia, including trivia nested in the first item.
    SourceFile,
    SourceFile
);

ast_node!(
    /// A `#[...]` declaration annotation.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `#[tag(key = [1, source::value])]`:
    ///
    /// ```text
    /// Attribute
    /// +-- Hash "#" [token]
    /// +-- LBracket "[" [token]
    /// +-- Path (tag) [node] -> path()
    /// +-- AttributeArgs ((key = [1, source::value])) [node] -> args()
    /// `-- RBracket "]" [token]
    /// ```
    ///
    /// `#[tag = "x"]`:
    ///
    /// ```text
    /// Attribute
    /// +-- Hash "#" [token]
    /// +-- LBracket "[" [token]
    /// +-- Path (tag) [node] -> path()
    /// +-- Eq "=" [token]
    /// +-- AttributeValue ("x") [node] -> value()
    /// `-- RBracket "]" [token]
    /// ```
    ///
    /// An attribute may have no payload, parenthesized arguments, or an `=` value.
    /// `args()` and `value()` read distinct direct nodes; nested values belong to arguments.
    Attribute,
    Attribute
);

ast_node!(
    /// Parenthesized arguments of an attribute.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `(key = [1, source::value])`:
    ///
    /// ```text
    /// AttributeArgs
    /// +-- LParen "(" [token]
    /// +-- AttributeArgList (key = [1, source::value]) [node] -> arguments() via list
    /// `-- RParen ")" [token]
    /// ```
    ///
    /// `arguments()` traverses the direct `AttributeArgList`; empty `()` has no list
    /// and returns an empty iterator.
    AttributeArgs,
    AttributeArgs
);

ast_node!(
    /// Comma-separated attribute arguments or array elements.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `key = [1, source::value]`:
    ///
    /// ```text
    /// AttributeArgList
    /// `-- AttributeArg (key = [1, source::value]) [node]
    /// ```
    ///
    /// Delimiters belong to `AttributeArgs` or `AttributeValue`. The owning wrapper
    /// provides the iterator over these direct `AttributeArg` nodes.
    AttributeArgList,
    AttributeArgList
);

ast_node!(
    /// One positional or named attribute argument.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `key = [1, source::value]`:
    ///
    /// ```text
    /// AttributeArg
    /// +-- Name (key) [node]
    /// +-- Eq "=" [token]
    /// `-- AttributeValue ([1, source::value]) [node] -> value()
    /// ```
    ///
    /// A named argument has a direct `Name` before `=`. Positional arguments have no
    /// such name; identifiers inside their value are nested paths.
    AttributeArg,
    AttributeArg
);

ast_node!(
    /// A literal, path or array inside an attribute.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `[1, source::value]`:
    ///
    /// ```text
    /// AttributeValue
    /// +-- LBracket "[" [token]
    /// +-- AttributeArgList (1, source::value) [node] -> elements() via list
    /// `-- RBracket "]" [token]
    /// ```
    ///
    /// `"x"`:
    ///
    /// ```text
    /// AttributeValue
    /// `-- Literal ("x") [node] [Expr #0] -> literal()
    /// ```
    ///
    /// `source::value`:
    ///
    /// ```text
    /// AttributeValue
    /// `-- Path (source::value) [node] -> path()
    /// ```
    ///
    /// Literal, path and array forms use different child shapes. `elements()` descends
    /// through `AttributeArgList` only for the array form; it is empty for scalars.
    AttributeValue,
    AttributeValue
);

ast_node!(
    /// An inline `mod name { ... }` or external `mod name;` declaration.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `pub mod math { fn total() {} }`:
    ///
    /// ```text
    /// ModuleDef
    /// +-- PubKw "pub" [token]
    /// +-- ModKw "mod" [token]
    /// +-- Name (math) [node] -> name()
    /// `-- ModuleBlock ({ fn total() {} }) [node] -> block()
    /// ```
    ///
    /// `mod math;`:
    ///
    /// ```text
    /// ModuleDef
    /// +-- ModKw "mod" [token]
    /// +-- Name (math) [node] -> name()
    /// `-- Semi ";" [token]
    /// ```
    ///
    /// External declarations end in `;` and have no `ModuleBlock`. This crate records
    /// the spelling only; it does not locate or load another source file.
    ModuleDef,
    ModuleDef
);

ast_node!(
    /// The braced declarations of an inline module.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `{ fn total() {} }`:
    ///
    /// ```text
    /// ModuleBlock
    /// +-- LBrace "{" [token]
    /// +-- FnDef (fn total() {}) [node] -> items()
    /// `-- RBrace "}" [token]
    /// ```
    ModuleBlock,
    ModuleBlock
);

ast_node!(
    /// A `use` declaration with an optional visibility prefix.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `pub use math::{sum as total, value};`:
    ///
    /// ```text
    /// UseDecl
    /// +-- PubKw "pub" [token]
    /// +-- UseKw "use" [token]
    /// +-- UseTree (math::{sum as total, value}) [node] -> tree()
    /// `-- Semi ";" [token]
    /// ```
    ///
    /// `tree()` is the local syntactic use tree, not a resolved import record.
    /// Visibility is read from direct tokens, including `pub(super)`.
    UseDecl,
    UseDecl
);

ast_node!(
    /// One branch of a use declaration, retaining its local path prefix.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `math::{sum as total, value}`:
    ///
    /// ```text
    /// UseTree
    /// +-- Path (math) [node] -> path()
    /// +-- ColonColon "::" [token]
    /// +-- LBrace "{" [token]
    /// +-- UseTreeList (sum as total, value) [node] -> nested_trees() via list
    /// `-- RBrace "}" [token]
    /// ```
    ///
    /// `math::sum as total`:
    ///
    /// ```text
    /// UseTree
    /// +-- Path (math::sum) [node] -> path()
    /// +-- AsKw "as" [token]
    /// `-- Name (total) [node] -> alias()
    /// ```
    ///
    /// `math::*`:
    ///
    /// ```text
    /// UseTree
    /// +-- Path (math) [node] -> path()
    /// +-- ColonColon "::" [token]
    /// `-- Star "*" [token]
    /// ```
    ///
    /// A group nests through `UseTreeList`; its braces belong to the parent `UseTree`.
    /// `path()` contains only this branch's prefix. `alias()` finds a direct `Name` after
    /// `as`; an unaliased leaf returns `None`, even when its path has a final name.
    /// `nested_trees()` flattens one list level, not the whole recursive import tree.
    /// `is_glob()` checks only a direct `*`, not globs in nested branches.
    UseTree,
    UseTree
);

ast_node!(
    /// Comma-separated child use trees; enclosing braces belong to the parent.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `sum as total, value`:
    ///
    /// ```text
    /// UseTreeList
    /// +-- UseTree (sum as total) [node] -> trees()
    /// +-- Comma "," [token]
    /// `-- UseTree (value) [node] -> trees()
    /// ```
    UseTreeList,
    UseTreeList
);

ast_node!(
    /// A trait declaration with direct method, constant and associated-type children.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `pub trait Read<T>: Base { type Output; const ZERO: i32; fn read(self) -> T; }`:
    ///
    /// ```text
    /// TraitDef
    /// +-- PubKw "pub" [token]
    /// +-- TraitKw "trait" [token]
    /// +-- Name (Read) [node] -> name()
    /// +-- GenericParamList (<T>) [node] -> generic_params()
    /// +-- Colon ":" [token]
    /// +-- TraitBoundList (Base) [node] -> supertraits()
    /// +-- LBrace "{" [token]
    /// +-- AssociatedType (type Output;) [node] -> associated_types()
    /// +-- ConstDef (const ZERO: i32;) [node] -> associated_consts()
    /// +-- MethodDef (fn read(self) -> T;) [node] -> methods()
    /// `-- RBrace "}" [token]
    /// ```
    ///
    /// Members are direct children rather than a separate body-list node. The three
    /// member iterators independently filter methods, constants and associated types.
    TraitDef,
    TraitDef
);

ast_node!(
    /// An inherent or trait implementation for a type spelling.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `impl<T> Read<T> for Box<T> where T: Base { fn read(self) -> T { self.value } }`:
    ///
    /// ```text
    /// ImplBlock
    /// +-- ImplKw "impl" [token]
    /// +-- GenericParamList (<T>) [node] -> generic_params()
    /// +-- TraitRef (Read<T>) [node] -> trait_ref()
    /// +-- ForKw "for" [token]
    /// +-- TypeRef (Box<T>) [node] -> target_type()
    /// +-- WhereClause (where T: Base) [node] -> where_clause()
    /// +-- LBrace "{" [token]
    /// +-- MethodDef (fn read(self) -> T { self.value }) [node] -> methods()
    /// `-- RBrace "}" [token]
    /// ```
    ///
    /// `impl Point { fn x(self) {} }`:
    ///
    /// ```text
    /// ImplBlock
    /// +-- ImplKw "impl" [token]
    /// +-- TypeRef (Point) [node] -> target_type()
    /// +-- LBrace "{" [token]
    /// +-- MethodDef (fn x(self) {}) [node] -> methods()
    /// `-- RBrace "}" [token]
    /// ```
    ///
    /// Inherent implementations omit `TraitRef` and `for`. `target_type()` reads the
    /// first direct `TypeRef`; types inside the trait reference are not candidates.
    ImplBlock,
    ImplBlock
);

ast_node!(
    /// A trait or implementation method, with a body or terminating semicolon.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `fn read(self, n: i32) -> i32;`:
    ///
    /// ```text
    /// MethodDef
    /// +-- FnKw "fn" [token]
    /// +-- Name (read) [node] -> name()
    /// +-- LParen "(" [token]
    /// +-- ParamList (self, n: i32) [node] -> param_list()
    /// +-- RParen ")" [token]
    /// +-- Arrow "->" [token]
    /// +-- TypeRef (i32) [node] -> return_type()
    /// `-- Semi ";" [token]
    /// ```
    ///
    /// `fn x(self) -> i32 { self.x }`:
    ///
    /// ```text
    /// MethodDef
    /// +-- FnKw "fn" [token]
    /// +-- Name (x) [node] -> name()
    /// +-- LParen "(" [token]
    /// +-- ParamList (self) [node] -> param_list()
    /// +-- RParen ")" [token]
    /// +-- Arrow "->" [token]
    /// +-- TypeRef (i32) [node] -> return_type()
    /// `-- BlockExpr ({ self.x }) [node] [Expr #0] -> body()
    /// ```
    ///
    /// A method without a body ends in `;`, so `body()` returns `None`. A leading
    /// `self` parameter has a name but no explicit type child. Return types, generic
    /// parameters and where clauses can be absent.
    MethodDef,
    MethodDef
);

ast_node!(
    /// A function declaration and its optional generic/signature components.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `pub fn total<T>(x: T) -> T where T: Base { x }`:
    ///
    /// ```text
    /// FnDef
    /// +-- PubKw "pub" [token]
    /// +-- FnKw "fn" [token]
    /// +-- Name (total) [node] -> name()
    /// +-- GenericParamList (<T>) [node] -> generic_params()
    /// +-- LParen "(" [token]
    /// +-- ParamList (x: T) [node] -> param_list()
    /// +-- RParen ")" [token]
    /// +-- Arrow "->" [token]
    /// +-- TypeRef (T) [node] -> return_type()
    /// +-- WhereClause (where T: Base) [node] -> where_clause()
    /// `-- BlockExpr ({ x }) [node] [Expr #0] -> body()
    /// ```
    ///
    /// Parentheses belong to `FnDef`, not `ParamList`. The return type is the first
    /// direct `TypeRef`; parameter types are nested inside parameters. Generic parameters,
    /// return type and where clause are optional. Declaration mode permits `;` in place
    /// of a body; recovered source can also lack required children.
    FnDef,
    FnDef
);

ast_node!(
    /// A constant declaration; associated constants can omit their initializer.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `pub const LIMIT: i32 = 4;`:
    ///
    /// ```text
    /// ConstDef
    /// +-- PubKw "pub" [token]
    /// +-- ConstKw "const" [token]
    /// +-- Name (LIMIT) [node] -> name()
    /// +-- Colon ":" [token]
    /// +-- TypeRef (i32) [node] -> ty()
    /// +-- Eq "=" [token]
    /// +-- Literal (4) [node] [Expr #0] -> initializer()
    /// `-- Semi ";" [token]
    /// ```
    ///
    /// A type annotation is syntactically optional at module level. Trait/impl
    /// associated constants require a type spelling and may omit `= expression`;
    /// `initializer()` then returns `None`. Semantic acceptance belongs to HIR.
    ConstDef,
    ConstDef
);

ast_node!(
    /// A module-only `static val` or `static var` declaration.
    ///
    /// `#[tag] pub static var count: i32 = next();` has these direct children
    /// (trivia omitted; descendants collapsed):
    ///
    /// ```text
    /// StaticDef
    /// +-- Attribute (#[tag]) [node] -> attributes()
    /// +-- PubKw "pub" [token] -> visibility()
    /// +-- StaticKw "static" [token]
    /// +-- VarKw "var" [token] -> is_mutable()
    /// +-- Name (count) [node] -> name()
    /// +-- Colon ":" [token]
    /// +-- TypeRef (i32) [node] -> ty()
    /// +-- Eq "=" [token]
    /// +-- CallExpr (next()) [node] [Expr #0] -> initializer()
    /// `-- Semi ";" [token]
    /// ```
    ///
    /// Attributes and visibility are optional; `val` replaces `var` for an
    /// immutable binding. Name, type and initializer are required by the grammar
    /// but may be missing or incomplete after recovery. Accessors filter direct
    /// children, excluding expressions inside attributes. Local and associated
    /// forms are retained as error nodes, not static declarations.
    StaticDef,
    StaticDef
);

ast_node!(
    /// A nominal struct declaration with named fields.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `pub struct Point<T> { val x: T, var y: T }`:
    ///
    /// ```text
    /// StructDef
    /// +-- PubKw "pub" [token]
    /// +-- StructKw "struct" [token]
    /// +-- Name (Point) [node] -> name()
    /// +-- GenericParamList (<T>) [node] -> generic_params()
    /// +-- LBrace "{" [token]
    /// +-- FieldList (val x: T, var y: T) [node] -> field_list()
    /// `-- RBrace "}" [token]
    /// ```
    StructDef,
    StructDef
);

ast_node!(
    /// A nominal enum declaration with unit or tuple-payload variants.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `enum State<T> { Ready, Value(T) }`:
    ///
    /// ```text
    /// EnumDef
    /// +-- EnumKw "enum" [token]
    /// +-- Name (State) [node] -> name()
    /// +-- GenericParamList (<T>) [node] -> generic_params()
    /// +-- LBrace "{" [token]
    /// +-- VariantList (Ready, Value(T)) [node] -> variant_list()
    /// `-- RBrace "}" [token]
    /// ```
    EnumDef,
    EnumDef
);

ast_node!(
    /// A module-level transparent alias for a type spelling.
    ///
    /// The alias introduces no runtime type identity. Resolution and bound checking
    /// belong to HIR; this wrapper only retains the source spelling.
    ///
    /// # Tree shape
    ///
    /// `pub type Items<T: Read> = Vec<T> where T: Write;` (trivia omitted):
    ///
    /// ```text
    /// TypeAliasDef
    /// +-- PubKw "pub" [token] -> visibility()
    /// +-- TypeKw "type" [token]
    /// +-- Name (Items) [node] -> name()
    /// +-- GenericParamList (<T: Read>) [node] -> generic_params()
    /// +-- Eq "=" [token]
    /// +-- TypeRef (Vec<T>) [node] -> ty()
    /// +-- WhereClause (where T: Write) [node] -> where_clause()
    /// `-- Semi ";" [token]
    /// ```
    ///
    /// Attributes precede visibility as direct children. Visibility, generics and
    /// the where clause are optional; the where clause may instead precede `=`.
    /// Required name/target nodes may be missing or incomplete after recovery.
    /// Module-level aliases have this kind in both ordinary and declaration mode;
    /// associated types and declaration-mode opaque types use [`AssociatedType`].
    TypeAliasDef,
    TypeAliasDef
);

impl TypeAliasDef {
    /// Returns the written visibility; without `pub`, the alias is private.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns `Items` in `type Items<T> = Vec<T>;`.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns the alias name text, or `None` when its name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns `<T: Read>` in `type Items<T: Read> = Vec<T>;`.
    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }

    /// Returns the optional `where T: Read` before `=` or after the target type.
    pub fn where_clause(&self) -> Option<WhereClause> {
        support::child(self.syntax())
    }

    /// Returns the target `Vec<T>` in `type Items<T> = Vec<T>;`.
    /// An absent target during recovery returns `None`.
    pub fn ty(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }
}

ast_node!(
    /// An associated type declaration/definition or declaration-mode opaque type.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `type Output<T>: Base = T where T: Base;`:
    ///
    /// ```text
    /// AssociatedType
    /// +-- TypeKw "type" [token]
    /// +-- Name (Output) [node] -> name()
    /// +-- GenericParamList (<T>) [node] -> generic_params()
    /// +-- Colon ":" [token]
    /// +-- TraitBoundList (Base) [node] -> bounds()
    /// +-- Eq "=" [token]
    /// +-- TypeRef (T) [node] -> ty()
    /// +-- WhereClause (where T: Base) [node] -> where_clause()
    /// `-- Semi ";" [token]
    /// ```
    ///
    /// The assigned type, bounds, generic parameters and where clause are optional.
    /// The same kind represents top-level opaque types in declaration mode.
    /// Visibility syntax on this node is consumed only in declaration mode.
    AssociatedType,
    AssociatedType
);

impl AssociatedType {
    /// Returns `Visibility::Public` for `pub type Output;`, and `Private` without `pub`.
    /// Prefixes `pub(super)` and `pub(crate)` select `PublicSuper` and `PublicCrate` respectively.
    /// Reads direct tokens; does not check whether the visibility is legal in this context.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns `<T>` in `type Output<T>;`; returns `None` in `type Output;`.
    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }

    /// Returns `where T: Read` in `type Output<T> where T: Read;`.
    /// Returns `None` when no where clause is written.
    pub fn where_clause(&self) -> Option<WhereClause> {
        support::child(self.syntax())
    }

    /// Returns `Output` in `type Output<T> = Box<T>;`.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns `Some("Output")` for `type Output = i32;`, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns the assigned `i32` in `type Output = i32;`.
    /// Returns `None` for a declaration such as `type Output: Read;`.
    pub fn ty(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }

    /// Returns `Read + Write` in `type Output: Read + Write;`.
    /// Returns `None` for `type Output = i32;` when no bounds are written.
    pub fn bounds(&self) -> Option<TraitBoundList> {
        support::child(self.syntax())
    }
}

/// The visibility spelling on a declaration; access checking happens in HIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// No `pub` prefix; visibility defaults to private.
    Private,
    /// The `pub` prefix.
    Public,
    /// The `pub(super)` prefix.
    PublicSuper,
    /// The `pub(crate)` prefix.
    PublicCrate,
}

pub(crate) fn visibility_of(syntax: &SyntaxNode) -> Visibility {
    if support::token(syntax, SyntaxKind::PubKw).is_none() {
        Visibility::Private
    } else if support::token(syntax, SyntaxKind::SuperKw).is_some() {
        Visibility::PublicSuper
    } else if support::token(syntax, SyntaxKind::CrateKw).is_some() {
        Visibility::PublicCrate
    } else {
        Visibility::Public
    }
}

/// A typed choice of declarations found directly in a file or module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// An opaque declaration-mode type, such as `type Handle;`.
    TypeDecl(AssociatedType),
    /// A transparent module-level alias, such as `type Items<T> = Vec<T>;`.
    TypeAliasDef(TypeAliasDef),
    /// An inline module, such as `mod math { fn total() {} }`.
    ModuleDef(ModuleDef),
    /// An import, such as `use math::{Point, sum as total};`.
    UseDecl(UseDecl),
    /// A trait declaration, such as `trait Read { fn read(self) -> i32; }`.
    TraitDef(TraitDef),
    /// An inherent `impl Point {}` or trait implementation `impl Read for Point {}`.
    ImplBlock(ImplBlock),
    /// A function declaration, such as `fn next(x: i32) -> i32 { x + 1 }`.
    FnDef(FnDef),
    /// A constant declaration, such as `const LIMIT: i32 = 10;`.
    ConstDef(ConstDef),
    /// A module static, such as `static var count: i32 = 0;`.
    StaticDef(StaticDef),
    /// A struct declaration, such as `struct Point { val x: i32, var y: i32 }`.
    StructDef(StructDef),
    /// An enum declaration, such as `enum Slot<T> { Empty, Value(T) }`.
    EnumDef(EnumDef),
}

impl AstNode for Item {
    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::AssociatedType
                | SyntaxKind::TypeAliasDef
                | SyntaxKind::ModuleDef
                | SyntaxKind::UseDecl
                | SyntaxKind::TraitDef
                | SyntaxKind::ImplBlock
                | SyntaxKind::FnDef
                | SyntaxKind::ConstDef
                | SyntaxKind::StaticDef
                | SyntaxKind::StructDef
                | SyntaxKind::EnumDef
        )
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        match syntax.kind() {
            SyntaxKind::AssociatedType => AssociatedType::cast(syntax).map(Self::TypeDecl),
            SyntaxKind::TypeAliasDef => TypeAliasDef::cast(syntax).map(Self::TypeAliasDef),
            SyntaxKind::ModuleDef => ModuleDef::cast(syntax).map(Self::ModuleDef),
            SyntaxKind::UseDecl => UseDecl::cast(syntax).map(Self::UseDecl),
            SyntaxKind::TraitDef => TraitDef::cast(syntax).map(Self::TraitDef),
            SyntaxKind::ImplBlock => ImplBlock::cast(syntax).map(Self::ImplBlock),
            SyntaxKind::FnDef => FnDef::cast(syntax).map(Self::FnDef),
            SyntaxKind::ConstDef => ConstDef::cast(syntax).map(Self::ConstDef),
            SyntaxKind::StaticDef => StaticDef::cast(syntax).map(Self::StaticDef),
            SyntaxKind::StructDef => StructDef::cast(syntax).map(Self::StructDef),
            SyntaxKind::EnumDef => EnumDef::cast(syntax).map(Self::EnumDef),
            _ => None,
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::TypeDecl(node) => node.syntax(),
            Self::TypeAliasDef(node) => node.syntax(),
            Self::ModuleDef(node) => node.syntax(),
            Self::UseDecl(node) => node.syntax(),
            Self::TraitDef(node) => node.syntax(),
            Self::ImplBlock(node) => node.syntax(),
            Self::FnDef(node) => node.syntax(),
            Self::ConstDef(node) => node.syntax(),
            Self::StaticDef(node) => node.syntax(),
            Self::StructDef(node) => node.syntax(),
            Self::EnumDef(node) => node.syntax(),
        }
    }
}

impl Item {
    /// Iterates `#[first]` then `#[second]` on `#[first] #[second] fn run() {}`.
    /// Does not include attributes on nested declarations; an unannotated item yields none.
    pub fn attributes(&self) -> impl Iterator<Item = Attribute> {
        self.syntax().children().filter_map(Attribute::cast)
    }
}

impl Attribute {
    /// Returns `tools::tag` in `#[tools::tag(value = 1)]`.
    pub fn path(&self) -> Option<Path> {
        support::child(self.syntax())
    }

    /// Returns `Some("tools::tag")` for `#[tools::tag]`, omitting trivia.
    /// Returns `None` if no readable path remains.
    pub fn name_text(&self) -> Option<String> {
        self.path().and_then(|path| path.text())
    }

    /// Returns `(key = 1)` in `#[tag(key = 1)]`.
    /// Returns `None` for `#[tag]` or `#[tag = 1]`; the latter uses `value()`.
    pub fn args(&self) -> Option<AttributeArgs> {
        support::child(self.syntax())
    }

    /// Returns `1` in `#[tag = 1]`.
    /// Returns `None` for `#[tag(key = 1)]`; that value belongs to an argument under `args()`.
    pub fn value(&self) -> Option<AttributeValue> {
        support::child(self.syntax())
    }
}

impl AttributeArgs {
    /// Iterates `1` then `key = 2` in `(1, key = 2)`; `()` yields no arguments.
    /// Traverses one argument-list level, without flattening nested array values.
    pub fn arguments(&self) -> impl Iterator<Item = AttributeArg> {
        self.syntax()
            .children()
            .filter_map(AttributeArgList::cast)
            .flat_map(|list| list.syntax().children().filter_map(AttributeArg::cast))
    }
}

impl AttributeArg {
    /// Returns `Some("key")` for `key = 1`; returns `None` for positional `1` or `source::value`.
    pub fn name_text(&self) -> Option<String> {
        support::child::<Name>(self.syntax()).and_then(|name| name.text())
    }

    /// Returns `1` for both positional `1` and named `key = 1`.
    /// For `key = [1, 2]`, returns the whole array value.
    pub fn value(&self) -> Option<AttributeValue> {
        support::child(self.syntax())
    }
}

impl AttributeValue {
    /// Returns `1` for literal value `1`; returns `None` for path `source::value` or array `[1]`.
    pub fn literal(&self) -> Option<Literal> {
        support::child(self.syntax())
    }

    /// Returns `source::value` for that path value; returns `None` for literal `1` or array `[1]`.
    pub fn path(&self) -> Option<Path> {
        support::child(self.syntax())
    }

    /// Iterates `1` then `source::value` in `[1, source::value]` as attribute arguments.
    /// Scalar values and `[]` yield no elements; nested arrays are not flattened.
    pub fn elements(&self) -> impl Iterator<Item = AttributeArg> {
        self.syntax()
            .children()
            .filter_map(AttributeArgList::cast)
            .flat_map(|list| list.syntax().children().filter_map(AttributeArg::cast))
    }
}

impl ModuleDef {
    /// Returns `Visibility::Public` for `pub mod math {}`, and `Private` without `pub`.
    /// Prefixes `pub(super)` and `pub(crate)` select `PublicSuper` and `PublicCrate` respectively.
    /// Reads direct tokens; does not check whether the visibility is legal in this context.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns `math` in `mod math { fn total() {} }`.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns `Some("math")` for `mod math {}`, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns `{ fn total() {} }` in `mod math { fn total() {} }`.
    /// Returns `None` when no inline module body is present.
    pub fn block(&self) -> Option<ModuleBlock> {
        support::child(self.syntax())
    }
}

impl ModuleBlock {
    /// Iterates `fn a() {}` then `const B: i32 = 1;` in `mod m { fn a() {} const B: i32 = 1; }`.
    /// Does not flatten nested modules; an empty body yields no items.
    pub fn items(&self) -> impl Iterator<Item = Item> {
        support::children(self.syntax())
    }
}

impl UseDecl {
    /// Returns `Visibility::Public` for `pub use math::Point;`, and `Private` without `pub`.
    /// Prefixes `pub(super)` and `pub(crate)` select `PublicSuper` and `PublicCrate` respectively.
    /// Reads direct tokens; does not check whether the visibility is legal in this context.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns `math::{sum, Point}` in `use math::{sum, Point};`.
    pub fn tree(&self) -> Option<UseTree> {
        support::child(self.syntax())
    }
}

impl UseTree {
    /// Whether this branch imports `*`: true for `math::*`, false for `math::Point`.
    /// The outer `math::{*}` is also false; its nested `*` branch is true.
    pub fn is_glob(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::Star).is_some()
    }

    /// Returns `math` in `math::{Point, sum}` or `math::*`, and `math::Point` in `math::Point`.
    /// A bare `*` branch has no path and returns `None`.
    pub fn path(&self) -> Option<Path> {
        support::child(self.syntax())
    }

    /// Returns `P` in `math::Point as P`; returns `None` in `math::Point`.
    /// Selects the first direct name after `as`; a missing recovered alias also yields `None`.
    pub fn alias(&self) -> Option<Name> {
        let mut after_as = false;
        self.syntax()
            .children_with_tokens()
            .find_map(|element| match element {
                NodeOrToken::Token(token) if token.kind() == SyntaxKind::AsKw => {
                    after_as = true;
                    None
                }
                NodeOrToken::Node(node) if after_as => Name::cast(node),
                _ => None,
            })
    }

    /// Iterates `Point` then `ops::{sum}` in `math::{Point, ops::{sum}}`.
    /// Traverses one list level only; `sum` remains nested. Leaf and glob branches yield no trees.
    pub fn nested_trees(&self) -> impl Iterator<Item = UseTree> {
        self.syntax()
            .children()
            .filter_map(UseTreeList::cast)
            .flat_map(|list| list.trees().collect::<Vec<_>>())
    }
}

impl UseTreeList {
    /// Iterates `Point` then `sum as total` in the list of `use math::{Point, sum as total};`.
    pub fn trees(&self) -> impl Iterator<Item = UseTree> {
        support::children(self.syntax())
    }
}

impl TraitDef {
    /// Iterates `const ZERO: i32;` in `trait Read { const ZERO: i32; fn read(self); }`.
    /// Excludes methods and associated types; only direct constants are selected.
    pub fn associated_consts(&self) -> impl Iterator<Item = ConstDef> {
        support::children(self.syntax())
    }

    /// Returns `Read + Write` in `trait Stream: Read + Write {}`.
    /// Returns `None` for `trait Stream {}`.
    pub fn supertraits(&self) -> Option<TraitBoundList> {
        support::child(self.syntax())
    }

    /// Iterates `type Output;` in `trait Read { type Output; fn read(self); }`.
    /// Excludes methods and constants.
    pub fn associated_types(&self) -> impl Iterator<Item = AssociatedType> {
        support::children(self.syntax())
    }

    /// Returns `Visibility::Public` for `pub trait Read {}`, and `Private` without `pub`.
    /// Prefixes `pub(super)` and `pub(crate)` select `PublicSuper` and `PublicCrate` respectively.
    /// Reads direct tokens; does not check whether the visibility is legal in this context.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns `Read` in `trait Read<T> {}`.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns `Some("Read")` for `trait Read<T> {}`, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns `<T>` in `trait Read<T> {}`; returns `None` in `trait Read {}`.
    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }

    /// Iterates `fn read(self);` in `trait Read { type Output; fn read(self); }`.
    /// Excludes associated types and constants.
    pub fn methods(&self) -> impl Iterator<Item = MethodDef> {
        support::children(self.syntax())
    }
}

impl ImplBlock {
    /// Iterates `const ZERO: i32 = 0;` in `impl Read for Point { const ZERO: i32 = 0; }`.
    /// Excludes methods and associated types.
    pub fn associated_consts(&self) -> impl Iterator<Item = ConstDef> {
        support::children(self.syntax())
    }

    /// Iterates `type Output = i32;` in `impl Read for Point { type Output = i32; }`.
    /// Excludes methods and constants.
    pub fn associated_types(&self) -> impl Iterator<Item = AssociatedType> {
        support::children(self.syntax())
    }

    /// Returns `<T>` in `impl<T> Read for Box<T> {}`; returns `None` in `impl Point {}`.
    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }

    /// Returns `Read<i32>` in `impl Read<i32> for Point {}`.
    /// Returns `None` for an inherent implementation such as `impl Point {}`.
    pub fn trait_ref(&self) -> Option<TraitRef> {
        support::child(self.syntax())
    }

    /// Returns `Box<T>` in `impl<T> Read<T> for Box<T> {}`, or `Point` in `impl Point {}`.
    /// Selects the first direct type; trait arguments are nested under `trait_ref()`.
    pub fn target_type(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).next()
    }

    /// Returns `where T: Read` in `impl<T> Box<T> where T: Read {}`.
    /// Returns `None` when no where clause is written.
    pub fn where_clause(&self) -> Option<WhereClause> {
        support::child(self.syntax())
    }

    /// Iterates `fn x(self) {}` then `fn y(self) {}` in `impl Point { fn x(self) {} fn y(self) {} }`.
    /// Excludes associated types and constants.
    pub fn methods(&self) -> impl Iterator<Item = MethodDef> {
        support::children(self.syntax())
    }
}

impl MethodDef {
    /// Whether the declaration explicitly uses `async`: true for `async fn read() {}`, false for `fn read() {}`.
    /// Does not inspect the body or infer whether the result is a Future.
    pub fn is_async(&self) -> bool {
        self.syntax()
            .children_with_tokens()
            .any(|child| child.kind() == SyntaxKind::AsyncKw)
    }

    /// Returns `Visibility::Public` for `pub fn read(self);`, and `Private` without `pub`.
    /// Prefixes `pub(super)` and `pub(crate)` select `PublicSuper` and `PublicCrate` respectively.
    /// Reads direct tokens; does not check whether the visibility is legal in this context.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns `read` in `fn read(self, n: i32) -> i32;`.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns `Some("read")` for `fn read(self);`, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns `<T>` in `fn read<T>(self, value: T) {}`; returns `None` in `fn read(self) {}`.
    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }

    /// Returns the list `self, n: i32` in `fn read(self, n: i32);`, excluding parentheses.
    /// An empty parameter list has no parameters; a missing recovered list yields `None`.
    pub fn param_list(&self) -> Option<ParamList> {
        support::child(self.syntax())
    }

    /// Returns `i32` in `fn read(self) -> i32;`; returns `None` in `fn read(self);`.
    /// Parameter types are nested and cannot be selected as the return type.
    pub fn return_type(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).next()
    }

    /// Returns `where T: Read` in `fn read<T>(self, value: T) where T: Read;`.
    /// Returns `None` when no where clause is written.
    pub fn where_clause(&self) -> Option<WhereClause> {
        support::child(self.syntax())
    }

    /// Returns `{ self.x }` in `fn x(self) -> i32 { self.x }`.
    /// Returns `None` for a bodyless declaration such as `fn x(self) -> i32;`.
    pub fn body(&self) -> Option<BlockExpr> {
        support::child(self.syntax())
    }
}

impl SourceFile {
    /// Returns `"Math helpers"` for leading `//! Math helpers` before `fn total() {}`.
    /// Joins leading inner-doc lines with newlines; ordinary comments and outer `///` docs are excluded.
    /// Stops at the first non-trivia token; returns an empty string when no inner docs are found.
    pub fn module_documentation(&self) -> String {
        self.syntax()
            .descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .take_while(|token| token.kind().is_trivia())
            .filter(|token| token.kind() == SyntaxKind::LineComment)
            .filter_map(|token| {
                token
                    .text()
                    .strip_prefix("//!")
                    .map(|line| line.strip_prefix(' ').unwrap_or(line).to_owned())
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Iterates `fn a() {}` then `mod b {}` in `fn a() {} mod b {}`.
    /// Selects direct declarations only; does not descend into module `b`.
    pub fn items(&self) -> impl Iterator<Item = Item> {
        support::children(self.syntax())
    }
}

impl FnDef {
    /// Whether the declaration explicitly uses `async`: true for `async fn read() {}`, false for `fn read() {}`.
    /// Does not inspect the body or infer whether the result is a Future.
    pub fn is_async(&self) -> bool {
        self.syntax()
            .children_with_tokens()
            .any(|child| child.kind() == SyntaxKind::AsyncKw)
    }

    /// Returns `Visibility::Public` for `pub fn read() {}`, and `Private` without `pub`.
    /// Prefixes `pub(super)` and `pub(crate)` select `PublicSuper` and `PublicCrate` respectively.
    /// Reads direct tokens; does not check whether the visibility is legal in this context.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns `total` in `fn total<T>(x: T) -> T { x }`.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns `Some("total")` for `fn total() {}`, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns the list `x: i32, y: i32` in `fn add(x: i32, y: i32) {}`, excluding parentheses.
    /// An empty parameter list has no parameters; a missing recovered list yields `None`.
    pub fn param_list(&self) -> Option<ParamList> {
        support::child(self.syntax())
    }

    /// Returns `<T>` in `fn identity<T>(x: T) -> T { x }`.
    /// Returns `None` in `fn identity(x: i32) -> i32 { x }`.
    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }

    /// Returns `i32` in `fn total() -> i32 { 1 }`; returns `None` in `fn total() {}`.
    /// Reports only the written return type, not an inferred or default type.
    pub fn return_type(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).next()
    }

    /// Returns `{ x + 1 }` in `fn next(x: i32) -> i32 { x + 1 }`.
    /// Returns `None` for bodyless `fn next(x: i32) -> i32;` in declaration mode or missing recovered bodies.
    pub fn body(&self) -> Option<BlockExpr> {
        support::child(self.syntax())
    }

    /// Returns `where T: Read` in `fn read<T>(x: T) where T: Read {}`.
    /// Returns `None` when no where clause is written.
    pub fn where_clause(&self) -> Option<WhereClause> {
        support::child(self.syntax())
    }
}

impl ConstDef {
    /// Returns `Visibility::Public` for `pub const LIMIT: i32 = 10;`, and `Private` without `pub`.
    /// Prefixes `pub(super)` and `pub(crate)` select `PublicSuper` and `PublicCrate` respectively.
    /// Reads direct tokens; does not check whether the visibility is legal in this context.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns `LIMIT` in `const LIMIT: i32 = 10;`.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns `Some("LIMIT")` for `const LIMIT: i32 = 10;`, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns `i32` in `const LIMIT: i32 = 10;`.
    pub fn ty(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).next()
    }

    /// Returns `10` in `const LIMIT: i32 = 10;`.
    /// Returns `None` for an associated declaration such as `const LIMIT: i32;`.
    pub fn initializer(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }
}

impl StaticDef {
    /// Returns the direct visibility prefix, or `Private` when absent.
    /// Supports `pub`, `pub(super)` and `pub(crate)` as on [`ConstDef`].
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns `count` in `static var count: i32 = 0;`.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns `i32` in `static var count: i32 = 0;`, if retained after recovery.
    pub fn ty(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }

    /// Returns the first direct expression, `next()` in `static val count: i32 = next();`.
    /// Returns `None` when the initializer is missing after recovery.
    pub fn initializer(&self) -> Option<Expr> {
        support::child(self.syntax())
    }

    /// Iterates direct attributes in source order, excluding initializer attributes.
    pub fn attributes(&self) -> impl Iterator<Item = Attribute> {
        support::children(self.syntax())
    }

    /// Returns true for a direct `var` token, false for `val` or a missing keyword.
    /// This describes rebinding syntax, not deep immutability of the stored value.
    pub fn is_mutable(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::VarKw).is_some()
    }
}

impl StructDef {
    /// Returns `Visibility::Public` for `pub struct Point {}`, and `Private` without `pub`.
    /// Prefixes `pub(super)` and `pub(crate)` select `PublicSuper` and `PublicCrate` respectively.
    /// Reads direct tokens; does not check whether the visibility is legal in this context.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns `Point` in `struct Point { val x: i32 }`.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns `Some("Point")` for `struct Point {}`, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns the field list `val x: i32` in `struct Point { val x: i32 }`.
    /// Braces belong to the declaration; use the list's `fields()` to visit fields.
    pub fn field_list(&self) -> Option<FieldList> {
        support::child(self.syntax())
    }

    /// Returns `<T>` in `struct Box<T> { val value: T }`; returns `None` in `struct Point {}`.
    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }
}

impl EnumDef {
    /// Returns `Visibility::Public` for `pub enum Flag { On, Off }`, and `Private` without `pub`.
    /// Prefixes `pub(super)` and `pub(crate)` select `PublicSuper` and `PublicCrate` respectively.
    /// Reads direct tokens; does not check whether the visibility is legal in this context.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns `Slot` in `enum Slot<T> { Empty, Value(T) }`.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns `Some("Slot")` for `enum Slot<T> { Empty, Value(T) }`, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns the variant list `Empty, Value(T)` in `enum Slot<T> { Empty, Value(T) }`.
    /// Braces belong to the declaration; use the list's `variants()` to visit variants.
    pub fn variant_list(&self) -> Option<VariantList> {
        support::child(self.syntax())
    }

    /// Returns `<T>` in `enum Slot<T> { Empty, Value(T) }`; returns `None` in `enum Flag { On, Off }`.
    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }
}
