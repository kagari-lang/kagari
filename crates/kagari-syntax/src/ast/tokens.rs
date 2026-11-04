//! Typed, spelling-free references into the owning parse's single lexical table.

use std::{marker::PhantomData, num::NonZeroU32};

use crate::{
    ast::{
        expr::{BinaryOp, PrefixOp, RangeOp},
        stmt::AssignmentOp,
    },
    kind::SyntaxKind,
};

/// A present lexical token of grammatical category `K`; construction is parser-owned.
#[derive(Debug)]
pub struct Token<K> {
    index: NonZeroU32,
    marker: PhantomData<K>,
}

impl<K> Token<K> {
    pub(crate) fn new(index: u32) -> Self {
        Self {
            index: NonZeroU32::new(index.checked_add(1).expect("present lexical index"))
                .expect("one-based lexical index"),
            marker: PhantomData,
        }
    }

    pub(crate) fn index(self) -> u32 {
        self.index.get() - 1
    }
}

impl<K> Copy for Token<K> {}

impl<K> Clone for Token<K> {
    fn clone(&self) -> Self {
        *self
    }
}

/// An operator's parsed alternative and actual contiguous lexical pieces.
#[derive(Debug)]
pub struct OperatorToken<O> {
    pub operation: O,
    pub(crate) first: u32,
    pub(crate) end: u32,
}

pub(crate) trait OperatorKind {
    fn syntax_kind(&self) -> SyntaxKind;
}

impl OperatorKind for BinaryOp {
    fn syntax_kind(&self) -> SyntaxKind {
        match self {
            Self::Add => SyntaxKind::Plus,
            Self::Subtract => SyntaxKind::Minus,
            Self::Multiply => SyntaxKind::Star,
            Self::Divide => SyntaxKind::Slash,
            Self::Remainder => SyntaxKind::Percent,
            Self::BitAnd => SyntaxKind::Amp,
            Self::BitOr => SyntaxKind::Pipe,
            Self::BitXor => SyntaxKind::Caret,
            Self::ShiftLeft => SyntaxKind::Shl,
            Self::ShiftRight => SyntaxKind::Shr,
            Self::And => SyntaxKind::AmpAmp,
            Self::Or => SyntaxKind::PipePipe,
            Self::Equal => SyntaxKind::EqEq,
            Self::NotEqual => SyntaxKind::NotEq,
            Self::IdentityEqual => SyntaxKind::IdentityEq,
            Self::IdentityNotEqual => SyntaxKind::IdentityNotEq,
            Self::Less => SyntaxKind::Lt,
            Self::LessEqual => SyntaxKind::Le,
            Self::Greater => SyntaxKind::Gt,
            Self::GreaterEqual => SyntaxKind::Ge,
        }
    }
}

impl OperatorKind for PrefixOp {
    fn syntax_kind(&self) -> SyntaxKind {
        match self {
            Self::Negate => SyntaxKind::Minus,
            Self::Not => SyntaxKind::Bang,
        }
    }
}

impl OperatorKind for RangeOp {
    fn syntax_kind(&self) -> SyntaxKind {
        match self {
            Self::Exclusive => SyntaxKind::DotDot,
            Self::Inclusive => SyntaxKind::DotDotEq,
        }
    }
}

impl OperatorKind for AssignmentOp {
    fn syntax_kind(&self) -> SyntaxKind {
        match self {
            Self::Assign => SyntaxKind::Eq,
            Self::Compound(operation) => match operation {
                BinaryOp::Add => SyntaxKind::PlusEq,
                BinaryOp::Subtract => SyntaxKind::MinusEq,
                BinaryOp::Multiply => SyntaxKind::StarEq,
                BinaryOp::Divide => SyntaxKind::SlashEq,
                BinaryOp::Remainder => SyntaxKind::PercentEq,
                BinaryOp::BitAnd => SyntaxKind::AmpEq,
                BinaryOp::BitOr => SyntaxKind::PipeEq,
                BinaryOp::BitXor => SyntaxKind::CaretEq,
                BinaryOp::ShiftLeft => SyntaxKind::ShlEq,
                BinaryOp::ShiftRight => SyntaxKind::ShrEq,
                _ => operation.syntax_kind(),
            },
        }
    }
}

macro_rules! markers {
    ($($name:ident),+ $(,)?) => { $(#[derive(Debug)] pub enum $name {})+ };
}

markers!(
    Lexeme,
    NameToken,
    Comma,
    Colon,
    ColonColon,
    Semi,
    Eq,
    Arrow,
    FatArrow,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Lt,
    Gt,
    Pipe,
    Dot,
    DotDot,
    DotDotEq,
    Hash,
    Question,
    Bang,
    AsKw,
    AsyncKw,
    AwaitKw,
    FnKw,
    ModKw,
    UseKw,
    PubKw,
    StructKw,
    EnumKw,
    TraitKw,
    ImplKw,
    ForKw,
    InKw,
    TypeKw,
    ConstKw,
    StaticKw,
    WhereKw,
    IfKw,
    ElseKw,
    MatchKw,
    LoopKw,
    WhileKw,
    ReturnKw,
    BreakKw,
    ContinueKw,
    ValKw,
    FormatStart,
    FormatEnd,
    FormatOpen,
    FormatClose,
    FormatText,
);
