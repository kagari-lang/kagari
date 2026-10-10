//! Assignment-path syntax kept separate from ordinary value expressions.

use crate::hir::ids::{ExprId, PlaceId};

/// A potential assignment destination in Body.places, with only `kind` as payload.
///
/// ```text
/// items[next_index()].count += step();
/// StmtKind::Assign { target: p2, op: Some(Add), value: rhs_call }
/// Body.place(p2) -> PlaceData { kind: Field { base: p1, name: "count" } }
/// Body.place(p1) -> PlaceData { kind: Index { base: p0, index: index_call } }
/// Body.place(p0) -> PlaceData { kind: Name("items") }
/// index_call -> Body.expr -> Call for next_index()
/// rhs_call   -> Body.expr -> Call for step()
/// ```
///
/// The statement owns a target LINK, not a recursive expression subtree. Place
/// links enter Body.places; index/right-hand values enter Body.exprs. Lowering
/// preserves the target once for checked left-to-right mutation semantics; this
/// structure is not a second call to next_index(). Type checking supplies binding,
/// field/index and writeability facts before the compiler emits mutation steps.
#[derive(Debug, Clone)]
pub struct PlaceData {
    /// Root or projection of a potential assignment destination.
    pub kind: PlaceKind,
}

/// An unresolved assignment root or projection from another place.
///
/// | Assignment source | Target payload |
/// | --- | --- |
/// | `n = value;` | `Name("n")` |
/// | `object.count = value;` | `Field { base: object_place, name: "count" }` |
/// | `items[i] = value;` | `Index { base: items_place, index: i_expr }` |
/// | `get_object().count = value;` | Field whose base is `Expr(call_expr)` |
///
/// `base` is always a PlaceId; `index` and an Expr root are ExprIds. Name/Field
/// strings preserve spelling, not resolved bindings/members. A general expression
/// root is retained for later validation: even invalid `1 = value;` can have a
/// place containing an expression without becoming writable. Ordinary reads
/// `object.count` and `items[i]` instead use ExprKind::Field/Index.
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
