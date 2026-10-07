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
    /// Returns the generic arguments: the first direct `GenericArgList` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn generic_args(&self) -> Option<GenericArgList> {
        support::child(self.syntax())
    }

    /// Returns the receiver: the first direct `TypeRef` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn receiver(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }

    /// Returns the trait spelling: the first direct `TraitRef` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn trait_ref(&self) -> Option<TraitRef> {
        support::child(self.syntax())
    }

    /// Returns the member name: the first direct `Name` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn member(&self) -> Option<Name> {
        support::child(self.syntax())
    }
}

impl TypeRef {
    /// Returns the qualified form: the first direct `QualifiedType` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn qualified_type(&self) -> Option<QualifiedType> {
        support::child(self.syntax())
    }

    /// Returns the name: the first direct `Name` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns the normalized path spelling or `!`.
    /// Structural/grouped/qualified forms without a direct path/name yield `None`.
    pub fn name_text(&self) -> Option<String> {
        if support::token(self.syntax(), SyntaxKind::Bang).is_some() {
            return Some("!".into());
        }
        self.path()
            .and_then(|path| path.text())
            .or_else(|| self.name().and_then(|name| name.text()))
    }

    /// Returns the path: the first direct `Path` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn path(&self) -> Option<Path> {
        support::child(self.syntax())
    }

    /// Returns the generic arguments: the first direct `GenericArgList` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn generic_args(&self) -> Option<GenericArgList> {
        support::child(self.syntax())
    }

    /// Returns the tuple form: the first direct `TupleType` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn tuple_type(&self) -> Option<TupleType> {
        support::child(self.syntax())
    }

    /// Returns the grouped inner type: the first direct `TypeRef` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn grouped_type(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }

    /// Returns the array form: the first direct `ArrayType` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn array_type(&self) -> Option<ArrayType> {
        support::child(self.syntax())
    }

    /// Returns the function form: the first direct `FunctionType` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn function_type(&self) -> Option<FunctionType> {
        support::child(self.syntax())
    }
}

impl FunctionType {
    /// Iterates types inside direct `TypeList` nodes in source order.
    /// Excludes the direct result-type child.
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

    /// Returns the result: the last direct `TypeRef` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn result(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).last()
    }
}

impl TupleType {
    /// Iterates direct `TypeRef` children in source order, skipping other kinds.
    /// Returns an empty iterator when no matching children remain.
    pub fn element_types(&self) -> impl Iterator<Item = TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast)
    }
}

impl ArrayType {
    /// Returns the element type: the first direct `TypeRef` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn element_type(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).next()
    }
}
