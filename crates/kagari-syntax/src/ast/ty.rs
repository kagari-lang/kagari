//! Type spellings, grouping and qualified member views.
//!
//! [`TypeRef`] is the outer wrapper for all type forms; its optional accessors select
//! the form actually written. These nodes are not semantic types. See [`crate::ast`]
//! for tree notation and recovery conventions.

use crate::{
    ast::{
        misc::{GenericArgList, Name, Path, TraitRef},
        support,
        traits::AstNode,
    },
    kind::SyntaxKind,
};

ast_node!(
    /// A type spelling wrapper, not a resolved semantic type.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `Box<i32>`:
    ///
    /// ```text
    /// TypeRef
    /// +-- Path (Box) [node] -> path()
    /// `-- GenericArgList (<i32>) [node] -> generic_args()
    /// ```
    ///
    /// `(i32)`:
    ///
    /// ```text
    /// TypeRef
    /// +-- LParen "(" [token]
    /// +-- TypeRef (i32) [node] -> grouped_type()
    /// `-- RParen ")" [token]
    /// ```
    ///
    /// `!`:
    ///
    /// ```text
    /// TypeRef
    /// `-- Bang "!" [token]
    /// ```
    ///
    /// `(i32,)`:
    ///
    /// ```text
    /// TypeRef
    /// `-- TupleType ((i32,)) [node] -> tuple_type()
    /// ```
    ///
    /// `()`:
    ///
    /// ```text
    /// TypeRef
    /// `-- TupleType (()) [node] -> tuple_type()
    /// ```
    ///
    /// `[i32]`:
    ///
    /// ```text
    /// TypeRef
    /// `-- ArrayType ([i32]) [node] -> array_type()
    /// ```
    ///
    /// `fn(i32) -> i32`:
    ///
    /// ```text
    /// TypeRef
    /// `-- FunctionType (fn(i32) -> i32) [node] -> function_type()
    /// ```
    ///
    /// `<T as Read>::Output`:
    ///
    /// ```text
    /// TypeRef
    /// `-- QualifiedType (<T as Read>::Output) [node] -> qualified_type()
    /// ```
    ///
    /// The diagrams show alternatives, not simultaneous fields. Grouped `(T)` contains
    /// a direct `TypeRef` and parentheses; tuples contain a `TupleType` node.
    /// `name_text()` handles paths and `!`, not structural or qualified forms. Type
    /// arguments and qualified members remain unresolved syntax.
    TypeRef,
    TypeRef
);

ast_node!(
    /// A tuple type, distinguished from grouping by a comma or empty parentheses.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `(i32, bool)`:
    ///
    /// ```text
    /// TupleType
    /// +-- LParen "(" [token]
    /// +-- TypeRef (i32) [node] -> element_types()
    /// +-- Comma "," [token]
    /// +-- TypeRef (bool) [node] -> element_types()
    /// `-- RParen ")" [token]
    /// ```
    ///
    /// Commas distinguish `(T,)` from grouped `(T)`; `()` has no element types.
    TupleType,
    TupleType
);

ast_node!(
    /// An array type spelling with one element-type child.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `[i32]`:
    ///
    /// ```text
    /// ArrayType
    /// +-- LBracket "[" [token]
    /// +-- TypeRef (i32) [node] -> element_type()
    /// `-- RBracket "]" [token]
    /// ```
    ArrayType,
    ArrayType
);

ast_node!(
    /// A `fn(...) -> ...` type with separate input list and result.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `fn(i32, bool) -> i32`:
    ///
    /// ```text
    /// FunctionType
    /// +-- FnKw "fn" [token]
    /// +-- LParen "(" [token]
    /// +-- TypeList (i32, bool) [node] -> params() via list
    /// +-- RParen ")" [token]
    /// +-- Arrow "->" [token]
    /// `-- TypeRef (i32) [node] -> result()
    /// ```
    ///
    /// Parameter types are children of `TypeList`; `result()` reads the last direct
    /// `TypeRef`, so it cannot accidentally select a parameter type.
    FunctionType,
    FunctionType
);

ast_node!(
    /// A `<Receiver as Trait>::Member` spelling with optional member arguments.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `<T as Read>::Output<i32>`:
    ///
    /// ```text
    /// QualifiedType
    /// +-- Lt "<" [token]
    /// +-- TypeRef (T) [node] -> receiver()
    /// +-- AsKw "as" [token]
    /// +-- TraitRef (Read) [node] -> trait_ref()
    /// +-- Gt ">" [token]
    /// +-- ColonColon "::" [token]
    /// +-- Name (Output) [node] -> member()
    /// `-- GenericArgList (<i32>) [node] -> generic_args()
    /// ```
    ///
    /// The receiver is a direct `TypeRef`; the trait's arguments are nested under
    /// `TraitRef`. `generic_args()` selects arguments of the member after `::`.
    QualifiedType,
    QualifiedType
);

