//! Blocks and statements: separate statement sequences, value tails and mutation places.

use crate::hir::{
    expr::{Condition, ops::BinaryOp},
    ids::{BlockId, ExprId, LocalId, PatternId, PlaceId, StmtId, TypeRefId},
    writeability::Writeability,
};
use smallvec::SmallVec;

/// A block's ordered statement links and optional final value expression.
///
/// ```text
/// { val y: i32 = x + 1; y }
/// BlockData { statements: [s], tail_expr: Some(e) }
/// s -> Body.stmt -> StmtData { kind: Binding { local: l, writeability: Val,
///      name: "y", ty: Some(t), initializer: sum } }
/// t -> Body.type_ref -> Named("i32")
/// sum -> Body.expr -> Binary { lhs: x_expr, op: Add, rhs: one }
/// e -> Body.expr -> Name { name: "y", explicit_type: None }
/// ```
///
/// A tail has no terminating semicolon and is excluded from `statements`.
/// `{ y; }` instead has one Expr statement and `tail_expr: None`; `{}` has an
/// empty statement list and no tail. These absences are legal, not Missing nodes.
/// Block lowering creates the row in Body.blocks; resolution/checking follow its
/// IDs in source order. An if/loop statement or function body stores a BlockId;
/// a block used as a value is additionally wrapped by ExprKind::Block.
#[derive(Debug, Clone)]
pub struct BlockData {
    /// Statement IDs in source order, excluding the tail expression.
    pub statements: StmtBuffer,
    /// Final expression without a terminating semicolon, when present.
    pub tail_expr: Option<ExprId>,
}

/// A statement row in Body.stmts; its only payload field is `kind`.
///
/// `var n = 1;` becomes `StmtData { kind: Binding { local: l, writeability: Var,
/// name: "n", ty: None, initializer: one } }`. The owning block stores its StmtId;
/// Body::stmt performs qualified lookup. Local identity/type/source facts are
/// separate from this row; see StmtKind for every source-to-payload form.
#[derive(Debug, Clone)]
pub struct StmtData {
    /// Control-flow, binding, assignment or discarded-expression form.
    pub kind: StmtKind,
}

/// Statement syntax with explicit expression, place, block and binding links.
///
/// Symbolic IDs refer to the matching Body table; payloads below show every field.
///
/// | Source fragment | Kind |
/// | --- | --- |
/// | `val n: i32 = 1;` | `Binding { local: l, writeability: Val, name: "n", ty: Some(i32_type), initializer: one }` |
/// | `var n = 1;` | Same Binding, with `Var` and `ty: None` (inference) |
/// | `items[i] += step();` | `Assign { target: indexed_place, op: Some(Add), value: call }` |
/// | `n = 2;` | `Assign { target: name_place, op: None, value: two }` |
/// | `return n;` / `return;` | `Return { expr: Some(n_expr) }` / `Return { expr: None }` |
/// | `while ready { work(); }` | `While { condition: Condition::Expr(ready_expr), body: block }` |
/// | `loop { work(); }` in statement position | `Loop { body: block }` |
/// | `for (x, y) in pairs { work(x); }` | `For { pattern: tuple_pattern, iterable: pairs_expr, body: block }` |
/// | `break;` / `break n;` / `continue;` | `Break` / `BreakValue(n_expr)` / `Continue` |
/// | `work();` | `Expr(call)`; the Call itself lives in Body.exprs |
///
/// `local` is allocated, not written syntax. It keys binding facts; it is not an
/// expression index. Missing initializers recover as ExprKind::Missing. Mutation
/// targets enter Body.places, retaining the writable path once rather than
/// desugaring `+=` to a duplicated target read. Body.patterns owns for patterns,
/// and Body.blocks owns loop blocks. Binding while conditions use Condition::Binding.
///
/// `if ...` and bare blocks in statement position are Expr statements wrapping
/// ExprKind::If/Block; there are no separate If/Block statement variants. A final
/// value-position loop uses ExprKind::Loop. Lowering records syntax; checking
/// validates writes, conditions, iteration and loop-control scope. Named local
/// `fn` declarations currently have no statement form.
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
