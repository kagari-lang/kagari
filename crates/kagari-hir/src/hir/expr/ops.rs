//! Operator tags with their source spellings; these are not executable instructions.

/// Source unary operators; overload/type selection occurs later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixOp {
    /// Arithmetic negation: `-`.
    Neg,
    /// Negation operator: `!`.
    Not,
}

/// Source binary operators, also reused by compound assignment forms.
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
