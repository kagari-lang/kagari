use kagari_common::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Fn,
    Ident(String),
    Number(String),
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Semi,
    Arrow,
    Eof,
    Unknown(char),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}
