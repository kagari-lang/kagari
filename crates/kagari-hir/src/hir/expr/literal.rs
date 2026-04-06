#[derive(Debug, Clone)]
pub struct Literal {
    pub kind: LiteralKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiteralKind {
    Number,
    Float,
    String,
    Bool,
}
