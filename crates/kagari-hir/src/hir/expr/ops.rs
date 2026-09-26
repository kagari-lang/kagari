#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    IdentityEq,
    IdentityNotEq,
    Lt,
    Gt,
    Le,
    Ge,
    AndAnd,
    OrOr,
}
