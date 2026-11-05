//! Declarations own attributes and punctuation; module bodies use parse-local IDs.
//!
//! ```text
//! #[tag] pub fn f<T>(x: T) -> T { x }
//! FnDef { attributes: [tag], visibility: Public, name: f, generics: [T],
//!         parameters: [Param(x, Explicit(T))], result: Some(T), body: Block({x}), ... }
//! mod a { fn f() {} }    ModuleDef.form = Inline(body = private module-body ID)
//! mod a;                ModuleDef.form = External(semicolon token)
//! ```
//!
//! The parser owns construction. Inline bodies are required once the inline form
//! is selected. A missing opening brace leaves a missing body; an opened body
//! without its closing brace remains present with a missing close token.
//! Attribute targets and declaration names are explicit fields.

use crate::ast::{
    expr::{BlockExpr, Expr, Literal},
    misc::{
        BindingKeyword, FieldList, GenericParamList, Name, ParamList, Path, ReturnType,
        TraitBoundList, TraitRef, TypeAnnotation, VariantList, WhereClause,
    },
    site::{Required, SeparatedList, SyntaxSite},
    tokens::{self, Token},
    ty::TypeSyntax,
};

/// A private index into this physical parse's module-body table, never an HIR ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModuleId(pub u32);

node!(
    /// Items and delimiters of one root or inline module.
    ///
    /// A physical root has no brace fields. For `mod child { fn f() {} }`, the
    /// inline body has `open = Some('{')`, one FnDef node in `items`, and
    /// `close = Some(Present('}'))`. An unclosed body instead has
    /// `close = Some(Missing { .. })`. Tokens are references into the physical file;
    /// the surrounding ModuleDef owns the `mod` keyword and name.
    ModuleBody {
        open: Option<Token<tokens::LBrace>>,
        items: Vec<Item>,
        close: Option<Required<Token<tokens::RBrace>>>,
    }
);

/// A module-level declaration alternative, with explicit damaged-source coverage.
///
/// `use math;` selects `UseDecl`; `fn f() {}` selects `FnDef`. Each boxed
/// node owns its header and children; `Error` retains a [`SyntaxSite`].
#[derive(Debug)]
pub enum Item {
    ModuleDef(Box<ModuleDef>),
    UseDecl(Box<UseDecl>),
    FnDef(Box<FnDef>),
    ConstDef(Box<ConstDef>),
    StaticDef(Box<StaticDef>),
    StructDef(Box<StructDef>),
    EnumDef(Box<EnumDef>),
    TraitDef(Box<TraitDef>),
    ImplBlock(Box<ImplBlock>),
    TypeAlias(Box<TypeAlias>),
    OpaqueTypeDef(Box<OpaqueTypeDef>),
    Error(SyntaxSite),
}

/// The written visibility prefix, without name-resolution decisions.
///
/// No prefix is `Private`; `pub` is `Public { restriction: None, .. }`.
/// `pub(super)` adds a [`VisibilityRestriction`] while retaining the `pub` token.
#[derive(Debug)]
pub enum Visibility {
    Private,
    Public {
        keyword: Token<tokens::PubKw>,
        restriction: Option<Box<VisibilityRestriction>>,
    },
}

/// Read-only classification of a written visibility prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibilityKind {
    Private,
    Public,
    PublicSuper,
    PublicCrate,
}

group!(
    /// The parenthesized scope after a `pub` token owned by [`Visibility`].
    ///
    /// `pub(crate)` stores `open = '('`, `scope = Present('crate')` and
    /// `close = Present(')')`, all tokens. A missing scope retains its required slot.
    VisibilityRestriction {
        pub open: Token<tokens::LParen>,
        pub scope: Required<Token<tokens::VisibilityScopeToken>>,
        pub close: Required<Token<tokens::RParen>>,
    }
);

/// Attached attributes in source order; the common empty case owns no allocation.
#[derive(Debug, Default)]
pub struct Attributes {
    items: Option<Box<AttributeSequence>>,
}

#[derive(Debug)]
struct AttributeSequence {
    values: Box<[Attribute]>,
}

