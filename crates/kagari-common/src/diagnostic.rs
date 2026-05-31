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
    ExpectedModuleKeyword,
    ExpectedModuleName,
    ExpectedModuleBodyStart,
    ExpectedUseKeyword,
    ExpectedUseTree,
    ExpectedUseAlias,
    ExpectedPath,
    ExpectedTraitKeyword,
    ExpectedTraitName,
    ExpectedImplKeyword,
    ExpectedImplBodyStart,
    ExpectedForKeyword,
    ExpectedGenericParameterName,
    ExpectedWherePredicateSeparator,
    ExpectedFunctionKeyword,
    ExpectedConstKeyword,
    ExpectedStructKeyword,
    ExpectedEnumKeyword,
    ExpectedTraitBodyStart,
    ExpectedBindingKeyword,
    ExpectedReturnKeyword,
    ExpectedBreakKeyword,
    ExpectedContinueKeyword,
    ExpectedFunctionName,
    ExpectedConstName,
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
    ExpectedBindingName,
    ExpectedAssignmentOperator,
    ExpectedFieldBinding,
    ExpectedConstInitializer,
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
    InvalidAssignmentTarget {
        reason: String,
    },
    AssignmentTypeMismatch {
        expected: String,
        found: String,
    },
    ConditionTypeMismatch {
        context: &'static str,
        found: String,
    },
    BinaryOperandTypeMismatch {
        operator: &'static str,
        expected: String,
        lhs: String,
        rhs: String,
    },
    UnaryOperandTypeMismatch {
        operator: &'static str,
        expected: String,
        found: String,
    },
    ArrayElementTypeMismatch {
        expected: String,
        found: String,
    },
    InvalidStructInitializer {
        struct_name: String,
        reason: String,
    },
    UnknownTrait {
        trait_name: String,
    },
    InvalidTraitImpl {
        trait_name: String,
        type_name: String,
        reason: String,
    },
    InvalidInterfaceType {
        trait_name: String,
        reason: String,
    },
    TraitMethodMismatch {
        trait_name: String,
        method_name: String,
        reason: String,
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
    LegacyLetBinding,
    LegacyStaticItem,
    LegacyRefParameter,
    LegacyReceiverModifier,
    LegacyDynTrait,
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
            Self::ExpectedTopLevelItem => write!(
                f,
                "expected top-level item (`use`, `mod`, `fn`, `const`, `struct`, `enum`, `trait`, or `impl`)"
            ),
            Self::TopLevelControlFlowNotAllowed => {
                write!(
                    f,
                    "top-level `return`, `break`, and `continue` are not allowed"
                )
            }
            Self::ExpectedModuleKeyword => write!(f, "expected `mod`"),
            Self::ExpectedModuleName => write!(f, "expected module name"),
            Self::ExpectedModuleBodyStart => write!(f, "expected `{{` to start module body"),
            Self::ExpectedUseKeyword => write!(f, "expected `use`"),
            Self::ExpectedUseTree => write!(f, "expected import path or import group"),
            Self::ExpectedUseAlias => write!(f, "expected import alias"),
            Self::ExpectedPath => write!(f, "expected path"),
            Self::ExpectedTraitKeyword => write!(f, "expected `trait`"),
            Self::ExpectedTraitName => write!(f, "expected trait name"),
            Self::ExpectedImplKeyword => write!(f, "expected `impl`"),
            Self::ExpectedImplBodyStart => write!(f, "expected `{{` to start impl body"),
            Self::ExpectedForKeyword => write!(f, "expected `for`"),
            Self::ExpectedGenericParameterName => write!(f, "expected generic parameter name"),
            Self::ExpectedWherePredicateSeparator => {
                write!(f, "expected `:` after where predicate name")
            }
            Self::ExpectedFunctionKeyword => write!(f, "expected `fn`"),
            Self::ExpectedConstKeyword => write!(f, "expected `const`"),
            Self::ExpectedStructKeyword => write!(f, "expected `struct`"),
            Self::ExpectedEnumKeyword => write!(f, "expected `enum`"),
            Self::ExpectedTraitBodyStart => write!(f, "expected `{{` to start trait body"),
            Self::ExpectedBindingKeyword => write!(f, "expected `val` or `var`"),
            Self::ExpectedReturnKeyword => write!(f, "expected `return`"),
            Self::ExpectedBreakKeyword => write!(f, "expected `break`"),
            Self::ExpectedContinueKeyword => write!(f, "expected `continue`"),
            Self::ExpectedFunctionName => write!(f, "expected function name"),
            Self::ExpectedConstName => write!(f, "expected const name"),
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
            Self::ExpectedType => write!(f, "expected type path, array type, or tuple type"),
            Self::ExpectedBindingName => write!(f, "expected binding name"),
            Self::ExpectedAssignmentOperator => write!(f, "expected `=` in assignment"),
            Self::ExpectedFieldBinding => write!(f, "expected `val` or `var` before field name"),
            Self::ExpectedConstInitializer => write!(f, "expected `=` after const name"),
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
            Self::InvalidAssignmentTarget { reason } => {
                write!(f, "invalid assignment target: {reason}")
            }
            Self::AssignmentTypeMismatch { expected, found } => write!(
                f,
                "assignment type mismatch: expected `{expected}`, found `{found}`"
            ),
            Self::ConditionTypeMismatch { context, found } => write!(
                f,
                "{context} condition type mismatch: expected `bool`, found `{found}`"
            ),
            Self::BinaryOperandTypeMismatch {
                operator,
                expected,
                lhs,
                rhs,
            } => write!(
                f,
                "binary operator `{operator}` expects {expected} operands, found `{lhs}` and `{rhs}`"
            ),
            Self::UnaryOperandTypeMismatch {
                operator,
                expected,
                found,
            } => write!(
                f,
                "unary operator `{operator}` expects {expected} operand, found `{found}`"
            ),
            Self::ArrayElementTypeMismatch { expected, found } => write!(
                f,
                "array element type mismatch: expected `{expected}`, found `{found}`"
            ),
            Self::InvalidStructInitializer {
                struct_name,
                reason,
            } => write!(
                f,
                "invalid struct initializer for `{struct_name}`: {reason}"
            ),
            Self::UnknownTrait { trait_name } => write!(f, "unknown trait `{trait_name}`"),
            Self::InvalidTraitImpl {
                trait_name,
                type_name,
                reason,
            } => write!(
                f,
                "invalid impl of trait `{trait_name}` for `{type_name}`: {reason}"
            ),
            Self::InvalidInterfaceType { trait_name, reason } => write!(
                f,
                "trait `{trait_name}` cannot be used as an interface type: {reason}"
            ),
            Self::TraitMethodMismatch {
                trait_name,
                method_name,
                reason,
            } => write!(
                f,
                "trait method `{trait_name}.{method_name}` mismatch: {reason}"
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
            Self::LegacyLetBinding => {
                write!(f, "`let` bindings are not valid; use `val` or `var`")
            }
            Self::LegacyStaticItem => {
                write!(f, "`static` items are not part of the source language")
            }
            Self::LegacyRefParameter => write!(f, "`ref` parameters are not valid"),
            Self::LegacyReceiverModifier => {
                write!(f, "receiver modifiers are not valid; use plain `self`")
            }
            Self::LegacyDynTrait => {
                write!(f, "`dyn Trait` is not valid; use the trait name directly")
            }
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
