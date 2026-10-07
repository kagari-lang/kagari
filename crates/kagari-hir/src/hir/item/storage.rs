//! Constant declarations and lowering-time explicit export records.

use crate::hir::ids::{
    ConstId, EnumId, ExprId, FunctionId, ImplId, ModuleId, OpaqueTypeId, StructId, TraitId,
    TypeRefId, VariantId,
};
use kagari_types::visibility::Visibility;

/// A constant declaration and its initializer expression.
///
/// ```text
/// const LIMIT: i32 = 10;
/// Module.consts[c.index()] -> ConstItem { id: c, ty: Some(t), initializer: e, ... }
/// +-- t -> type syntax
/// `-- e -> Body.expr(e), owner = Body(Const(c))
/// ```
///
/// Associated initializers also occupy this collection. Evaluated scalar values and
/// their checked types belong to later semantic tables, not this record.
#[derive(Debug, Clone)]
pub struct ConstItem {
    /// Associated trait/impl container, or `None` for a free constant.
    pub owner: Option<ConstOwner>,
    /// Slot in `Module.consts`.
    pub id: ConstId,
    /// Declared visibility.
    pub visibility: Visibility,
    /// Constant name as written.
    pub name: String,
    /// Explicit annotation, when present.
    pub ty: Option<TypeRefId>,
    /// Initializer expression, possibly a recovery placeholder.
    pub initializer: ExprId,
}

/// The associated container of a constant initializer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstOwner {
    /// A trait default constant initializer.
    Trait(TraitId),
    /// An implementation's associated constant initializer.
    Impl(ImplId),
}

/// A lowering-local declaration/import reference used by an explicit export record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExportItem {
    /// Local opaque type handle; resolve it against the matching module.
    OpaqueType(OpaqueTypeId),
    /// Local function handle; resolve it against the matching module.
    Function(FunctionId),
    /// Local constant handle; resolve it against the matching module.
    Const(ConstId),
    /// Local child module handle; resolve it against the matching module.
    Module(ModuleId),
    /// Named import slot in `Module.imports`; no glob expansion is stored here.
    Import(usize),
    /// Local struct handle; resolve it against the matching module.
    Struct(StructId),
    /// Local enum handle; resolve it against the matching module.
    Enum(EnumId),
    /// Local enum variant handle; resolve it against the matching module.
    Variant(VariantId),
    /// Local trait handle; resolve it against the matching module.
    Trait(TraitId),
}

/// A named explicit export recorded during lowering; not the resolved export namespace.
#[derive(Debug, Clone)]
pub struct Export {
    /// Name exposed by this explicit export.
    pub name: String,
    /// Local declaration or import slot supplying that name.
    pub item: ExportItem,
}

/// Ordered `Vec<ConstItem>` storage for the owning declaration records.
pub type ConstBuffer = Vec<ConstItem>;
/// Ordered `Vec<Export>` storage for the owning declaration records.
pub type ExportBuffer = Vec<Export>;
