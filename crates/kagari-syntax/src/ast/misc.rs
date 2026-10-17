//! Names, paths, parameters, fields and other shared syntax components.
//!
//! These helpers retain spelling and child grouping; they do not resolve names or
//! compute types. Node diagrams follow the conventions in [`crate::ast`].

use crate::{
    ast::{
        item::{Visibility, visibility_of},
        support,
        traits::AstNode,
        ty::TypeRef,
    },
    kind::SyntaxKind,
};

use rowan::NodeOrToken;

/// The binding/field keyword as written, without semantic mutability analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Writeability {
    /// The `val` keyword.
    Val,
    /// The `var` keyword.
    Var,
}

ast_node!(
    /// One identifier or special path segment, without name resolution.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `total`:
    ///
    /// ```text
    /// Name
    /// `-- Ident "total" [token] -> text()
    /// ```
    ///
    /// `text()` selects a direct identifier or `crate`/`self`/`super` token, returning
    /// owned text. An empty recovered `Name` has no text.
    Name,
    Name
);

ast_node!(
    /// A sequence of names separated by `::`, without resolution or generic arguments.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `math::total`:
    ///
    /// ```text
    /// Path
    /// +-- Name (math) [node] -> segments()
    /// +-- ColonColon "::" [token]
    /// `-- Name (total) [node] -> segments()
    /// ```
    ///
    /// `segments()` returns direct `Name` children. `text()` joins readable segments
    /// with `::`, omitting trivia; this normalized spelling is not an exact source slice.
    Path,
    Path
);

ast_node!(
    /// Angle-bracketed generic parameter declarations.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `<T: Base, U>`:
    ///
    /// ```text
    /// GenericParamList
    /// +-- Lt "<" [token]
    /// +-- GenericParam (T: Base) [node] -> params()
    /// +-- Comma "," [token]
    /// +-- GenericParam (U) [node] -> params()
    /// `-- Gt ">" [token]
    /// ```
    GenericParamList,
    GenericParamList
);

ast_node!(
    /// One generic parameter name with optional trait bounds.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `T: Base`:
    ///
    /// ```text
    /// GenericParam
    /// +-- Name (T) [node] -> name()
    /// +-- Colon ":" [token]
    /// `-- TraitBoundList (Base) [node] -> bounds()
    /// ```
    GenericParam,
    GenericParam
);

ast_node!(
    /// Angle-bracketed type arguments and associated-type bindings.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `<i32, Output = bool>`:
    ///
    /// ```text
    /// GenericArgList
    /// +-- Lt "<" [token]
    /// +-- TypeRef (i32) [node] -> args()
    /// +-- Comma "," [token]
    /// +-- AssociatedTypeBinding (Output = bool) [node] -> bindings()
    /// `-- Gt ">" [token]
    /// ```
    ///
    /// `args()` yields only positional `TypeRef` children; `bindings()` yields only
    /// associated-type equalities. `positional_after_binding()` reports their source
    /// ordering without performing type checking.
    GenericArgList,
    GenericArgList
);

ast_node!(
    /// An associated-type equality inside generic arguments.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `Output = bool`:
    ///
    /// ```text
    /// AssociatedTypeBinding
    /// +-- Name (Output) [node]
    /// +-- Eq "=" [token]
    /// `-- TypeRef (bool) [node] -> ty()
    /// ```
    AssociatedTypeBinding,
    AssociatedTypeBinding
);

impl AssociatedTypeBinding {
    /// Returns `Some("Output")` for `Output = bool`, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        support::child::<Name>(self.syntax()).and_then(|name| name.text())
    }

    /// Returns `bool` in the generic argument binding `Output = bool`.
    pub fn ty(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }
}

ast_node!(
    /// The `where` keyword and its ordered predicates.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `where T: Base, T::Item: Read`:
    ///
    /// ```text
    /// WhereClause
    /// +-- WhereKw "where" [token]
    /// +-- WherePredicate (T: Base) [node] -> predicates()
    /// +-- Comma "," [token]
    /// `-- WherePredicate (T::Item: Read) [node] -> predicates()
    /// ```
    WhereClause,
    WhereClause
);

ast_node!(
    /// A type spelling constrained by a list of trait bounds.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `T: Base`:
    ///
    /// ```text
    /// WherePredicate
    /// +-- TypeRef (T) [node] -> target_type()
    /// +-- Colon ":" [token]
    /// `-- TraitBoundList (Base) [node] -> bounds()
    /// ```
    ///
    /// `target_type()` owns the full constrained type. `name()` follows its path and
    /// returns only the first segment; `name_text()` uses the target's normalized spelling.
    WherePredicate,
    WherePredicate
);

