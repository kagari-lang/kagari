//! Unresolved type syntax, distinct from the semantic type model in `crate::types`.

use smallvec::SmallVec;

use crate::hir::ids::TypeRefId;

/// Type syntax addressed by [`TypeRefId`], before semantic type resolution.
///
/// ```text
/// Pair<i32, String>
/// t0 -> Generic { name: "Pair", args: [t1, t2], bindings: [], ... }
/// +-- t1 -> Named("i32")
/// `-- t2 -> Named("String")
///
/// <T as Trait>::Output
/// t3 -> Projection { receiver: t4, trait_ref: t5, member: "Output", arguments: [] }
/// +-- t4 -> Named("T")
/// `-- t5 -> trait type syntax
/// ```
///
/// All arrows enter `Body.types` through `Body::type_ref`. These are source handles,
/// not the semantic [`crate::types::TypeId`] enum. Grouping parentheses collapse;
/// missing required syntax can become `Named("<missing>")`.
#[derive(Debug, Clone)]
pub struct TypeData {
    /// Source type form and child type-syntax IDs.
    pub kind: TypeKind,
}

/// Type spelling retained for later name resolution, substitution and checking.
#[derive(Debug, Clone)]
pub enum TypeKind {
    /// A bare type path such as `i32` or `m::Point`, or a recovery placeholder.
    Named(String),
    /// A type/trait application with positional arguments and associated bindings.
    Generic {
        /// Unresolved constructor/trait path.
        name: String,
        /// Positional type arguments in source order.
        args: TypeBuffer,
        /// Named associated-type constraints such as `Output = i32`.
        bindings: Vec<(String, TypeRefId)>,
        /// Records invalid argument ordering so semantic diagnostics can report it.
        positional_after_binding: bool,
        /// Marks callable-trait syntax rather than ordinary angle-bracket syntax.
        callable_syntax: bool,
    },
    /// A qualified associated-type projection such as `<T as Trait>::Output`.
    Projection {
        /// Arguments applied to the associated type member.
        arguments: TypeBuffer,
        /// Type whose implementation provides the member.
        receiver: TypeRefId,
        /// Trait qualification expressed as type syntax.
        trait_ref: TypeRefId,
        /// Associated member name.
        member: String,
    },
    /// Tuple element type IDs; an empty sequence represents unit syntax.
    Tuple(TypeBuffer),
    /// Element type for `[T]`; this spelling does not store an array length.
    Array(TypeRefId),
    /// A function type with explicit parameter and result syntax.
    Function {
        /// Parameter type IDs in declaration order.
        params: TypeBuffer,
        /// Result type ID; missing required syntax uses a placeholder.
        result: TypeRefId,
    },
}

/// Type-syntax IDs with four inline slots; additional elements spill to the heap.
pub type TypeBuffer = SmallVec<[TypeRefId; 4]>;
