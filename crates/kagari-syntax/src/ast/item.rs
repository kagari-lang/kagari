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
//! is selected; missing braces retain a missing body, not an invented empty one.
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

node!(ModuleBody {
    open: Option<Token<tokens::LBrace>>,
    items: Vec<Item>,
    close: Option<Required<Token<tokens::RBrace>>>,
});

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

group!(VisibilityRestriction {
    pub open: Token<tokens::LParen>,
    pub scope: Required<Token<tokens::Lexeme>>,
    pub close: Required<Token<tokens::RParen>>,
});

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

node!(Attribute {
    hash: Token<tokens::Hash>,
    open: Required<Token<tokens::LBracket>>,
    path: Required<Path>,
    payload: Option<AttributePayload>,
    close: Required<Token<tokens::RBracket>>,
});

#[derive(Debug)]
pub enum AttributePayload {
    Value {
        equal: Token<tokens::Eq>,
        value: Required<AttributeValue>,
    },
    Arguments(AttributeArgs),
}

node!(AttributeArgs {
    open: Token<tokens::LParen>,
    arguments: SeparatedList<AttributeArg>,
    close: Required<Token<tokens::RParen>>,
});

node!(AttributeArg {
    key: Option<AttributeKey>,
    value: Required<AttributeValue>,
});

group!(AttributeKey {
    pub name: Name,
    pub equal: Token<tokens::Eq>,
});

#[derive(Debug)]
pub enum AttributeValue {
    Literal(Literal),
    Path(Path),
    List(AttributeList),
    Error(SyntaxSite),
}

node!(AttributeList {
    open: Token<tokens::LBracket>,
    elements: SeparatedList<AttributeArg>,
    close: Required<Token<tokens::RBracket>>,
});

node!(ModuleDef {
    attributes: Attributes,
    visibility: Visibility,
    keyword: Required<Token<tokens::ModKw>>,
    name: Required<Name>,
    form: ModuleForm,
});

#[derive(Debug)]
pub enum ModuleForm {
    External(Token<tokens::Semi>),
    Inline(InlineModule),
}

group!(InlineModule {
    pub(crate) body: Required<ModuleId>,
});

node!(UseDecl {
    attributes: Attributes,
    visibility: Visibility,
    keyword: Required<Token<tokens::UseKw>>,
    tree: Required<UseTree>,
    terminator: Required<Token<tokens::Semi>>,
});

node!(UseTree {
    prefix: Option<Path>,
    suffix: Option<UseSuffix>,
});

#[derive(Debug)]
pub enum UseSuffix {
    Alias {
        keyword: Token<tokens::AsKw>,
        name: Required<Name>,
    },
    Glob {
        separator: Token<tokens::ColonColon>,
        star: Required<Token<tokens::Lexeme>>,
    },
    Group {
        separator: Option<Token<tokens::ColonColon>>,
        group: UseTreeList,
    },
}

node!(UseTreeList {
    open: Token<tokens::LBrace>,
    trees: SeparatedList<UseTree>,
    close: Required<Token<tokens::RBrace>>,
});

node!(FnDef {
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
});

#[derive(Debug)]
pub enum FunctionBody {
    Block(Required<Box<BlockExpr>>),
    Declaration(Required<Token<tokens::Semi>>),
}

node!(ConstDef {
    attributes: Attributes,
    visibility: Visibility,
    keyword: Required<Token<tokens::ConstKw>>,
    name: Required<Name>,
    annotation: Option<Box<TypeAnnotation>>,
    initializer: ConstInitializer,
    terminator: Required<Token<tokens::Semi>>,
});

#[derive(Debug)]
pub enum ConstInitializer {
    Definition {
        equal: Required<Token<tokens::Eq>>,
        value: Required<Expr>,
    },
    Declaration,
}

node!(StaticDef {
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
});

node!(StructDef {
    attributes: Attributes,
    visibility: Visibility,
    keyword: Required<Token<tokens::StructKw>>,
    name: Required<Name>,
    generics: Option<Box<GenericParamList>>,
    fields: Required<FieldList>,
});

node!(EnumDef {
    attributes: Attributes,
    visibility: Visibility,
    keyword: Required<Token<tokens::EnumKw>>,
    name: Required<Name>,
    generics: Option<Box<GenericParamList>>,
    variants: Required<VariantList>,
});

node!(TraitDef {
    attributes: Attributes,
    visibility: Visibility,
    keyword: Required<Token<tokens::TraitKw>>,
    name: Required<Name>,
    generics: Option<Box<GenericParamList>>,
    supertraits: Option<Supertraits>,
    open: Required<Token<tokens::LBrace>>,
    members: Vec<Member>,
    close: Required<Token<tokens::RBrace>>,
});

group!(Supertraits {
    pub colon: Token<tokens::Colon>,
    pub bounds: Required<TraitBoundList>,
});

node!(ImplBlock {
    attributes: Attributes,
    keyword: Token<tokens::ImplKw>,
    generics: Option<Box<GenericParamList>>,
    trait_clause: Option<ImplTrait>,
    target: Required<TypeSyntax>,
    where_clause: Option<Box<WhereClause>>,
    open: Required<Token<tokens::LBrace>>,
    members: Vec<Member>,
    close: Required<Token<tokens::RBrace>>,
});

group!(ImplTrait {
    pub trait_ref: Required<TraitRef>,
    pub for_keyword: Required<Token<tokens::ForKw>>,
});

#[derive(Debug)]
pub enum Member {
    MethodDef(Box<MethodDef>),
    AssociatedType(Box<AssociatedType>),
    ConstDef(Box<ConstDef>),
    Error(SyntaxSite),
}

node!(MethodDef {
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
});

node!(TypeAlias {
    attributes: Attributes,
    visibility: Visibility,
    keyword: Token<tokens::TypeKw>,
    name: Required<Name>,
    generics: Option<Box<GenericParamList>>,
    bounds: Option<Supertraits>,
    target: AliasTarget,
    terminator: Required<Token<tokens::Semi>>,
});

/// Both accepted placements retain their authored order for formatting and recovery.
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

node!(OpaqueTypeDef {
    attributes: Attributes,
    visibility: Visibility,
    keyword: Token<tokens::TypeKw>,
    name: Required<Name>,
    generics: Option<Box<GenericParamList>>,
    bounds: Option<Supertraits>,
    where_clause: Option<Box<WhereClause>>,
    terminator: Required<Token<tokens::Semi>>,
});

node!(AssociatedType {
    attributes: Attributes,
    visibility: Visibility,
    keyword: Token<tokens::TypeKw>,
    name: Required<Name>,
    generics: Option<Box<GenericParamList>>,
    bounds: Option<Supertraits>,
    default: Option<AssociatedDefault>,
    where_clause: Option<Box<WhereClause>>,
    terminator: Required<Token<tokens::Semi>>,
});

group!(AssociatedDefault {
    pub equal: Token<tokens::Eq>,
    pub ty: Required<TypeSyntax>,
});
