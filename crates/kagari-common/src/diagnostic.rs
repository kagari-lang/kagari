use std::fmt::{self, Display, Formatter};

use crate::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticKind {
    UnexpectedToken,
    ExpectedTopLevelItem,
    ExpectedFunctionKeyword,
    ExpectedStructKeyword,
    ExpectedEnumKeyword,
    ExpectedLetKeyword,
    ExpectedReturnKeyword,
    ExpectedBreakKeyword,
    ExpectedContinueKeyword,
    ExpectedFunctionName,
    ExpectedStructName,
    ExpectedEnumName,
    ExpectedFieldName,
    ExpectedVariantName,
    ExpectedIfKeyword,
    ExpectedMatchKeyword,
    ExpectedElseBranch,
    ExpectedWhileKeyword,
    ExpectedLoopKeyword,
    ExpectedFunctionParameterListStart,
    ExpectedFunctionParameterListEnd,
    ExpectedClosingParen,
    ExpectedClosingBracket,
    ExpectedParameterName,
    ExpectedParameterTypeSeparator,
    ExpectedFieldTypeSeparator,
    ExpectedStructLiteralBodyStart,
    ExpectedMatchBodyStart,
    ExpectedMatchPattern,
    ExpectedMatchArmArrow,
    ExpectedType,
    ExpectedLetBindingName,
    ExpectedLetInitializer,
    ExpectedFunctionBodyStart,
    ExpectedStructBodyStart,
    ExpectedBlockEnd,
    ExpectedStatementTerminator,
    ExpectedExpression,
    MissingFunctionName,
    DuplicateFunction {
        name: String,
    },
    UnknownType {
        type_name: String,
        function_name: String,
        position: TypePosition,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypePosition {
    Parameter,
    Return,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub kind: DiagnosticKind,
    pub span: Option<Span>,
}

impl Diagnostic {
    pub fn error(kind: DiagnosticKind) -> Self {
        Self {
            severity: Severity::Error,
            kind,
            span: None,
        }
    }

    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }
}

impl Display for DiagnosticKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedToken => write!(f, "unexpected token"),
            Self::ExpectedTopLevelItem => write!(f, "expected a top-level item"),
            Self::ExpectedFunctionKeyword => write!(f, "expected `fn`"),
            Self::ExpectedStructKeyword => write!(f, "expected `struct`"),
            Self::ExpectedEnumKeyword => write!(f, "expected `enum`"),
            Self::ExpectedLetKeyword => write!(f, "expected `let`"),
            Self::ExpectedReturnKeyword => write!(f, "expected `return`"),
            Self::ExpectedBreakKeyword => write!(f, "expected `break`"),
            Self::ExpectedContinueKeyword => write!(f, "expected `continue`"),
            Self::ExpectedFunctionName => write!(f, "expected function name"),
            Self::ExpectedStructName => write!(f, "expected struct name"),
            Self::ExpectedEnumName => write!(f, "expected enum name"),
            Self::ExpectedFieldName => write!(f, "expected field name"),
            Self::ExpectedVariantName => write!(f, "expected variant name"),
            Self::ExpectedIfKeyword => write!(f, "expected `if`"),
            Self::ExpectedMatchKeyword => write!(f, "expected `match`"),
            Self::ExpectedElseBranch => write!(f, "expected `if` or block after `else`"),
            Self::ExpectedWhileKeyword => write!(f, "expected `while`"),
            Self::ExpectedLoopKeyword => write!(f, "expected `loop`"),
            Self::ExpectedFunctionParameterListStart => {
                write!(f, "expected `(` after function name")
            }
            Self::ExpectedFunctionParameterListEnd => {
                write!(f, "expected `)` after parameters")
            }
            Self::ExpectedClosingParen => write!(f, "expected `)`"),
            Self::ExpectedClosingBracket => write!(f, "expected `]`"),
            Self::ExpectedParameterName => write!(f, "expected parameter name"),
            Self::ExpectedParameterTypeSeparator => {
                write!(f, "expected `:` after parameter name")
            }
            Self::ExpectedFieldTypeSeparator => write!(f, "expected `:` after field name"),
            Self::ExpectedStructLiteralBodyStart => {
                write!(f, "expected `{{` to start struct literal")
            }
            Self::ExpectedMatchBodyStart => write!(f, "expected `{{` to start match body"),
            Self::ExpectedMatchPattern => write!(f, "expected match pattern"),
            Self::ExpectedMatchArmArrow => write!(f, "expected `=>` after match pattern"),
            Self::ExpectedType => write!(f, "expected type"),
            Self::ExpectedLetBindingName => write!(f, "expected let binding name"),
            Self::ExpectedLetInitializer => write!(f, "expected `=` after let binding name"),
            Self::ExpectedFunctionBodyStart => {
                write!(f, "expected `{{` to start function body")
            }
            Self::ExpectedStructBodyStart => {
                write!(f, "expected `{{` to start struct body")
            }
            Self::ExpectedBlockEnd => write!(f, "expected `}}` to end block"),
            Self::ExpectedStatementTerminator => write!(f, "expected `;` after statement"),
            Self::ExpectedExpression => write!(f, "expected expression"),
            Self::MissingFunctionName => write!(f, "missing function name"),
            Self::DuplicateFunction { name } => write!(f, "duplicate function `{name}`"),
            Self::UnknownType {
                type_name,
                function_name,
                position: TypePosition::Parameter,
            } => write!(
                f,
                "unknown parameter type `{type_name}` in function `{function_name}`"
            ),
            Self::UnknownType {
                type_name,
                function_name,
                position: TypePosition::Return,
            } => write!(
                f,
                "unknown return type `{type_name}` in function `{function_name}`"
            ),
        }
    }
}

impl Display for Diagnostic {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.span {
            Some(span) => write!(
                f,
                "{:?}: {} at {}..{}",
                self.severity, self.kind, span.start, span.end
            ),
            None => write!(f, "{:?}: {}", self.severity, self.kind),
        }
    }
}
