//! Blocks and statements: separate statement sequences, value tails and mutation places.

use crate::hir::{
    expr::{Condition, ops::BinaryOp},
    ids::{BlockId, ExprId, LocalId, PatternId, PlaceId, StmtId, TypeRefId},
    writeability::Writeability,
};
use smallvec::SmallVec;

/// A block's ordered statements and optional value-producing tail.
///
/// ```text
/// { val y = x + 1; y }
/// BlockData
/// +-- statements: [s] -> Body.stmt(s) -> Binding { local: l, initializer: sum, ... }
/// `-- tail_expr: Some(e) -> Body.expr(e) -> Name { name: "y", ... }
/// ```
///
/// The tail is not included in `statements`. An empty block or one ending only in
/// statements has no tail; this is not a missing-expression error.
#[derive(Debug, Clone)]
pub struct BlockData {
    /// Statement IDs in source order, excluding the tail expression.
    pub statements: StmtBuffer,
    /// Final expression without a terminating semicolon, when present.
    pub tail_expr: Option<ExprId>,
}

/// A statement payload stored in [`crate::hir::body::Body`] under a `StmtId`.
#[derive(Debug, Clone)]
pub struct StmtData {
    /// Control-flow, binding, assignment or discarded-expression form.
    pub kind: StmtKind,
}

/// Statement forms with explicit links to expressions, places, blocks and bindings.
#[derive(Debug, Clone)]
pub enum StmtKind {
    /// A `val` or `var` local binding, such as `val y = x + 1;`.
    Binding {
        /// Allocated binding identity used by resolution and type tables.
        local: LocalId,
        /// Whether the binding permits reassignment.
        writeability: Writeability,
        /// Declared name; may be empty in recovered syntax.
        name: String,
        /// Explicit type annotation, or `None` when inference is requested.
        ty: Option<TypeRefId>,
        /// Initializer expression; missing syntax points to `ExprKind::Missing`.
        initializer: ExprId,
    },
    /// An assignment such as `a[i] += value`, retaining a place rather than a read expression.
    Assign {
        /// Writable-path syntax, resolved and checked later.
        target: PlaceId,
        /// Compound operator, or `None` for plain `=`.
        op: Option<BinaryOp>,
        /// Right-hand value expression.
        value: ExprId,
    },
    /// A `return` with an optional value.
    Return {
        /// Returned value, or `None` for bare `return`.
        expr: Option<ExprId>,
    },
    /// A `while` with an ordinary or pattern-binding condition.
    While {
        /// Condition evaluated before each iteration.
        condition: Condition,
        /// Loop body block.
        body: BlockId,
    },
    /// A statement-position unconditional `loop`.
    Loop {
        /// Loop body block; value-position loops also have an expression form.
        body: BlockId,
    },
    /// A `for pattern in iterable` loop before iterable protocol selection.
    For {
        /// Pattern introducing iteration bindings.
        pattern: PatternId,
        /// Source iterable expression; selected protocol facts belong to type checking.
        iterable: ExprId,
        /// Block run for each matched element.
        body: BlockId,
    },
    /// A bare `break`; loop validity is checked later.
    Break,
    /// A `break value` carrying the result expression for a value-producing loop.
    BreakValue(ExprId),
    /// A `continue` to the enclosing loop.
    Continue,
    /// An expression used as a statement rather than the block tail.
    Expr(ExprId),
}

/// Ordered statement IDs with eight inline slots; the inline capacity is not a limit.
pub type StmtBuffer = SmallVec<[StmtId; 8]>;
