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
///
/// In `math::sqrt`, `math` and `sqrt` are separate names; each `token` refers
/// to its original lexical entry. The `::` token belongs to [`PathSegment`].
#[derive(Debug)]
pub struct Name {
    pub(crate) token: Token<tokens::NameToken>,
}

/// Most paths contain one name; only multi-segment paths allocate a segment list.
///
/// `value` is `Single(Name(value))`; `math::sqrt` is `Segmented` with
/// `first = math` and one `rest` entry for `::sqrt`. These are written names,
/// not resolved declaration identities.
#[derive(Debug)]
pub enum Path {
    Single(Name),
    Segmented(Box<PathSegments>),
}

node!(
    /// A multi-name path stored as a first name and separator/name continuations.
    ///
    /// For `math::nested::sqrt`, `first` is `Name(math)` and `rest` contains
    /// `PathSegment('::', nested)` and `PathSegment('::', sqrt)`. Each name has
    /// its own source site; separator tokens are not included in name spelling.
    PathSegments {
        first: Name,
        rest: Vec<PathSegment>,
    }
);

group!(
    /// A path continuation owning its separator and required next name.
    ///
    /// In `math::sqrt`, `separator` is the `::` token and `name` holds `sqrt`;
    /// [`PathSegments`] owns the initial `math` name.
    PathSegment {
        pub separator: Token<tokens::ColonColon>,
        pub name: Required<Name>,
    }
);

/// The written `val` or `var` category, without a semantic writeability decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Writeability {
    Val,
    Var,
}

/// A written binding token paired with its [`Writeability`] classification.
///
/// `val x` stores `Val`; `var x` stores `Var`. The token retains its own
/// physical-source location, while the enclosing declaration owns `x`.
#[derive(Debug)]
pub struct BindingKeyword {
    pub writeability: Writeability,
    pub(crate) token: Token<tokens::BindingKeywordToken>,
}

node!(
    /// Angle delimiters and comma-preserving declared type parameters.
    ///
    /// `<T, U: Read>` stores two [`GenericParam`] entries in `parameters`;
    /// `open`/`close` retain `<`/`>` tokens. These are binder declarations,
    /// whereas [`GenericArgList`] stores arguments at a use site.
    GenericParamList {
        open: Required<Token<tokens::Lt>>,
        parameters: SeparatedList<GenericParam>,
        close: Required<Token<tokens::Gt>>,
    }
);

node!(
    /// A declared type parameter with an optional bound clause.
    ///
    /// `T` has `name = Name(T)` and `bounds = None`; `T: Read + Write` has
    /// `bounds = Some(BoundClause)`. Present required names are abbreviated;
    /// a written colon with no bound retains the clause and a missing child.
    GenericParam {
        name: Required<Name>,
        bounds: Option<Box<BoundClause>>,
    }
);

group!(
    /// A colon-prefixed bound list attached to a generic parameter.
    ///
    /// In `T: Read + Write`, `colon` is a token and `bounds` holds a
    /// [`TraitBoundList`]; the enclosing [`GenericParam`] owns the name `T`.
    BoundClause {
        pub colon: Token<tokens::Colon>,
        pub bounds: Required<TraitBoundList>,
    }
);

node!(
    /// Angle-delimited positional and associated type arguments in source order.
    ///
    /// ```text
    /// <T, Item = i32>
    /// open = '<' token, arguments = [Type(Named(T)), Binding(AssociatedTypeBinding(Item = i32))]
    /// close = '>' token
    /// ```
    ///
    /// Present required delimiters and list entries are abbreviated. [`SeparatedList`]
    /// retains commas and missing slots; filtered accessors do not renumber stored entries.
    GenericArgList {
        open: Required<Token<tokens::Lt>>,
        arguments: SeparatedList<GenericArgument>,
        close: Required<Token<tokens::Gt>>,
    }
);

/// Positional and associated arguments retain their shared authored order.
///
/// In `<T, Item = i32>`, the first entry is `Type(Named(T))` and the second
/// is `Binding(AssociatedTypeBinding)`. [`GenericArgList`] owns delimiters
/// and comma positions rather than partitioning arguments by category.
#[derive(Debug)]
pub enum GenericArgument {
    Type(TypeSyntax),
    Binding(AssociatedTypeBinding),
}

