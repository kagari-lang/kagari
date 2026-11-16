//! Unchecked source/structured type references, distinct from semantic `crate::types` facts.

use kagari_common::identity::{ModuleIdentity, table::DefinitionId};
use smallvec::SmallVec;

use crate::hir::ids::TypeRefId;

/// A module-relative exposure route, independent of package-alias spelling.
///
/// In unchecked HIR this is a requested route, not access evidence. Resolution
/// validates the module/dependency boundary and every path component, then records
/// the selected route on the resolved type fact. For `api::Public` re-exporting a
/// private `api::hidden::Secret`, this names module `api` and path `["Public"]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionExposure {
    pub module: ModuleIdentity,
    pub path: Vec<String>,
}

/// One unresolved type-syntax row in HirNodes.types, addressed by TypeRefId.
///
/// ```text
/// val value: Pair<i32, String> = ...;
/// binding.ty = Some(t)
/// HirNodes.type_ref(t) -> TypeData { kind: Generic {
///     name: "Pair", args: [int_type, string_type], bindings: [],
///     positional_after_binding: false, callable_syntax: false,
/// } }
/// int_type -> TypeData { kind: Named("i32") }
/// string_type -> TypeData { kind: Named("String") }
/// ```
///
/// Only `kind` is stored; arena/owner qualification is in t and the storage row.
/// Lowering produces these records from annotations, trait applications and some
/// synthetic name/type sites. Signature/body checking publishes semantic types
/// separately. TypeRefId is a syntax handle; crate::types::TypeId is a semantic
/// enum, not this table's index. Grouping parentheses collapse, and missing
/// required syntax can become `Named("<missing>")`.
#[derive(Debug, Clone)]
pub struct TypeData {
    /// Source type form and child type-syntax IDs.
    pub kind: TypeKind,
}

/// Source spelling or an exact structured declaration, with unchecked child applications.
/// Exact references skip spelling selection, not visibility, eligibility, arity or bounds.
///
/// Symbolic child IDs below enter HirNodes.types through HirNodes::type_ref; quoted names
/// are actual strings. Examples are syntax fragments, not standalone programs.
///
/// | Source type | Stored kind (all fields shown) |
/// | --- | --- |
/// | `i32`, `m::Point`, `T::Item` | `Named("i32")`, `Named("m::Point")`, `Named("T::Item")` |
/// | `Pair<i32, String>` | `Generic { name: "Pair", args: [i32_type, string_type], bindings: [], positional_after_binding: false, callable_syntax: false }` |
/// | `Reader<i32, Item = String>` | `Generic { name: "Reader", args: [i32_type], bindings: [("Item", string_type)], positional_after_binding: false, callable_syntax: false }` |
/// | `<T as Reader>::Item` | `Projection { arguments: [], receiver: T_type, trait_ref: reader_type, member: "Item" }` |
/// | `<T as Family>::Item<i32>` | Same Projection, with `arguments: [i32_type]` and trait_ref for Family |
/// | `(i32, String)` / `()` | `Tuple([i32_type, string_type])` / `Tuple([])` |
/// | `[i32]` | `Array(i32_type)`; no stored length |
/// | `fn(i32, String) -> bool` | `Function { params: [i32_type, string_type], result: bool_type }` |
///
/// `Generic.args` are positional INPUT types; `bindings` are named associated
/// OUTPUT equalities. In an invalid `Reader<Item = String, i32>` application,
/// `positional_after_binding: true` preserves the ordering error for checking.
/// Absence of `<...>` gives Named, rather than Generic with empty arguments.
///
/// In trait-bound syntax, `Fn(i32, String) -> bool` is normalized by trait-ref
/// lowering to `Generic { name: "Fn", args: [tuple_type],
/// bindings: [("Output", bool_type)], positional_after_binding: false,
/// callable_syntax: true }`, with `tuple_type = Tuple([i32_type, string_type])`.
/// This is distinct from lowercase `fn(...) -> ...`, which uses Function.
/// Grouped `(i32)` points directly to the inner type; inference `_` is a Named
/// spelling whose legality depends on the annotation context.
#[derive(Debug, Clone)]
pub enum TypeKind {
    /// Exact declaration application from structured registration. This is unchecked:
    /// the shared checker must establish accessibility, kind, arity and bounds.
    /// `name` is diagnostic text, never the route or binding authority.
    Definition {
        declaration: DefinitionId,
        name: String,
        /// Explicit unchecked exposure, or `None` to select a unique accessible
        /// public exposure from the actual catalog when the defining route is private.
        exposure: Option<DefinitionExposure>,
        args: TypeBuffer,
        bindings: Vec<(String, TypeRefId)>,
    },
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
    /// Element type for builtin `[T]`; the fixed object length is not part of its type.
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
