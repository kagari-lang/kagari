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
    /// A `type` declaration or definition in a trait, impl or declaration-mode file.
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
    /// Reads direct visibility tokens; defaults to private when `pub` is absent.
    /// A direct `super` token with `pub` produces `PublicSuper`.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns the generic parameters: the first direct `GenericParamList` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }

    /// Returns the where clause: the first direct `WhereClause` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn where_clause(&self) -> Option<WhereClause> {
        support::child(self.syntax())
    }

    /// Returns the name: the first direct `Name` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Copies the selected name's spelling, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns the type annotation or assigned type: the first direct `TypeRef` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn ty(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }

    /// Returns the trait bounds: the first direct `TraitBoundList` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
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
}

pub(crate) fn visibility_of(syntax: &SyntaxNode) -> Visibility {
    if support::token(syntax, SyntaxKind::PubKw).is_none() {
        Visibility::Private
    } else if support::token(syntax, SyntaxKind::SuperKw).is_some() {
        Visibility::PublicSuper
    } else {
        Visibility::Public
    }
}

/// A typed choice of declarations found directly in a file or module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// The [`AssociatedType`] view; see its node diagram.
    TypeDecl(AssociatedType),
    /// The [`ModuleDef`] view; see its node diagram.
    ModuleDef(ModuleDef),
    /// The [`UseDecl`] view; see its node diagram.
    UseDecl(UseDecl),
    /// The [`TraitDef`] view; see its node diagram.
    TraitDef(TraitDef),
    /// The [`ImplBlock`] view; see its node diagram.
    ImplBlock(ImplBlock),
    /// The [`FnDef`] view; see its node diagram.
    FnDef(FnDef),
    /// The [`ConstDef`] view; see its node diagram.
    ConstDef(ConstDef),
    /// The [`StructDef`] view; see its node diagram.
    StructDef(StructDef),
    /// The [`EnumDef`] view; see its node diagram.
    EnumDef(EnumDef),
}

impl AstNode for Item {
    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::AssociatedType
                | SyntaxKind::ModuleDef
                | SyntaxKind::UseDecl
                | SyntaxKind::TraitDef
                | SyntaxKind::ImplBlock
                | SyntaxKind::FnDef
                | SyntaxKind::ConstDef
                | SyntaxKind::StructDef
                | SyntaxKind::EnumDef
        )
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        match syntax.kind() {
            SyntaxKind::AssociatedType => AssociatedType::cast(syntax).map(Self::TypeDecl),
            SyntaxKind::ModuleDef => ModuleDef::cast(syntax).map(Self::ModuleDef),
            SyntaxKind::UseDecl => UseDecl::cast(syntax).map(Self::UseDecl),
            SyntaxKind::TraitDef => TraitDef::cast(syntax).map(Self::TraitDef),
            SyntaxKind::ImplBlock => ImplBlock::cast(syntax).map(Self::ImplBlock),
            SyntaxKind::FnDef => FnDef::cast(syntax).map(Self::FnDef),
            SyntaxKind::ConstDef => ConstDef::cast(syntax).map(Self::ConstDef),
            SyntaxKind::StructDef => StructDef::cast(syntax).map(Self::StructDef),
            SyntaxKind::EnumDef => EnumDef::cast(syntax).map(Self::EnumDef),
            _ => None,
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::TypeDecl(node) => node.syntax(),
            Self::ModuleDef(node) => node.syntax(),
            Self::UseDecl(node) => node.syntax(),
            Self::TraitDef(node) => node.syntax(),
            Self::ImplBlock(node) => node.syntax(),
            Self::FnDef(node) => node.syntax(),
            Self::ConstDef(node) => node.syntax(),
            Self::StructDef(node) => node.syntax(),
            Self::EnumDef(node) => node.syntax(),
        }
    }
}

impl Item {
    /// Iterates direct `Attribute` children in source order, skipping other kinds.
    /// Returns an empty iterator when no matching children remain.
    pub fn attributes(&self) -> impl Iterator<Item = Attribute> {
        self.syntax().children().filter_map(Attribute::cast)
    }
}

impl Attribute {
    /// Returns the path: the first direct `Path` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn path(&self) -> Option<Path> {
        support::child(self.syntax())
    }

    /// Copies the selected name's spelling, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.path().and_then(|path| path.text())
    }

    /// Returns the arguments: the first direct `AttributeArgs` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn args(&self) -> Option<AttributeArgs> {
        support::child(self.syntax())
    }

    /// Returns the value expression: the first direct `AttributeValue` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn value(&self) -> Option<AttributeValue> {
        support::child(self.syntax())
    }
}