node!(
    /// An associated name bound to a type inside a generic argument list.
    ///
    /// In `Reader<Item = i32>`, `name` holds `Item`, `equal` holds the `=` token,
    /// and `ty` holds `Named(i32)`. Each role is required even in damaged input.
    AssociatedTypeBinding {
        name: Required<Name>,
        equal: Required<Token<tokens::Eq>>,
        ty: Required<TypeSyntax>,
    }
);

node!(
    /// A where keyword followed by comma-preserving type-bound predicates.
    ///
    /// For `where T: Read, U: Write`, `keyword` holds `where` and `predicates`
    /// contains two [`WherePredicate`] nodes. The surrounding declaration owns
    /// its body delimiter or terminator, not this clause.
    WhereClause {
        keyword: Token<tokens::WhereKw>,
        predicates: SeparatedList<WherePredicate>,
    }
);

node!(
    /// A target type, colon token and required trait bounds.
    ///
    /// `T: Read + Write` stores `target = Named(T)` and a [`TraitBoundList`]
    /// in `bounds`; `colon` is a separate required token. Required wrappers are
    /// omitted from this sketch.
    WherePredicate {
        target: Required<TypeSyntax>,
        colon: Required<Token<tokens::Colon>>,
        bounds: Required<TraitBoundList>,
    }
);

node!(
    /// An initial trait reference followed by plus-prefixed bounds.
    ///
    /// `Read + Write` stores `first = Present(TraitRef(Read))` and one `rest`
    /// entry for `+ Write`. A trailing `+` retains a missing trait reference;
    /// this is not a comma-separated list.
    TraitBoundList {
        first: Required<TraitRef>,
        rest: Vec<TraitBound>,
    }
);

group!(
    /// A plus token and required trait reference continuing a bound list.
    ///
    /// In `Read + Write`, `plus` holds `+` and `trait_ref` holds `Write`;
    /// the enclosing [`TraitBoundList`] owns `Read`.
    TraitBound {
        pub(crate) plus: Token<tokens::Plus>,
        pub trait_ref: Required<TraitRef>,
    }
);

node!(
    /// A trait path with optional generic or callable-style arguments.
    ///
    /// ```text
    /// Read           path = Path(Read), application = None
    /// Read<T>        path = Path(Read), application = Some(Generic(<T>))
    /// Fn(i32) -> T   path = Path(Fn), application = Some(Callable((i32) -> T))
    /// ```
    ///
    /// Present required paths are abbreviated. Arguments remain syntax;
    /// resolution and trait eligibility belong to analysis.
    TraitRef {
        path: Required<Path>,
        application: Option<Box<TraitApplication>>,
    }
);

/// Generic or callable argument syntax attached to a trait reference.
///
/// `Read<T>` uses `Generic`; `Fn(i32) -> bool` uses `Callable`.
/// The enclosing [`TraitRef`] owns the path in either form.
#[derive(Debug)]
pub enum TraitApplication {
    Generic(GenericArgList),
    Callable(CallableTrait),
}

node!(
    /// A callable trait's parenthesized inputs and optional output clause.
    ///
    /// In `Fn(i32, String) -> bool`, `inputs` contains two type nodes,
    /// `open`/`close` retain parentheses and `output` is `Some(ReturnType)`.
    /// The enclosing [`TraitRef`] owns the `Fn` path.
    CallableTrait {
        open: Token<tokens::LParen>,
        inputs: SeparatedList<TypeSyntax>,
        close: Required<Token<tokens::RParen>>,
        output: Option<ReturnType>,
    }
);

group!(
    /// An arrow token and required result type.
    ///
    /// In `fn f() -> i32 { 1 }`, `arrow` holds `->` and `ty` holds `Named(i32)`.
    /// No result clause is represented by the owner's `None`; a written `->`
    /// without a type retains this clause with a missing required type.
    ReturnType {
        pub arrow: Token<tokens::Arrow>,
        pub ty: Required<TypeSyntax>,
    }
);