impl Attributes {
    pub(crate) fn new(values: Vec<Attribute>) -> Self {
        Self {
            items: (!values.is_empty()).then(|| {
                Box::new(AttributeSequence {
                    values: values.into_boxed_slice(),
                })
            }),
        }
    }

    pub fn as_slice(&self) -> &[Attribute] {
        self.items.as_ref().map_or(&[], |sequence| &sequence.values)
    }
}

node!(
    /// An attached attribute, including its hash and bracket tokens.
    ///
    /// ```text
    /// #[lang = "add"]
    /// path = Present(Path(lang)), payload = Some(Value { equal: '=', value: Literal("add") })
    /// hash = '#', open = Present('['), close = Present(']')
    /// ```
    ///
    /// `#[tag]` has no payload; argument and nested list forms belong to
    /// [`AttributePayload`] and [`AttributeValue`].
    Attribute {
        hash: Token<tokens::Hash>,
        open: Required<Token<tokens::LBracket>>,
        path: Required<Path>,
        payload: Option<AttributePayload>,
        close: Required<Token<tokens::RBracket>>,
    }
);

/// The optional contents following an attribute path.
///
/// `#[tag = 1]` uses `Value` with an equals token and required value;
/// `#[tag(a, b)]` uses `Arguments` with a parenthesized [`AttributeArgs`] node.
#[derive(Debug)]
pub enum AttributePayload {
    Value {
        equal: Token<tokens::Eq>,
        value: Required<AttributeValue>,
    },
    Arguments(AttributeArgs),
}

node!(
    /// Parenthesized attribute arguments with comma-preserving entries.
    ///
    /// For `#[tag(level = 1, enabled)]`, `open`/`close` hold the parentheses and
    /// `arguments` contains two [`AttributeArg`] nodes in a [`SeparatedList`].
    AttributeArgs {
        open: Token<tokens::LParen>,
        arguments: SeparatedList<AttributeArg>,
        close: Required<Token<tokens::RParen>>,
    }
);

node!(
    /// One positional or named attribute argument.
    ///
    /// ```text
    /// enabled      key = None, value = Present(Path(enabled))
    /// level = 1    key = Some(AttributeKey(level, '=')), value = Present(Literal(1))
    /// ```
    ///
    /// A written `level =` retains the key and a missing required value.
    AttributeArg {
        key: Option<AttributeKey>,
        value: Required<AttributeValue>,
    }
);

group!(
    /// The name and equals token introducing a named attribute argument.
    ///
    /// In `level = 1`, `name` is the `level` name and `equal` is the `=` token;
    /// the enclosing [`AttributeArg`] owns the value.
    AttributeKey {
        pub name: Name,
        pub equal: Token<tokens::Eq>,
    }
);

/// An attribute literal, path, nested list or damaged-source site.
///
/// In `#[tag([1, name])]`, `List` owns entries whose values are respectively
/// `Literal` and `Path`; spelling and delimiters remain in their original source.
#[derive(Debug)]
pub enum AttributeValue {
    Literal(Literal),
    Path(Path),
    List(AttributeList),
    Error(SyntaxSite),
}

node!(
    /// A bracketed list used as an attribute value.
    ///
    /// For `#[tag([a, b])]`, `open`/`close` hold the bracket tokens and `elements`
    /// contains two [`AttributeArg`] nodes; commas remain in the [`SeparatedList`].
    AttributeList {
        open: Token<tokens::LBracket>,
        elements: SeparatedList<AttributeArg>,
        close: Required<Token<tokens::RBracket>>,
    }
);

node!(
    /// A module header and its external or inline form.
    ///
    /// ```text
    /// pub mod child;       visibility = Public, name = Present(Name(child)), form = External(';')
    /// mod child { ... }    visibility = Private, form = Inline(body = parse-local module ID)
    /// ```
    ///
    /// `keyword` holds the `mod` token. The inline [`ModuleBody`] owns its braces
    /// and items; it is accessed through the owning module view, not a transferable ID.
    ModuleDef {
        attributes: Attributes,
        visibility: Visibility,
        keyword: Required<Token<tokens::ModKw>>,
        name: Required<Name>,
        form: ModuleForm,
    }
);

