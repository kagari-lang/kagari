//! Explicit type alternatives. Grouping remains syntax rather than an inferred type.
//!
//! ```text
//! (i32)         TypeSyntax::Grouped { ty: Named(i32), ... }
//! (i32,)        TypeSyntax::Tuple { elements: [Named(i32)], ... }
//! [T]           TypeSyntax::Array { element: Named(T), ... }
//! Vec<T>        TypeSyntax::Applied { path: Vec, arguments: [Type(T)], ... }
//! <T as R>::Out TypeSyntax::Qualified { receiver: T, trait_ref: R, member: Out, ... }
//! ```
//!
//! These alternatives describe source grouping, not semantic types. Delimiters
//! and required missing children remain distinct, and HIR matches `TypeView`.

use crate::ast::{
    misc::{GenericArgList, Name, Path, TraitRef},
    site::{Required, SeparatedList, SyntaxSite},
    tokens::{self, Token},
};

/// For `Outer<T>`, `Applied` owns the path and angle-delimited arguments.
/// `(T)` is `Grouped`; `(T,)` is `Tuple`. Resolution belongs to HIR.
#[derive(Debug)]
pub enum TypeSyntax {
    Named(Path),
    Applied(Box<AppliedType>),
    Never(Token<tokens::Bang>),
    Grouped(Box<GroupedType>),
    Tuple(Box<TupleType>),
    Array(Box<ArrayType>),
    Function(Box<FunctionType>),
    Qualified(Box<QualifiedType>),
    Error(SyntaxSite),
}

node!(AppliedType {
    path: Required<Path>,
    arguments: GenericArgList,
});

node!(GroupedType {
    open: Token<tokens::LParen>,
    ty: Required<TypeSyntax>,
    close: Required<Token<tokens::RParen>>,
});

node!(TupleType {
    open: Token<tokens::LParen>,
    elements: SeparatedList<TypeSyntax>,
    close: Required<Token<tokens::RParen>>,
});

node!(ArrayType {
    open: Token<tokens::LBracket>,
    element: Required<TypeSyntax>,
    close: Required<Token<tokens::RBracket>>,
});

node!(FunctionType {
    keyword: Token<tokens::FnKw>,
    open: Required<Token<tokens::LParen>>,
    parameters: SeparatedList<TypeSyntax>,
    close: Required<Token<tokens::RParen>>,
    arrow: Required<Token<tokens::Arrow>>,
    result: Required<TypeSyntax>,
});

node!(QualifiedType {
    open: Token<tokens::Lt>,
    receiver: Required<TypeSyntax>,
    as_keyword: Required<Token<tokens::AsKw>>,
    trait_ref: Required<TraitRef>,
    close: Required<Token<tokens::Gt>>,
    separator: Required<Token<tokens::ColonColon>>,
    member: Required<Name>,
    arguments: Option<Box<GenericArgList>>,
});
