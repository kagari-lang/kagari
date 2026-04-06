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
    TopLevelControlFlowNotAllowed,
    ExpectedFunctionKeyword,
    ExpectedConstKeyword,
    ExpectedStaticKeyword,
    ExpectedStructKeyword,
    ExpectedEnumKeyword,
    ExpectedLetKeyword,
    ExpectedReturnKeyword,
    ExpectedBreakKeyword,
    ExpectedContinueKeyword,
    ExpectedFunctionName,
    ExpectedConstName,
    ExpectedStaticName,
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
    ExpectedConstInitializer,
    ExpectedStaticInitializer,
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
    UnknownConstType {
        const_name: String,
        type_name: String,
    },
    InvalidConstInitializer {
        const_name: String,
        reason: String,
    },
    ConstCycle {
        const_name: String,
    },
    ConstWriteNotAllowed {
        const_name: String,
    },
    UnknownStaticType {
        static_name: String,
        type_name: String,
    },
    CallArityMismatch {
        function_name: String,
        expected: usize,
        found: usize,
    },
    ArgumentTypeMismatch {
        function_name: String,
        parameter_name: String,
        expected: String,
        found: String,
    },
    InvalidAssignmentTarget,
    AssignmentTypeMismatch {
        expected: String,
        found: String,
    },
    IfBranchTypeMismatch {
        expected: String,
        found: String,
    },
    MatchArmTypeMismatch {
        expected: String,
        found: String,
    },
    ReturnTypeMismatch {
        function_name: String,
        expected: String,
        found: String,
    },
    BreakOutsideLoop,
    ContinueOutsideLoop,
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
            Self::TopLevelControlFlowNotAllowed => {
                write!(
                    f,
                    "top-level `return`, `break`, and `continue` are not allowed"
                )
            }
            Self::ExpectedFunctionKeyword => write!(f, "expected `fn`"),
            Self::ExpectedConstKeyword => write!(f, "expected `const`"),
            Self::ExpectedStaticKeyword => write!(f, "expected `static`"),
            Self::ExpectedStructKeyword => write!(f, "expected `struct`"),
            Self::ExpectedEnumKeyword => write!(f, "expected `enum`"),
            Self::ExpectedLetKeyword => write!(f, "expected `let`"),
            Self::ExpectedReturnKeyword => write!(f, "expected `return`"),
            Self::ExpectedBreakKeyword => write!(f, "expected `break`"),
            Self::ExpectedContinueKeyword => write!(f, "expected `continue`"),
            Self::ExpectedFunctionName => write!(f, "expected function name"),
            Self::ExpectedConstName => write!(f, "expected const name"),
            Self::ExpectedStaticName => write!(f, "expected static name"),
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
            Self::ExpectedConstInitializer => write!(f, "expected `=` after const name"),
            Self::ExpectedStaticInitializer => write!(f, "expected `=` after static name"),
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
            Self::UnknownConstType {
                const_name,
                type_name,
            } => write!(
                f,
                "unknown const type `{type_name}` in const `{const_name}`"
            ),
            Self::InvalidConstInitializer { const_name, reason } => {
                write!(
                    f,
                    "invalid const initializer in const `{const_name}`: {reason}"
                )
            }
            Self::ConstCycle { const_name } => {
                write!(f, "cyclic const dependency involving `{const_name}`")
            }
            Self::ConstWriteNotAllowed { const_name } => {
                write!(
                    f,
                    "cannot perform write-like operation on const `{const_name}`"
                )
            }
            Self::UnknownStaticType {
                static_name,
                type_name,
            } => write!(
                f,
                "unknown static type `{type_name}` in static `{static_name}`"
            ),
            Self::CallArityMismatch {
                function_name,
                expected,
                found,
            } => write!(
                f,
                "call arity mismatch in `{function_name}`: expected {expected} arguments, found {found}"
            ),
            Self::ArgumentTypeMismatch {
                function_name,
                parameter_name,
                expected,
                found,
            } => write!(
                f,
                "argument type mismatch for parameter `{parameter_name}` in `{function_name}`: expected `{expected}`, found `{found}`"
            ),
            Self::InvalidAssignmentTarget => write!(f, "invalid assignment target"),
            Self::AssignmentTypeMismatch { expected, found } => write!(
                f,
                "assignment type mismatch: expected `{expected}`, found `{found}`"
            ),
            Self::IfBranchTypeMismatch { expected, found } => write!(
                f,
                "if branch type mismatch: expected `{expected}`, found `{found}`"
            ),
            Self::MatchArmTypeMismatch { expected, found } => write!(
                f,
                "match arm type mismatch: expected `{expected}`, found `{found}`"
            ),
            Self::ReturnTypeMismatch {
                function_name,
                expected,
                found,
            } => write!(
                f,
                "return type mismatch in function `{function_name}`: expected `{expected}`, found `{found}`"
            ),
            Self::BreakOutsideLoop => write!(f, "`break` used outside of a loop"),
            Self::ContinueOutsideLoop => write!(f, "`continue` used outside of a loop"),
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
