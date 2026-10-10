//! Function declarations, inline parameters and method origin tags.

use crate::hir::{
    ids::{BlockId, FunctionId, ParamId, TypeRefId},
    item::behavior::{GenericParamBuffer, TraitBoundBuffer},
    writeability::Writeability,
};
use kagari_types::visibility::Visibility;

/// A function declaration's signature and optional body, stored in `Module.functions`.
///
/// ```text
/// pub fn echo<T: PartialEq>(x: T) -> T where T: Eq { x }
/// Function {
///     id: f, kind: User, is_async: false, visibility: Public, name: "echo",
///     generic_params: [GenericParam { id: g, name: "T", bounds: [partial_eq_ref] }],
///     bounds: [TraitBound { target: "T", target_ref: t_where, traits: [eq_ref] }],
///     params: [Param { id: p, writeability: Val, name: "x", ty: t_param }],
///     return_type: Some(t_result), body: Some(b),
/// }
/// t_where, t_param, t_result -> Body.type_ref -> TypeKind::Named("T")
/// partial_eq_ref / eq_ref -> TraitRef { ty: ... } -> trait type syntax
/// b -> Body.block -> BlockData { statements: [], tail_expr: Some(x_expr) }
/// x_expr -> Body.expr -> Name { name: "x", explicit_type: None }
/// ```
///
/// The traits require matching declarations from the installed foundation.
/// Symbolic IDs are allocator outputs, not written syntax or promised indices.
/// `f.index()` selects the function row and matching source-map slot; the stored
/// `id` repeats that identity for consumers holding only `&Function`.
///
/// Omitting `pub` selects `Private`; omitting generics/`where` leaves those buffers
/// empty. Omitting `-> T` sets `return_type` to `None` (checked as unit), whereas
/// `-> ()` stores `Some(unit_type)`. `async fn` sets `is_async`, while its written
/// result type still describes the completed body. Trait requirements and installed
/// declaration-only functions can have `body: None`; `{}` is `Some(empty_block)`.
///
/// Method signatures/bodies use this same collection: their `kind` is
/// `TraitMethod` or `ImplMethod`, with an external trait/impl member linking to `f`.
/// Impl binders can be prepended to method binders, and implicit `self` type syntax
/// is supplied by lowering. Ordinary function parameters use `ParamId`; closure
/// parameters use `LocalId` in `ExprKind::Closure`. Named local `fn` declarations
/// are not currently accepted in function bodies.
///
/// Lowering assigns `HirOwner::Body(BodyOwner::Function(f))` to signature/body
/// nodes. Type annotations enter `Body.types`, not the semantic type table;
/// resolution, checking and compiler lowering consume these source records later.
#[derive(Debug, Clone)]
pub struct Function {
    /// Slot in the enclosing module's function collection.
    pub id: FunctionId,
    /// Free function, trait method or implementation method origin.
    pub kind: FunctionKind,
    /// Explicit async factory; the written return type describes its completed body.
    pub is_async: bool,
    /// Declared source visibility, enforced by later lookup.
    pub visibility: Visibility,
    /// Declared name; may be empty in recovered syntax.
    pub name: String,
    /// Generic binders; method lowering can prepend inherited binders.
    pub generic_params: GenericParamBuffer,
    /// Where-clause requirements before trait/type resolution.
    pub bounds: TraitBoundBuffer,
    /// Inline parameter records in call order.
    pub params: ParamBuffer,
    /// Explicit result type syntax, absent when omitted.
    pub return_type: Option<TypeRefId>,
    /// Absent for interface requirements and installed native declarations.
    pub body: Option<BlockId>,
}

/// Declaration origin used by namespace collection and method-specific checking.
///
/// | Source location | Tag | Owning link |
/// | --- | --- | --- |
/// | `fn run() {}` at module level | `User` | `Item::Function(f)` |
/// | `trait R { fn run(self); }` or a default body | `TraitMethod` | `TraitMethod.function = f` |
/// | `impl R for S { fn run(self) {} }` | `ImplMethod` | `ImplMethod.function = f` |
///
/// All three use `Module.functions`. This tag neither identifies the particular
/// trait/impl nor chooses the runtime calling convention. `User` also covers
/// declaration-only installed functions; a closure is an expression, not a fourth
/// tag here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    /// A free `fn` declaration, including a declaration-only installed function.
    User,
    /// A trait requirement or default method.
    TraitMethod,
    /// A method declared inside an implementation block.
    ImplMethod,
}

/// A function parameter stored inline in `Function.params`, in source order.
///
/// ```text
/// fn add(x: i32) -> i32 { x + 1 }
/// Param { id: p, writeability: Val, name: "x", ty: t }
/// t -> Body.type_ref(t) -> TypeKind::Named("i32")
/// p -> SourceMap.param_span(p); resolved uses of x refer to Param(p)
/// ```
///
/// `id` is allocated, not derived from the spelling. Its index addresses the
/// allocation-wide parameter source map, not necessarily position zero in this
/// function. Ordinary parameter lowering sets `writeability: Val`; this does not
/// make a shared object deeply immutable. For a method's `self`, `name` is "self"
/// and lowering supplies its type syntax. Missing required annotations recover
/// with a placeholder type rather than an inference-enabled `None`.
#[derive(Debug, Clone)]
pub struct Param {
    /// Arena/owner-qualified identity; its index is not necessarily its position in `params`.
    pub id: ParamId,
    /// Parameter binding writeability assigned by lowering.
    pub writeability: Writeability,
    /// Source name, including `self` where applicable.
    pub name: String,
    /// Parameter type syntax; missing required types use a placeholder.
    pub ty: TypeRefId,
}

/// Ordered `Vec<Function>` storage for the owning declaration records.
pub type FunctionBuffer = Vec<Function>;
/// Ordered `Vec<Param>` storage for the owning declaration records.
pub type ParamBuffer = Vec<Param>;