ast_node!(
    /// One or more trait spellings separated by `+`.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `Base + Read`:
    ///
    /// ```text
    /// TraitBoundList
    /// +-- TraitRef (Base) [node] -> bounds()
    /// +-- Plus "+" [token]
    /// `-- TraitRef (Read) [node] -> bounds()
    /// ```
    TraitBoundList,
    TraitBoundList
);

ast_node!(
    /// A trait path with generic arguments or callable input/output syntax.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `Read<i32>`:
    ///
    /// ```text
    /// TraitRef
    /// +-- Path (Read) [node] -> path()
    /// `-- GenericArgList (<i32>) [node] -> generic_args()
    /// ```
    ///
    /// `Fn(i32) -> bool`:
    ///
    /// ```text
    /// TraitRef
    /// +-- Path (Fn) [node] -> path()
    /// +-- LParen "(" [token]
    /// +-- TypeList (i32) [node] -> callable_inputs()
    /// +-- RParen ")" [token]
    /// +-- Arrow "->" [token]
    /// `-- TypeRef (bool) [node] -> callable_output()
    /// ```
    ///
    /// The callable form stores inputs under `TypeList` and output as a direct
    /// `TypeRef`. A missing arrow/output yields `None`; generic forms have neither
    /// callable component. A trait spelling is not a resolved trait identity.
    TraitRef,
    TraitRef
);

ast_node!(
    /// Comma-separated type spellings; delimiters belong to its parent.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `i32, bool`:
    ///
    /// ```text
    /// TypeList
    /// +-- TypeRef (i32) [node] -> types()
    /// +-- Comma "," [token]
    /// `-- TypeRef (bool) [node] -> types()
    /// ```
    TypeList,
    TypeList
);

ast_node!(
    /// Function or method parameters; parentheses belong to the declaration.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `x: i32, y: i32`:
    ///
    /// ```text
    /// ParamList
    /// +-- Param (x: i32) [node] -> params()
    /// +-- Comma "," [token]
    /// `-- Param (y: i32) [node] -> params()
    /// ```
    ParamList,
    ParamList
);

ast_node!(
    /// One named parameter, including the special method receiver `self`.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `x: i32`:
    ///
    /// ```text
    /// Param
    /// +-- Name (x) [node] -> name()
    /// +-- Colon ":" [token]
    /// `-- TypeRef (i32) [node] -> ty()
    /// ```
    ///
    /// `self`:
    ///
    /// ```text
    /// Param
    /// `-- Name (self) [node] -> name()
    /// ```
    ///
    /// A regular parameter has a type child. The special first method parameter
    /// `self` has only `Name`, so `ty()` legitimately returns `None`.
    Param,
    Param
);

ast_node!(
    /// Struct fields in source order; braces belong to the struct declaration.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `val x: i32, var y: i32`:
    ///
    /// ```text
    /// FieldList
    /// +-- Field (val x: i32) [node] -> fields()
    /// +-- Comma "," [token]
    /// `-- Field (var y: i32) [node] -> fields()
    /// ```
    FieldList,
    FieldList
);

ast_node!(
    /// A struct field with visibility, writeability, name and type spelling.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `pub var x: i32`:
    ///
    /// ```text
    /// Field
    /// +-- PubKw "pub" [token]
    /// +-- VarKw "var" [token]
    /// +-- Name (x) [node] -> name()
    /// +-- Colon ":" [token]
    /// `-- TypeRef (i32) [node] -> ty()
    /// ```
    Field,
    Field
);

ast_node!(
    /// Enum variants in source order; braces belong to the enum declaration.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `Ready, Value(i32)`:
    ///
    /// ```text
    /// VariantList
    /// +-- Variant (Ready) [node] -> variants()
    /// +-- Comma "," [token]
    /// `-- Variant (Value(i32)) [node] -> variants()
    /// ```
    VariantList,
    VariantList
);

ast_node!(
    /// A unit variant or a variant with parenthesized payload types.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `Value(i32)`:
    ///
    /// ```text
    /// Variant
    /// +-- Name (Value) [node] -> name()
    /// +-- LParen "(" [token]
    /// +-- TypeList (i32) [node] -> payload_types()
    /// `-- RParen ")" [token]
    /// ```
    Variant,
    Variant
);