/// The syntax distinguishing an external module from an inline body.
///
/// `mod child;` stores its semicolon in `External`; `mod child {}` stores a
/// parse-local body reference in `Inline`. A missing inline body remains required.
#[derive(Debug)]
pub enum ModuleForm {
    External(Token<tokens::Semi>),
    Inline(InlineModule),
}

group!(
    /// A required reference to an inline body in the owning physical parse.
    ///
    /// For `mod child {}`, `body` identifies the brace-owning [`ModuleBody`].
    /// A header with no opening brace retains `Required::Missing`; an opened
    /// but unclosed body remains present with its own missing closing token.
    InlineModule {
        pub(crate) body: Required<ModuleId>,
    }
);

node!(
    /// An import declaration with attributes, visibility and a semicolon.
    ///
    /// ```text
    /// pub use math::sqrt as root;
    /// UseDecl
    ///   attributes: empty
    ///   visibility: Public { keyword: 'pub' token, restriction: None }
    ///   keyword: Present('use' token)
    ///   tree: Present(UseTree node)
    ///     prefix: Some(Path for math::sqrt)
    ///     suffix: Some(Alias { keyword: 'as' token, name: Present(Name for root) })
    ///   terminator: Present(';' token)
    /// ```
    ///
    /// A missing semicolon is [`Required::Missing`], never a synthesized token.
    /// Import resolution belongs to analysis; this node retains the written tree.
    UseDecl {
        attributes: Attributes,
        visibility: Visibility,
        keyword: Required<Token<tokens::UseKw>>,
        tree: Required<UseTree>,
        terminator: Required<Token<tokens::Semi>>,
    }
);

node!(
    /// A recursive import path with an optional alias, glob or group suffix.
    ///
    /// ```text
    /// math::sqrt        prefix = Some(Path(math::sqrt)), suffix = None
    /// math::sqrt as f   prefix = Some(Path(math::sqrt)), suffix = Some(Alias(...))
    /// math::*          prefix = Some(Path(math)), suffix = Some(Glob(...))
    /// math::{sin, cos}  prefix = Some(Path(math)), suffix = Some(Group(...))
    /// {math, io}        prefix = None, suffix = Some(Group(separator = None, ...))
    /// ```
    ///
    /// Groups own nested [`UseTree`] nodes through [`UseTreeList`]. Optional fields
    /// also retain damaged input: a recovered empty tree is not a valid import.
    /// Aliases and globs require a prefix; prefixed groups require `::`. Recovery
    /// may retain other combinations, accompanied by parser diagnostics.
    UseTree {
        prefix: Option<Path>,
        suffix: Option<UseSuffix>,
    }
);

/// The part of an import tree following its optional path prefix.
///
/// `as local` owns an alias keyword/name; `::*` owns separator/star tokens.
/// `::{a, b}` owns a separator and [`UseTreeList`], while a root `{a, b}`
/// group has no separator. A written alias with no name retains a missing slot.
#[derive(Debug)]
pub enum UseSuffix {
    Alias {
        keyword: Token<tokens::AsKw>,
        name: Required<Name>,
    },
    Glob {
        separator: Token<tokens::ColonColon>,
        star: Required<Token<tokens::Star>>,
    },
    Group {
        separator: Option<Token<tokens::ColonColon>>,
        group: UseTreeList,
    },
}

node!(
    /// Braces and comma-separated child import trees.
    ///
    /// In `math::{sin, cos as c}`, the parent [`UseTree`] owns `math::`;
    /// this node owns the brace tokens and two `trees` entries. See [`SeparatedList`]
    /// for missing entries, retained commas and filtered accessor positions.
    UseTreeList {
        open: Token<tokens::LBrace>,
        trees: SeparatedList<UseTree>,
        close: Required<Token<tokens::RBrace>>,
    }
);

