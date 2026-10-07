//! Constant declarations and their semantic owners.

use crate::hir::ids::{ConstId, ExprId, ImplId, TraitId, TypeRefId};
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

/// Ordered `Vec<ConstItem>` storage for the owning declaration records.
pub type ConstBuffer = Vec<ConstItem>;