impl Name {
    /// Returns `Some("value")` for `value`, or `Some("self")` for a `self` path segment.
    /// Accepts identifiers and `crate`/`self`/`super`; returns `None` if no matching token remains.
    pub fn text(&self) -> Option<String> {
        self.syntax()
            .children_with_tokens()
            .find_map(|element| match element {
                NodeOrToken::Token(token)
                    if matches!(
                        token.kind(),
                        SyntaxKind::Ident
                            | SyntaxKind::CrateKw
                            | SyntaxKind::SelfKw
                            | SyntaxKind::SuperKw
                    ) =>
                {
                    Some(token.text().to_string())
                }
                _ => None,
            })
    }
}

impl Path {
    /// Iterates `crate`, `math`, then `Value` in `crate::math::Value`, excluding separators and trivia.
    pub fn segments(&self) -> impl Iterator<Item = Name> {
        support::children(self.syntax())
    }

    /// Returns `Some("crate::math::Value")` for `crate :: math :: Value`.
    /// Joins readable segments with `::`; returns `None` if none remain. Does not preserve trivia.
    pub fn text(&self) -> Option<String> {
        let segments = self
            .segments()
            .filter_map(|segment| segment.text())
            .collect::<Vec<_>>();
        (!segments.is_empty()).then(|| segments.join("::"))
    }
}

impl GenericParamList {
    /// Iterates `T: Read` then `U` in `<T: Read, U>`, preserving source order.
    pub fn params(&self) -> impl Iterator<Item = GenericParam> {
        support::children(self.syntax())
    }
}

impl GenericParam {
    /// Returns `T` in the generic parameter `T: Read`.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns `Some("T")` for `T: Read`, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns `Read + Write` in `T: Read + Write`; returns `None` for unbounded `T`.
    pub fn bounds(&self) -> Option<TraitBoundList> {
        support::child(self.syntax())
    }
}

impl GenericArgList {
    /// Iterates `Output = bool` in `<i32, Output = bool>`.
    /// Excludes the positional argument `i32`; `<i32>` yields no bindings.
    pub fn bindings(&self) -> impl Iterator<Item = AssociatedTypeBinding> {
        support::children(self.syntax())
    }

    /// Whether a positional type follows a binding: true for `<Output = bool, i32>`.
    /// False for `<i32, Output = bool>`; reports written order without validating the arguments.
    pub fn positional_after_binding(&self) -> bool {
        let mut binding_seen = false;
        for child in self.syntax().children() {
            if child.kind() == SyntaxKind::AssociatedTypeBinding {
                binding_seen = true;
            } else if child.kind() == SyntaxKind::TypeRef && binding_seen {
                return true;
            }
        }
        false
    }

    /// Iterates `i32` in `<i32, Output = bool>`, excluding the binding and its nested `bool`.
    /// `<Output = bool>` yields no positional arguments.
    pub fn args(&self) -> impl Iterator<Item = TypeRef> {
        support::children(self.syntax())
    }
}

impl WhereClause {
    /// Iterates `T: Read` then `T::Item: Write` in `where T: Read, T::Item: Write`.
    pub fn predicates(&self) -> impl Iterator<Item = WherePredicate> {
        support::children(self.syntax())
    }
}

impl WherePredicate {
    /// Returns `T::Item` in the predicate `T::Item: Read`.
    pub fn target_type(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }

    /// Returns the first target path segment `T` in `T::Item: Read`.
    /// For a structural target such as `(T, U): Read`, returns `None`; does not descend into that type.
    pub fn name(&self) -> Option<Name> {
        self.target_type()
            .and_then(|ty| ty.path())
            .and_then(|path| path.segments().next())
    }

    /// Returns `Some("T::Item")` for `T::Item: Read`, unlike `name()` which selects only `T`.
    /// Uses the target type spelling; structural targets such as `(T, U): Read` yield `None`.
    pub fn name_text(&self) -> Option<String> {
        self.target_type().and_then(|ty| ty.name_text())
    }

    /// Returns `Read + Write` in the predicate `T::Item: Read + Write`.
    pub fn bounds(&self) -> Option<TraitBoundList> {
        support::child(self.syntax())
    }
}

impl TraitBoundList {
    /// Iterates `Read` then `Write` in `T: Read + Write`, excluding the target `T`.
    pub fn bounds(&self) -> impl Iterator<Item = TraitRef> {
        support::children(self.syntax())
    }
}