node!(
    /// A function declaration with its written signature and body form.
    ///
    /// ```text
    /// pub fn id<T>(x: T) -> T { x }
    /// visibility = Public, keyword = 'fn', name = Name(id), generics = Some(<T>)
    /// parameters = ParamList(x: T), result = Some(ReturnType('->', T))
    /// async_keyword = None, where_clause = None, body = Block(BlockExpr { tail: x, ... })
    /// ```
    ///
    /// The sketch abbreviates present required slots and child nodes. Attributes
    /// belong to this declaration; [`FunctionBody`] distinguishes blocks from signatures.
    FnDef {
        attributes: Attributes,
        visibility: Visibility,
        async_keyword: Option<Token<tokens::AsyncKw>>,
        keyword: Required<Token<tokens::FnKw>>,
        name: Required<Name>,
        generics: Option<Box<GenericParamList>>,
        parameters: ParamList,
        result: Option<ReturnType>,
        where_clause: Option<Box<WhereClause>>,
        body: FunctionBody,
    }
);

/// A block body or a semicolon-only function/method declaration.
///
/// `fn f() {}` stores a required [`BlockExpr`] in `Block`; an admitted
/// `fn f();` signature stores its semicolon in `Declaration`. A missing block
/// is distinct from a declaration; parser mode and context govern admission.
#[derive(Debug)]
pub enum FunctionBody {
    Block(Required<Box<BlockExpr>>),
    Declaration(Required<Token<tokens::Semi>>),
}

node!(
    /// A constant definition or an associated constant declaration.
    ///
    /// ```text
    /// const LIMIT: i32 = 8;
    /// name = Name(LIMIT), annotation = Some(TypeAnnotation(':', i32))
    /// initializer = Definition { equal: '=' token, value: Literal(8) }, terminator = ';' token
    /// ```
    ///
    /// Present required slots are abbreviated. An associated `const LIMIT: i32;`
    /// uses [`ConstInitializer::Declaration`] instead of a missing definition value.
    ConstDef {
        attributes: Attributes,
        visibility: Visibility,
        keyword: Required<Token<tokens::ConstKw>>,
        name: Required<Name>,
        annotation: Option<Box<TypeAnnotation>>,
        initializer: ConstInitializer,
        terminator: Required<Token<tokens::Semi>>,
    }
);

/// An equals/value definition or an associated declaration without one.
///
/// `const N = 1;` uses `Definition`; associated `const N: i32;` uses
/// `Declaration`. In `const N = ;`, the definition's value remains missing.
#[derive(Debug)]
pub enum ConstInitializer {
    Definition {
        equal: Required<Token<tokens::Eq>>,
        value: Required<Expr>,
    },
    Declaration,
}

node!(
    /// A module static with a required binding kind, type and initializer.
    ///
    /// ```text
    /// static var count: i32 = 0;
    /// keyword = 'static', binding = BindingKeyword(Var), name = Name(count)
    /// colon = ':', ty = Named(i32), equal = '=', initializer = Literal(0), terminator = ';'
    /// ```
    ///
    /// Present required slots are abbreviated; punctuation denotes tokens and type/value
    /// fields denote child syntax. Storage initialization semantics belong to analysis/runtime.
    StaticDef {
        attributes: Attributes,
        visibility: Visibility,
        keyword: Required<Token<tokens::StaticKw>>,
        binding: Required<BindingKeyword>,
        name: Required<Name>,
        colon: Required<Token<tokens::Colon>>,
        ty: Required<TypeSyntax>,
        equal: Required<Token<tokens::Eq>>,
        initializer: Required<Expr>,
        terminator: Required<Token<tokens::Semi>>,
    }
);

node!(
    /// A named struct with optional generics and a required field list.
    ///
    /// For `struct Point<T> { val x: T }`, `name` is `Point`, `generics` is
    /// `Some(GenericParamList(T))`, and `fields` contains a [`FieldList`] node.
    /// The list owns the braces; each field owns its own attributes and visibility.
    StructDef {
        attributes: Attributes,
        visibility: Visibility,
        keyword: Required<Token<tokens::StructKw>>,
        name: Required<Name>,
        generics: Option<Box<GenericParamList>>,
        fields: Required<FieldList>,
    }
);

