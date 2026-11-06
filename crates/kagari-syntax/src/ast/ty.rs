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

node!(
    /// A type path with an angle-delimited argument list.
    ///
    /// `Vec<T>` stores `path = Present(Path(Vec))` and `arguments = <T>`.
    /// [`GenericArgList`] owns angle tokens and comma-preserving type/binding entries;
    /// name resolution is not part of this node.
    AppliedType {
        path: Required<Path>,
        arguments: GenericArgList,
    }
);

node!(
    /// Parentheses around a single required type, preserving source grouping.
    ///
    /// `(T)` stores `ty = Present(Named(T))` with `open`/`close` parenthesis
    /// tokens. `(T,)` instead uses [`TupleType`], even with only one element.
    GroupedType {
        open: Token<tokens::LParen>,
        ty: Required<TypeSyntax>,
        close: Required<Token<tokens::RParen>>,
    }
);

node!(
    /// A parenthesized, comma-preserving list of tuple element types.
    ///
    /// `(T, U)` stores two `elements` entries; `(T,)` retains one and its trailing
    /// comma; `()` has none. `open`/`close` retain the parenthesis tokens.
    TupleType {
        open: Token<tokens::LParen>,
        elements: SeparatedList<TypeSyntax>,
        close: Required<Token<tokens::RParen>>,
    }
);

node!(
    /// A bracketed array element type.
    ///
    /// `[T]` stores `element = Present(Named(T))`, `open = '['` and
    /// `close = Present(']')`. Delimiters are tokens; no length expression is stored.
    ArrayType {
        open: Token<tokens::LBracket>,
        element: Required<TypeSyntax>,
        close: Required<Token<tokens::RBracket>>,
    }
);

node!(
    /// A function type with parameter types and a required result clause.
    ///
    /// ```text
    /// fn(i32, String) -> bool
    /// keyword = 'fn' token, open = '(' token, parameters = [Named(i32), Named(String)]
    /// close = ')' token, arrow = '->' token, result = Named(bool)
    /// ```
    ///
    /// Present required slots and list entries are abbreviated. Parameters contain
    /// types, not named [`Param`](crate::ast::misc::Param) nodes. Missing result
    /// syntax retains required slots rather than inventing a type.
    FunctionType {
        keyword: Token<tokens::FnKw>,
        open: Required<Token<tokens::LParen>>,
        parameters: SeparatedList<TypeSyntax>,
        close: Required<Token<tokens::RParen>>,
        arrow: Required<Token<tokens::Arrow>>,
        result: Required<TypeSyntax>,
    }
);

node!(
    /// A qualified receiver/trait path and associated member.
    ///
    /// ```text
    /// <T as Reader>::Item<U>
    /// open = '<', receiver = Named(T), as_keyword = 'as', trait_ref = TraitRef(Reader)
    /// close = '>', separator = '::', member = Name(Item), arguments = Some(<U>)
    /// ```
    ///
    /// Present required slots are abbreviated; quoted spellings are tokens and the
    /// other fields are child syntax. `arguments = None` when no member application
    /// is written. The same structure also appears in qualified path expressions.
    QualifiedType {
        open: Token<tokens::Lt>,
        receiver: Required<TypeSyntax>,
        as_keyword: Required<Token<tokens::AsKw>>,
        trait_ref: Required<TraitRef>,
        close: Required<Token<tokens::Gt>>,
        separator: Required<Token<tokens::ColonColon>>,
        member: Required<Name>,
        arguments: Option<Box<GenericArgList>>,
    }
);