impl AttributeArgs {
    /// Iterates arguments through direct argument-list children in source order.
    /// Empty `()` has no list and yields no arguments.
    pub fn arguments(&self) -> impl Iterator<Item = AttributeArg> {
        self.syntax()
            .children()
            .filter_map(AttributeArgList::cast)
            .flat_map(|list| list.syntax().children().filter_map(AttributeArg::cast))
    }
}

impl AttributeArg {
    /// Copies the selected name's spelling, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        support::child::<Name>(self.syntax()).and_then(|name| name.text())
    }

    /// Returns the value expression: the first direct `AttributeValue` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn value(&self) -> Option<AttributeValue> {
        support::child(self.syntax())
    }
}

impl AttributeValue {
    /// Returns the literal: the first direct `Literal` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn literal(&self) -> Option<Literal> {
        support::child(self.syntax())
    }

    /// Returns the path: the first direct `Path` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn path(&self) -> Option<Path> {
        support::child(self.syntax())
    }

    /// Iterates array elements through direct argument-list children.
    /// Scalar literal/path values yield an empty iterator.
    pub fn elements(&self) -> impl Iterator<Item = AttributeArg> {
        self.syntax()
            .children()
            .filter_map(AttributeArgList::cast)
            .flat_map(|list| list.syntax().children().filter_map(AttributeArg::cast))
    }
}

impl ModuleDef {
    /// Reads direct visibility tokens; defaults to private when `pub` is absent.
    /// A direct `super` token with `pub` produces `PublicSuper`.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns the name: the first direct `Name` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Copies the selected name's spelling, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns the inline module body: the first direct `ModuleBlock` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn block(&self) -> Option<ModuleBlock> {
        support::child(self.syntax())
    }
}

impl ModuleBlock {
    /// Iterates direct `Item` children in source order, skipping other kinds.
    /// Returns an empty iterator when no matching children remain.
    pub fn items(&self) -> impl Iterator<Item = Item> {
        support::children(self.syntax())
    }
}

impl UseDecl {
    /// Reads direct visibility tokens; defaults to private when `pub` is absent.
    /// A direct `super` token with `pub` produces `PublicSuper`.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns the use tree: the first direct `UseTree` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn tree(&self) -> Option<UseTree> {
        support::child(self.syntax())
    }
}

impl UseTree {
    /// Whether this branch contains a direct `*`; does not inspect nested branches.
    pub fn is_glob(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::Star).is_some()
    }

    /// Returns the path: the first direct `Path` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn path(&self) -> Option<Path> {
        support::child(self.syntax())
    }

    /// Returns the first direct `Name` after an `as` token.
    /// Unaliased paths and missing recovered alias names yield `None`.
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

    /// Iterates trees inside direct `UseTreeList` children in source order.
    /// Flattens one list level only; leaf and glob branches yield no trees.
    pub fn nested_trees(&self) -> impl Iterator<Item = UseTree> {
        self.syntax()
            .children()
            .filter_map(UseTreeList::cast)
            .flat_map(|list| list.trees().collect::<Vec<_>>())
    }
}

impl UseTreeList {
    /// Iterates direct `UseTree` children in source order, skipping other kinds.
    /// Returns an empty iterator when no matching children remain.
    pub fn trees(&self) -> impl Iterator<Item = UseTree> {
        support::children(self.syntax())
    }
}

impl TraitDef {
    /// Iterates direct `ConstDef` children in source order, skipping other kinds.
    /// Returns an empty iterator when no matching children remain.
    pub fn associated_consts(&self) -> impl Iterator<Item = ConstDef> {
        support::children(self.syntax())
    }

    /// Returns the supertrait bounds: the first direct `TraitBoundList` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn supertraits(&self) -> Option<TraitBoundList> {
        support::child(self.syntax())
    }

    /// Iterates direct `AssociatedType` children in source order, skipping other kinds.
    /// Returns an empty iterator when no matching children remain.
    pub fn associated_types(&self) -> impl Iterator<Item = AssociatedType> {
        support::children(self.syntax())
    }

    /// Reads direct visibility tokens; defaults to private when `pub` is absent.
    /// A direct `super` token with `pub` produces `PublicSuper`.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns the name: the first direct `Name` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Copies the selected name's spelling, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns the generic parameters: the first direct `GenericParamList` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }

    /// Iterates direct `MethodDef` children in source order, skipping other kinds.
    /// Returns an empty iterator when no matching children remain.
    pub fn methods(&self) -> impl Iterator<Item = MethodDef> {
        support::children(self.syntax())
    }
}

