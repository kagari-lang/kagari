//! Assignment-path syntax kept separate from ordinary value expressions.

use crate::hir::ids::{ExprId, PlaceId};

/// An assignment destination, addressed through a `PlaceId`.
///
/// ```text
/// items[i].count += 1
/// StmtKind::Assign { target: p2, op: Some(Add), value: one }
/// Body.places[p2] -> Field { base: p1, name: "count" }
/// Body.places[p1] -> Index { base: p0, index: i_expr }
/// Body.places[p0] -> Name("items")
/// Body.exprs     -> i_expr, one                 // expressions, not place IDs
/// ```
///
/// Owner pairs/ID qualification are omitted here. Lowering retains the writable
/// path separately from ordinary read expressions; it does not establish that a
/// destination is writable. Type checking supplies that contract for later lowering.
#[derive(Debug, Clone)]
pub struct PlaceData {
    /// Root or projection of a potential assignment destination.
    pub kind: PlaceKind,
}

/// An unresolved place root or a field/index projection from another place.
#[derive(Debug, Clone)]
pub enum PlaceKind {
    /// A named root, such as `items`, resolved against bindings later.
    Name(String),
    /// A general expression root retained for later assignment validation.
    Expr(ExprId),
    /// A named field projection from a place.
    Field {
        /// Base place whose field is selected.
        base: PlaceId,
        /// Field spelling; selected member facts are separate.
        name: String,
    },
    /// An indexed projection from a place.
    Index {
        /// Base place whose indexed element is selected.
        base: PlaceId,
        /// Index value expression, evaluated under the mutation contract.
        index: ExprId,
    },
}