impl TraitRef {
    /// Returns the input list `i32, bool` in `Fn(i32, bool) -> String`.
    /// Returns `None` for generic spelling `Read<i32>`.
    pub fn callable_inputs(&self) -> Option<TypeList> {
        support::child(self.syntax())
    }

    /// Returns `String` in `Fn(i32) -> String`.
    /// Returns `None` when no output is written, or for generic spelling `Read<i32>`.
    pub fn callable_output(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }

    /// Returns `Read` in `Read<i32>`, or `Fn` in `Fn(i32) -> bool`.
    pub fn path(&self) -> Option<Path> {
        support::child(self.syntax())
    }

    /// Returns `Some("io::Read")` for `io :: Read<i32>`, omitting type arguments and trivia.
    /// Returns `None` if no readable path remains.
    pub fn path_text(&self) -> Option<String> {
        self.path().and_then(|path| path.text())
    }

    /// Returns `<i32>` in `Read<i32>`; returns `None` in `Read` or `Fn(i32) -> bool`.
    pub fn generic_args(&self) -> Option<GenericArgList> {
        support::child(self.syntax())
    }
}

impl TypeList {
    /// Iterates `i32` then `bool` in the payload list of `Pair(i32, bool)`.
    /// Delimiters belong to the parent; an empty list yields no types.
    pub fn types(&self) -> impl Iterator<Item = TypeRef> {
        support::children(self.syntax())
    }
}

impl ParamList {
    /// Iterates `x: i32` then `y: bool` in `fn check(x: i32, y: bool) {}`.
    /// Includes a written `self` parameter in methods; an empty list yields no parameters.
    pub fn params(&self) -> impl Iterator<Item = Param> {
        support::children(self.syntax())
    }
}

impl Param {
    /// Returns `x` in `x: i32`, or `self` for a method receiver.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns `Some("x")` for `x: i32`, or `Some("self")` for a receiver.
    /// Returns `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns `i32` in `x: i32`; returns `None` for a bare method receiver `self`.
    pub fn ty(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }
}

impl FieldList {
    /// Iterates `val x: i32` then `var y: i32` in `struct Point { val x: i32, var y: i32 }`.
    pub fn fields(&self) -> impl Iterator<Item = Field> {
        support::children(self.syntax())
    }
}

impl Field {
    /// Returns `Visibility::Public` for `pub val x: i32`, and `Private` without `pub`.
    /// Prefixes `pub(super)` and `pub(crate)` select `PublicSuper` and `PublicCrate` respectively.
    /// Reads direct tokens; does not check whether the visibility is legal in this context.
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    /// Returns `Some(Writeability::Val)` for `val x: i32` and `Some(Writeability::Var)` for `var x: i32`.
    /// Returns `None` if neither direct keyword is present.
    pub fn writeability(&self) -> Option<Writeability> {
        if support::token(self.syntax(), SyntaxKind::ValKw).is_some() {
            Some(Writeability::Val)
        } else if support::token(self.syntax(), SyntaxKind::VarKw).is_some() {
            Some(Writeability::Var)
        } else {
            None
        }
    }

    /// Whether the field uses `val`: true for `val x: i32`, false for `var x: i32`.
    pub fn is_val(&self) -> bool {
        self.writeability() == Some(Writeability::Val)
    }

    /// Whether the field uses `var`: true for `var x: i32`, false for `val x: i32`.
    pub fn is_var(&self) -> bool {
        self.writeability() == Some(Writeability::Var)
    }

    /// Returns `x` in `pub var x: i32`, excluding visibility and writeability.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns `Some("x")` for `pub var x: i32`, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns `i32` in `pub var x: i32`.
    pub fn ty(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }
}

impl VariantList {
    /// Iterates `Empty` then `Value(i32)` in `enum Slot { Empty, Value(i32) }`.
    pub fn variants(&self) -> impl Iterator<Item = Variant> {
        support::children(self.syntax())
    }
}

impl Variant {
    /// Returns `Value` in `Value(i32)`, or `Empty` in the unit variant `Empty`.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns `Some("Value")` for `Value(i32)`, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns the payload list `i32, bool` in `Value(i32, bool)`.
    /// Returns `None` for the unit variant `Empty`.
    pub fn payload_types(&self) -> Option<TypeList> {
        support::child(self.syntax())
    }
}
