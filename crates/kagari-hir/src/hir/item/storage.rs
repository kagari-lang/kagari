//! Constant declarations and their semantic owners.

use crate::hir::ids::{ConstId, ExprId, ImplId, TraitId, TypeRefId};
use kagari_types::visibility::Visibility;

/// A constant definition stored in Module.consts, with an expression initializer.
///
/// ```text
/// pub const LIMIT: i32 = 10;
/// ConstItem { owner: None, id: c, visibility: Public, name: "LIMIT",
///             ty: Some(t), initializer: e }
/// c -> Module.constant(c); SourceMap.const_span(c)
/// t -> Body.type_ref -> Named("i32")
/// e -> Body.expr -> Literal { kind: Number, text: "10" }
/// initializer nodes carry HirOwner::Body(BodyOwner::Const(c))
/// ```
///
/// `owner: None` means module-level. A trait default or impl associated constant
/// initializer uses `Some(ConstOwner::Trait/Impl(...))` and an AssociatedConst
/// member links to `c`; a trait requirement without an initializer has no ConstItem.
/// `id`/owner context are synthesized; visibility/name/annotation/value come from
/// syntax. An absent annotation is retained as `ty: None` for later validation,
/// whereas missing initializer syntax creates ExprKind::Missing, not a `None`.
/// Constant checking/evaluation publishes scalar facts separately; this record
/// neither stores an evaluated value nor proves const-safe semantics.
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

/// Associated declaration container of a ConstItem; unrelated to its node allocation owner.
///
/// `trait R { const N: i32 = 1; }` assigns `ConstItem.owner = Some(Trait(r))`;
/// `impl R for S { const N: i32 = 2; }` assigns `Some(Impl(i))`.
/// `r`/`i` index Module.traits/impls. A free `const N: i32 = 1;` has `owner: None`.
/// In all three cases initializer nodes are owned by BodyOwner::Const(c), not
/// directly by the trait/impl. Associated checking consumes the container link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstOwner {
    /// A trait default constant initializer.
    Trait(TraitId),
    /// An implementation's associated constant initializer.
    Impl(ImplId),
}

/// Ordered `Vec<ConstItem>` storage for the owning declaration records.
pub type ConstBuffer = Vec<ConstItem>;