node!(
    /// A named enum whose variant list owns braces and commas.
    ///
    /// For `enum Maybe<T> { Empty, Value(T) }`, `generics` stores `<T>` and
    /// `variants` holds a [`VariantList`] with two entries; `Value` owns a tuple payload.
    /// The `enum` keyword and name remain separate token/name fields.
    EnumDef {
        attributes: Attributes,
        visibility: Visibility,
        keyword: Required<Token<tokens::EnumKw>>,
        name: Required<Name>,
        generics: Option<Box<GenericParamList>>,
        variants: Required<VariantList>,
    }
);

node!(
    /// A trait declaration with optional bounds and ordered members.
    ///
    /// ```text
    /// trait Reader<T>: Base { type Item; fn read(self) -> T; }
    /// name = Name(Reader), generics = Some(<T>), supertraits = Some(':' + Base)
    /// members = [AssociatedType(Item), MethodDef(read)], open = '{', close = '}'
    /// ```
    ///
    /// The sketch abbreviates present required slots. Brace tokens belong to this node;
    /// member declarations own their signatures, terminators and attributes.
    TraitDef {
        attributes: Attributes,
        visibility: Visibility,
        keyword: Required<Token<tokens::TraitKw>>,
        name: Required<Name>,
        generics: Option<Box<GenericParamList>>,
        supertraits: Option<Supertraits>,
        open: Required<Token<tokens::LBrace>>,
        members: Vec<Member>,
        close: Required<Token<tokens::RBrace>>,
    }
);

group!(
    /// A colon-prefixed bound clause on a trait or type declaration.
    ///
    /// In `trait Reader: Read + Send {}`, `colon` is a token and `bounds` holds
    /// [`TraitBoundList`] with `first = Read` and one `rest` entry for `+ Send`.
    Supertraits {
        pub colon: Token<tokens::Colon>,
        pub bounds: Required<TraitBoundList>,
    }
);

node!(
    /// An inherent or trait implementation with an explicit target type.
    ///
    /// ```text
    /// impl Point {}                  trait_clause = None, target = Named(Point)
    /// impl<T> Read for Box<T> {}      generics = Some(<T>), trait_clause = Some(Read + 'for')
    ///                                target = Applied(Box<T>)
    /// ```
    ///
    /// Present required slots are abbreviated. `where_clause` retains optional bounds;
    /// `open`/`close` are brace tokens surrounding ordered [`Member`] nodes.
    ImplBlock {
        attributes: Attributes,
        keyword: Token<tokens::ImplKw>,
        generics: Option<Box<GenericParamList>>,
        trait_clause: Option<ImplTrait>,
        target: Required<TypeSyntax>,
        where_clause: Option<Box<WhereClause>>,
        open: Required<Token<tokens::LBrace>>,
        members: Vec<Member>,
        close: Required<Token<tokens::RBrace>>,
    }
);

group!(
    /// The trait reference and `for` token preceding an implementation target.
    ///
    /// In `impl Read for File {}`, `trait_ref` holds `Read` and `for_keyword`
    /// holds `for`; the enclosing [`ImplBlock`] owns the `File` target type.
    ImplTrait {
        pub trait_ref: Required<TraitRef>,
        pub for_keyword: Required<Token<tokens::ForKw>>,
    }
);

/// A trait or implementation member, distinct from a module-level [`Item`].
///
/// `fn read(self);`, `type Item;` and `const N: i32;` select `MethodDef`,
/// `AssociatedType` and `ConstDef` respectively; `Error` retains source coverage.
#[derive(Debug)]
pub enum Member {
    MethodDef(Box<MethodDef>),
    AssociatedType(Box<AssociatedType>),
    ConstDef(Box<ConstDef>),
    Error(SyntaxSite),
}

node!(
    /// A trait or implementation method with a receiver-aware parameter list.
    ///
    /// For `fn get(self, index: i32) -> T { value }`, `parameters` contains the
    /// receiver and explicit parameter, `result` stores `-> T`, and `body` is a block.
    /// [`ParamList`] owns parentheses and commas; [`FunctionBody`] also represents
    /// semicolon-only declarations. Attributes and visibility belong to this method.
    MethodDef {
        attributes: Attributes,
        visibility: Visibility,
        async_keyword: Option<Token<tokens::AsyncKw>>,
        keyword: Required<Token<tokens::FnKw>>,
        name: Required<Name>,
        generics: Option<Box<GenericParamList>>,
        parameters: ParamList,
        result: Option<ReturnType>,
        where_clause: Option<Box<WhereClause>>,
        body: FunctionBody,
    }
);