impl ImplBlock {
    /// Iterates direct `ConstDef` children in source order, skipping other kinds.
    /// Returns an empty iterator when no matching children remain.
    pub fn associated_consts(&self) -> impl Iterator<Item = ConstDef> {
        support::children(self.syntax())
    }

    /// Iterates direct `AssociatedType` children in source order, skipping other kinds.
    /// Returns an empty iterator when no matching children remain.
    pub fn associated_types(&self) -> impl Iterator<Item = AssociatedType> {
        support::children(self.syntax())
    }

    /// Returns the generic parameters: the first direct `GenericParamList` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }

    /// Returns the trait spelling: the first direct `TraitRef` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn trait_ref(&self) -> Option<TraitRef> {
        support::child(self.syntax())
    }

    /// Returns the target type: the first direct `TypeRef` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn target_type(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).next()
    }

    /// Returns the where clause: the first direct `WhereClause` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn where_clause(&self) -> Option<WhereClause> {
        support::child(self.syntax())
    }

    /// Iterates direct `MethodDef` children in source order, skipping other kinds.
    /// Returns an empty iterator when no matching children remain.
    pub fn methods(&self) -> impl Iterator<Item = MethodDef> {
        support::children(self.syntax())
    }
}

impl MethodDef {
    /// Reads direct visibility tokens; defaults to private when `pub` is absent.
    /// A direct `super` token with `pub` produces `PublicSuper`.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns the name: the first direct `Name` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Copies the selected name's spelling, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns the generic parameters: the first direct `GenericParamList` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }

    /// Returns the parameter list: the first direct `ParamList` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn param_list(&self) -> Option<ParamList> {
        support::child(self.syntax())
    }

    /// Returns the declared return type: the first direct `TypeRef` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn return_type(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).next()
    }

    /// Returns the where clause: the first direct `WhereClause` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn where_clause(&self) -> Option<WhereClause> {
        support::child(self.syntax())
    }

    /// Returns the body: the first direct `BlockExpr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn body(&self) -> Option<BlockExpr> {
        support::child(self.syntax())
    }
}

impl SourceFile {
    /// Inner documentation belongs to the module's leading parsed trivia.
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

    /// Iterates direct `Item` children in source order, skipping other kinds.
    /// Returns an empty iterator when no matching children remain.
    pub fn items(&self) -> impl Iterator<Item = Item> {
        support::children(self.syntax())
    }
}

impl FnDef {
    /// Reads direct visibility tokens; defaults to private when `pub` is absent.
    /// A direct `super` token with `pub` produces `PublicSuper`.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns the name: the first direct `Name` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Copies the selected name's spelling, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns the parameter list: the first direct `ParamList` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn param_list(&self) -> Option<ParamList> {
        support::child(self.syntax())
    }

    /// Returns the generic parameters: the first direct `GenericParamList` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }

    /// Returns the declared return type: the first direct `TypeRef` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn return_type(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).next()
    }

    /// Returns the body: the first direct `BlockExpr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn body(&self) -> Option<BlockExpr> {
        support::child(self.syntax())
    }

    /// Returns the where clause: the first direct `WhereClause` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn where_clause(&self) -> Option<WhereClause> {
        support::child(self.syntax())
    }
}

impl ConstDef {
    /// Reads direct visibility tokens; defaults to private when `pub` is absent.
    /// A direct `super` token with `pub` produces `PublicSuper`.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns the name: the first direct `Name` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Copies the selected name's spelling, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns the type annotation or assigned type: the first direct `TypeRef` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn ty(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).next()
    }

    /// Returns the initializer: the first direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn initializer(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }
}

impl StructDef {
    /// Reads direct visibility tokens; defaults to private when `pub` is absent.
    /// A direct `super` token with `pub` produces `PublicSuper`.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns the name: the first direct `Name` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Copies the selected name's spelling, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns the field list: the first direct `FieldList` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn field_list(&self) -> Option<FieldList> {
        support::child(self.syntax())
    }

    /// Returns the generic parameters: the first direct `GenericParamList` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }
}

impl EnumDef {
    /// Reads direct visibility tokens; defaults to private when `pub` is absent.
    /// A direct `super` token with `pub` produces `PublicSuper`.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns the name: the first direct `Name` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Copies the selected name's spelling, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns the variant list: the first direct `VariantList` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn variant_list(&self) -> Option<VariantList> {
        support::child(self.syntax())
    }

    /// Returns the generic parameters: the first direct `GenericParamList` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }
}