group!(
    /// A type annotation retaining separate required colon and type roles.
    ///
    /// In `val x: i32 = 1;`, `colon` holds `:` and `ty` holds `Named(i32)`.
    /// Owners with an optional annotation use `None` for no clause; `Some` with a
    /// missing type retains a written or recovered annotation rather than erasing it.
    TypeAnnotation {
        pub colon: Required<Token<tokens::Colon>>,
        pub ty: Required<TypeSyntax>,
    }
);

node!(
    /// Parentheses and comma-preserving function or method parameters.
    ///
    /// `(self, x: i32)` stores two [`Param`] entries, the first with
    /// [`ParameterType::Receiver`]. `open`/`close` retain the delimiter tokens;
    /// missing parameter slots stay in the underlying [`SeparatedList`].
    ParamList {
        open: Required<Token<tokens::LParen>>,
        parameters: SeparatedList<Param>,
        close: Required<Token<tokens::RParen>>,
    }
);

node!(
    /// A parameter name and its receiver or explicit type form.
    ///
    /// ```text
    /// self      name = Present(Name(self)), annotation = Receiver
    /// x: i32    name = Present(Name(x)), annotation = Explicit { colon: ':', ty: Named(i32) }
    /// ```
    ///
    /// The explicit form abbreviates required wrappers. A missing ordinary parameter
    /// type remains an explicit missing role, not an implicit receiver.
    Param {
        name: Required<Name>,
        annotation: ParameterType,
    }
);

/// The special receiver form or an explicit colon/type parameter form.
///
/// An admitted leading method `self` uses `Receiver`, without synthetic type
/// syntax. `x: i32` uses `Explicit`; a missing colon or type stays a missing
/// required slot in that form.
#[derive(Debug)]
pub enum ParameterType {
    Receiver,
    Explicit {
        colon: Required<Token<tokens::Colon>>,
        ty: Required<TypeSyntax>,
    },
}

node!(
    /// Brace tokens and comma-preserving struct field declarations.
    ///
    /// `{ val x: i32, var y: i32 }` stores two [`Field`] entries in `fields`;
    /// `open`/`close` hold the braces. Field attributes belong to each entry.
    FieldList {
        open: Required<Token<tokens::LBrace>>,
        fields: SeparatedList<Field>,
        close: Required<Token<tokens::RBrace>>,
    }
);

node!(
    /// A struct field declaration with attributes, visibility and binding kind.
    ///
    /// ```text
    /// pub val x: i32
    /// visibility = Public, binding = BindingKeyword(Val), name = Name(x)
    /// colon = ':' token, ty = Named(i32)
    /// ```
    ///
    /// Present required slots are abbreviated. The surrounding [`FieldList`] owns
    /// the comma; `binding` retains the written `val`/`var` choice.
    Field {
        attributes: Attributes,
        visibility: Visibility,
        binding: Required<BindingKeyword>,
        name: Required<Name>,
        colon: Required<Token<tokens::Colon>>,
        ty: Required<TypeSyntax>,
    }
);

node!(
    /// Brace tokens and comma-preserving enum variants.
    ///
    /// For `{ Empty, Value(T) }`, `variants` contains two [`Variant`] nodes;
    /// `open`/`close` hold braces and each entry retains its own comma.
    VariantList {
        open: Required<Token<tokens::LBrace>>,
        variants: SeparatedList<Variant>,
        close: Required<Token<tokens::RBrace>>,
    }
);

node!(
    /// An enum variant name, attributes and optional tuple payload.
    ///
    /// `Empty` has `payload = None`; `Value(T)` has `Some(TuplePayload)`.
    /// The payload owns parentheses and type entries; the surrounding [`VariantList`]
    /// owns the comma after the whole variant.
    Variant {
        attributes: Attributes,
        name: Required<Name>,
        payload: Option<TuplePayload>,
    }
);

node!(
    /// An enum variant's parenthesized payload types.
    ///
    /// In `Pair(i32, String)`, `types` contains two type entries and
    /// `open`/`close` retain parentheses. The enclosing [`Variant`] owns `Pair`.
    TuplePayload {
        open: Token<tokens::LParen>,
        types: SeparatedList<TypeSyntax>,
        close: Required<Token<tokens::RParen>>,
    }
);