node!(
    /// A transparent type alias retaining the written placement of bounds.
    ///
    /// For `type Items<T> = Vec<T>;`, `name` is `Items`, `generics` stores `<T>`,
    /// and `target` is [`AliasTarget::TrailingWhere`] with `ty = Applied(Vec<T>)`.
    /// `keyword`/`terminator` retain `type`/`;`; [`AliasTarget`] owns `=` and any
    /// `where` clause. `bounds` stores an optional colon-prefixed bound clause.
    TypeAlias {
        attributes: Attributes,
        visibility: Visibility,
        keyword: Token<tokens::TypeKw>,
        name: Required<Name>,
        generics: Option<Box<GenericParamList>>,
        bounds: Option<Supertraits>,
        target: AliasTarget,
        terminator: Required<Token<tokens::Semi>>,
    }
);

/// Both accepted placements retain their authored order for formatting and recovery.
///
/// ```text
/// type A<T> where T: Read = T;  LeadingWhere { clause: where T: Read, equal: '=', ty: T }
/// type A<T> = T where T: Read;  TrailingWhere { equal: '=', ty: T, clause: Some(where T: Read) }
/// type A = i32;                TrailingWhere { equal: '=', ty: i32, clause: None }
/// ```
///
/// Present required slots are abbreviated. The enclosing [`TypeAlias`] owns
/// the declaration header and semicolon; the equals token and type stay here.
#[derive(Debug)]
pub enum AliasTarget {
    LeadingWhere {
        clause: Box<WhereClause>,
        equal: Required<Token<tokens::Eq>>,
        ty: Required<TypeSyntax>,
    },
    TrailingWhere {
        equal: Required<Token<tokens::Eq>>,
        ty: Required<TypeSyntax>,
        clause: Option<Box<WhereClause>>,
    },
}

node!(
    /// A declaration-mode type without an alias target.
    ///
    /// For `type Handle<T>: Resource;`, `name` is `Handle`, `generics` stores `<T>`,
    /// `bounds` stores `: Resource`, and `terminator` holds `;`. An optional
    /// `where_clause` precedes the terminator. No equals token or target type is invented.
    OpaqueTypeDef {
        attributes: Attributes,
        visibility: Visibility,
        keyword: Token<tokens::TypeKw>,
        name: Required<Name>,
        generics: Option<Box<GenericParamList>>,
        bounds: Option<Supertraits>,
        where_clause: Option<Box<WhereClause>>,
        terminator: Required<Token<tokens::Semi>>,
    }
);

node!(
    /// A trait or implementation associated type with an optional definition.
    ///
    /// ```text
    /// type Item;          name = Name(Item), default = None
    /// type Item = i32;    name = Name(Item), default = Some(AssociatedDefault('=', i32))
    /// ```
    ///
    /// `generics`, `bounds` and `where_clause` retain their written clauses.
    /// The `default` field also stores implementation definitions; semantic admissibility
    /// is checked later. A written `=` with no type retains a missing required child.
    AssociatedType {
        attributes: Attributes,
        visibility: Visibility,
        keyword: Token<tokens::TypeKw>,
        name: Required<Name>,
        generics: Option<Box<GenericParamList>>,
        bounds: Option<Supertraits>,
        default: Option<AssociatedDefault>,
        where_clause: Option<Box<WhereClause>>,
        terminator: Required<Token<tokens::Semi>>,
    }
);

group!(
    /// An equals token and required type in an associated type definition.
    ///
    /// In `type Item = i32;`, `equal` holds `=` and `ty` holds `Named(i32)`;
    /// the surrounding [`AssociatedType`] owns the semicolon.
    AssociatedDefault {
        pub equal: Token<tokens::Eq>,
        pub ty: Required<TypeSyntax>,
    }
);
