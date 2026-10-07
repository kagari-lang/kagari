//! Function declarations, inline parameters and method origin tags.

use crate::hir::{
    ids::{BlockId, FunctionId, ParamId, TypeRefId},
    item::behavior::{GenericParamBuffer, TraitBoundBuffer},
    writeability::Writeability,
};
use kagari_types::visibility::Visibility;

/// A function signature's source structure and optional body block.
///
/// ```text
/// fn add(x: i32) -> i32 { x + 1 }
/// Module.functions[f.index()] -> Function
/// +-- id: f; kind: User; name: "add"
/// +-- params: [Param { id: p, name: "x", ty: t0, ... }]
/// +-- return_type: Some(t1) -> Body.type_ref(t1) -> Named("i32")
/// `-- body: Some(b) -> Body.block(b) -> BlockData { tail_expr: Some(sum), ... }
/// ```
///
/// Type annotations are syntax IDs, not checked types. Lowering installs
/// `HirOwner::Body(BodyOwner::Function(f))` while building parameters, signature
/// types and body, then restores the previous owner. Method functions also occupy
/// `Module.functions`; their trait/impl records refer here by `FunctionId`.
#[derive(Debug, Clone)]
pub struct Function {
    /// Slot in the enclosing module's function collection.
    pub id: FunctionId,
    /// Free function, trait method or implementation method origin.
    pub kind: FunctionKind,
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

/// Where a function declaration originated; not its selected runtime calling convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    /// A free `fn` declaration, including a declaration-only installed function.
    User,
    /// A trait requirement or default method.
    TraitMethod,
    /// A method declared inside an implementation block.
    ImplMethod,
}

/// An inline function parameter record with a source-map-wide parameter identity.
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
