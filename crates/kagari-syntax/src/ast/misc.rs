//! Names, ordered applications, signature fields and bounds as written.
//!
//! ```text
//! Box<T, Output = i32>   GenericArgList.arguments = [Type(T), Binding(Output=i32)]
//! T: Read + Write       GenericParam { name: T, bounds: Some(first=Read, rest=[Write]) }
//! value: [i32]          Param { name: value, annotation: Explicit(colon, Array(i32)) }
//! ```
//!
//! Names keep their own token sites. Applications preserve positional/associated
//! argument order and delimiters; they do not resolve names or infer types.

use crate::ast::{
    item::{Attributes, Visibility},
    site::{Required, SeparatedList},
    tokens::{self, Token},
    ty::TypeSyntax,
};

/// An identifier or permitted path keyword, retaining its own navigation site.
#[derive(Debug)]
pub struct Name {
    pub(crate) token: Token<tokens::NameToken>,
}

/// Most paths contain one name; only multi-segment paths allocate a segment list.
#[derive(Debug)]
pub enum Path {
    Single(Name),
    Segmented(Box<PathSegments>),
}

node!(PathSegments {
    first: Name,
    rest: Vec<PathSegment>,
});

group!(PathSegment {
    pub separator: Token<tokens::ColonColon>,
    pub name: Required<Name>,
});

/// The written `val` or `var` category, without a semantic writeability decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Writeability {
    Val,
    Var,
}

#[derive(Debug)]
pub struct BindingKeyword {
    pub writeability: Writeability,
    pub(crate) token: Token<tokens::Lexeme>,
}

node!(GenericParamList {
    open: Required<Token<tokens::Lt>>,
    parameters: SeparatedList<GenericParam>,
    close: Required<Token<tokens::Gt>>,
});

node!(GenericParam {
    name: Required<Name>,
    bounds: Option<Box<BoundClause>>,
});

group!(BoundClause {
    pub colon: Token<tokens::Colon>,
    pub bounds: Required<TraitBoundList>,
});

node!(GenericArgList {
    open: Required<Token<tokens::Lt>>,
    arguments: SeparatedList<GenericArgument>,
    close: Required<Token<tokens::Gt>>,
});

/// Positional and associated arguments retain their shared authored order.
#[derive(Debug)]
pub enum GenericArgument {
    Type(TypeSyntax),
    Binding(AssociatedTypeBinding),
}

node!(AssociatedTypeBinding {
    name: Required<Name>,
    equal: Required<Token<tokens::Eq>>,
    ty: Required<TypeSyntax>,
});

node!(WhereClause {
    keyword: Token<tokens::WhereKw>,
    predicates: SeparatedList<WherePredicate>,
});

node!(WherePredicate {
    target: Required<TypeSyntax>,
    colon: Required<Token<tokens::Colon>>,
    bounds: Required<TraitBoundList>,
});

node!(TraitBoundList {
    first: Required<TraitRef>,
    rest: Vec<TraitBound>,
});

group!(TraitBound {
    pub(crate) plus: Token<tokens::Lexeme>,
    pub trait_ref: Required<TraitRef>,
});

node!(TraitRef {
    path: Required<Path>,
    application: Option<Box<TraitApplication>>,
});

#[derive(Debug)]
pub enum TraitApplication {
    Generic(GenericArgList),
    Callable(CallableTrait),
}

node!(CallableTrait {
    open: Token<tokens::LParen>,
    inputs: SeparatedList<TypeSyntax>,
    close: Required<Token<tokens::RParen>>,
    output: Option<ReturnType>,
});

group!(ReturnType {
    pub arrow: Token<tokens::Arrow>,
    pub ty: Required<TypeSyntax>,
});

group!(TypeAnnotation {
    pub colon: Required<Token<tokens::Colon>>,
    pub ty: Required<TypeSyntax>,
});

node!(ParamList {
    open: Required<Token<tokens::LParen>>,
    parameters: SeparatedList<Param>,
    close: Required<Token<tokens::RParen>>,
});

node!(Param {
    name: Required<Name>,
    annotation: ParameterType,
});

#[derive(Debug)]
pub enum ParameterType {
    Receiver,
    Explicit {
        colon: Required<Token<tokens::Colon>>,
        ty: Required<TypeSyntax>,
    },
}

node!(FieldList {
    open: Required<Token<tokens::LBrace>>,
    fields: SeparatedList<Field>,
    close: Required<Token<tokens::RBrace>>,
});

node!(Field {
    attributes: Attributes,
    visibility: Visibility,
    binding: Required<BindingKeyword>,
    name: Required<Name>,
    colon: Required<Token<tokens::Colon>>,
    ty: Required<TypeSyntax>,
});

node!(VariantList {
    open: Required<Token<tokens::LBrace>>,
    variants: SeparatedList<Variant>,
    close: Required<Token<tokens::RBrace>>,
});

node!(Variant {
    attributes: Attributes,
    name: Required<Name>,
    payload: Option<TuplePayload>,
});

node!(TuplePayload {
    open: Token<tokens::LParen>,
    types: SeparatedList<TypeSyntax>,
    close: Required<Token<tokens::RParen>>,
});