impl QualifiedType {
    /// Returns the member arguments `<i32>` in `<T as Read<bool>>::Output<i32>`.
    /// The trait arguments `<bool>` belong to `trait_ref()`; no member arguments yields `None`.
    pub fn generic_args(&self) -> Option<GenericArgList> {
        support::child(self.syntax())
    }

    /// Returns `T` in `<T as Read>::Output`.
    pub fn receiver(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }

    /// Returns `Read<i32>` in `<T as Read<i32>>::Output`.
    pub fn trait_ref(&self) -> Option<TraitRef> {
        support::child(self.syntax())
    }

    /// Returns `Output` in `<T as Read>::Output<i32>`, excluding its type arguments.
    pub fn member(&self) -> Option<Name> {
        support::child(self.syntax())
    }
}

impl TypeRef {
    /// Returns the qualified form `<T as Read>::Output`; returns `None` for plain `T`.
    pub fn qualified_type(&self) -> Option<QualifiedType> {
        support::child(self.syntax())
    }

    /// Returns a direct name child if present.
    /// For parsed `Box<i32>` or `T`, the name is nested in `path()`, so this returns `None`.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns `Some("Box")` for `Box<i32>`, `Some("math::Value")` for `math :: Value`, and `Some("!")` for `!`.
    /// Returns `None` for `(T)`, `[T]`, tuple, function and qualified forms; does not resolve types.
    pub fn name_text(&self) -> Option<String> {
        if support::token(self.syntax(), SyntaxKind::Bang).is_some() {
            return Some("!".into());
        }
        self.path()
            .and_then(|path| path.text())
            .or_else(|| self.name().and_then(|name| name.text()))
    }

    /// Returns `Box` in `Box<i32>`, or `math::Value` in `math::Value`.
    /// Returns `None` for structural forms such as `[i32]` and `(i32, bool)`.
    pub fn path(&self) -> Option<Path> {
        support::child(self.syntax())
    }

    /// Returns `<i32>` in `Box<i32>`; returns `None` in `Box`.
    /// For `<T as Read>::Output<i32>`, call `generic_args()` on the node returned by `qualified_type()`.
    pub fn generic_args(&self) -> Option<GenericArgList> {
        support::child(self.syntax())
    }

    /// Returns the tuple form for `(i32, bool)`, `(i32,)`, or `()`.
    /// Returns `None` for grouped `(i32)`; use `grouped_type()` for that form.
    pub fn tuple_type(&self) -> Option<TupleType> {
        support::child(self.syntax())
    }

    /// Returns the inner `i32` in `(i32)`.
    /// Returns `None` for `(i32,)` and `()`; those use `tuple_type()`.
    pub fn grouped_type(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }

    /// Returns the array form `[i32]`; returns `None` for nominal `Vec<i32>`.
    pub fn array_type(&self) -> Option<ArrayType> {
        support::child(self.syntax())
    }

    /// Returns the function form `fn(i32) -> bool`; returns `None` for a plain type path.
    pub fn function_type(&self) -> Option<FunctionType> {
        support::child(self.syntax())
    }
}

impl FunctionType {
    /// Iterates `i32` then `bool` in `fn(i32, bool) -> String`; `fn() -> String` yields no inputs.
    /// Reads the nested type list, excluding the result `String`.
    pub fn params(&self) -> impl Iterator<Item = TypeRef> {
        self.syntax()
            .children()
            .filter(|node| node.kind() == SyntaxKind::TypeList)
            .flat_map(|list| {
                list.children()
                    .filter_map(TypeRef::cast)
                    .collect::<Vec<_>>()
            })
    }

    /// Returns `String` in `fn(i32, bool) -> String`.
    /// Selects the last direct type child; parameter types are nested in a separate list.
    pub fn result(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).last()
    }
}

impl TupleType {
    /// Iterates `i32` then `bool` in `(i32, bool)`; `(i32,)` yields one type and `()` none.
    pub fn element_types(&self) -> impl Iterator<Item = TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast)
    }
}

impl ArrayType {
    /// Returns `i32` in `[i32]`, or the whole `[i32]` type in `[[i32]]`.
    pub fn element_type(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).next()
    }
}
