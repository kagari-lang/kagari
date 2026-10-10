//! Operator tags with their source spellings; these are not executable instructions.

/// Source unary operator tags used by ExprKind::Prefix.
///
/// `-x` maps to Prefix { op: Neg, expr: x_id }; `!ready` maps to
/// Prefix { op: Not, expr: ready_id }. Operand IDs enter Body.exprs; the tag
/// records spelling and does not select a checked numeric/boolean instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixOp {
    /// Arithmetic negation: `-`.
    Neg,
    /// Negation operator: `!`.
    Not,
}

/// Source binary operator tags, shared by infix expressions and compound assignments.
///
/// ```text
/// x + y   -> ExprKind::Binary { lhs: x_id, op: Add, rhs: y_id }
/// x += y; -> StmtKind::Assign { target: x_place, op: Some(Add), value: y_id }
/// x = y;  -> StmtKind::Assign { target: x_place, op: None, value: y_id }
/// ```
///
/// Each variant below names its token. Comparisons, identity comparisons and
/// short-circuit operators use this same source enum, but have different checked
/// semantics; a Binary node does not imply unconditional evaluation of both
/// operands. Type checking supplies selected operations/trait calls, and later
/// lowering preserves evaluation and trap order. The enum is not bytecode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    /// The `+` operator.
    Add,
    /// The `-` operator.
    Sub,
    /// The `*` operator.
    Mul,
    /// The `/` operator.
    Div,
    /// The `%` operator.
    Rem,
    /// The `&` operator.
    BitAnd,
    /// The `|` operator.
    BitOr,
    /// The `^` operator.
    BitXor,
    /// The `<<` operator.
    Shl,
    /// The `>>` operator.
    Shr,

    /// The `==` operator.
    Eq,
    /// The `!=` operator.
    NotEq,
    /// The `===` operator.
    IdentityEq,
    /// The `!==` operator.
    IdentityNotEq,
    /// The `<` operator.
    Lt,
    /// The `>` operator.
    Gt,
    /// The `<=` operator.
    Le,
    /// The `>=` operator.
    Ge,
    /// The `&&` operator.
    AndAnd,
    /// The `||` operator.
    OrOr,
}
