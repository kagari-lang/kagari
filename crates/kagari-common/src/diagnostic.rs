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
    StandardConstraintNotSatisfied {
        type_name: String,
        constraint: String,
        reason: String,
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
    ProfileFeatureDisabled {
        feature: &'static str,
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

impl DiagnosticKind {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnexpectedToken => "KG_PARSE_UNEXPECTED_TOKEN",
            Self::ExpectedTopLevelItem => "KG_PARSE_EXPECTED_TOP_LEVEL_ITEM",
            Self::TopLevelControlFlowNotAllowed => "KG_PARSE_TOP_LEVEL_CONTROL_FLOW",
            Self::ExpectedModuleKeyword => "KG_PARSE_EXPECTED_MODULE_KEYWORD",
            Self::ExpectedModuleName => "KG_PARSE_EXPECTED_MODULE_NAME",
            Self::ExpectedModuleBodyStart => "KG_PARSE_EXPECTED_MODULE_BODY_START",
            Self::ExpectedUseKeyword => "KG_PARSE_EXPECTED_USE_KEYWORD",
            Self::ExpectedUseTree => "KG_PARSE_EXPECTED_USE_TREE",
            Self::ExpectedUseAlias => "KG_PARSE_EXPECTED_USE_ALIAS",
            Self::ExpectedPath => "KG_PARSE_EXPECTED_PATH",
            Self::ExpectedTraitKeyword => "KG_PARSE_EXPECTED_TRAIT_KEYWORD",
            Self::ExpectedTraitName => "KG_PARSE_EXPECTED_TRAIT_NAME",
            Self::ExpectedImplKeyword => "KG_PARSE_EXPECTED_IMPL_KEYWORD",
            Self::ExpectedImplBodyStart => "KG_PARSE_EXPECTED_IMPL_BODY_START",
            Self::ExpectedForKeyword => "KG_PARSE_EXPECTED_FOR_KEYWORD",
            Self::ExpectedGenericParameterName => "KG_PARSE_EXPECTED_GENERIC_PARAMETER_NAME",
            Self::ExpectedWherePredicateSeparator => "KG_PARSE_EXPECTED_WHERE_PREDICATE_SEPARATOR",
            Self::ExpectedFunctionKeyword => "KG_PARSE_EXPECTED_FUNCTION_KEYWORD",
            Self::ExpectedConstKeyword => "KG_PARSE_EXPECTED_CONST_KEYWORD",
            Self::ExpectedStructKeyword => "KG_PARSE_EXPECTED_STRUCT_KEYWORD",
            Self::ExpectedEnumKeyword => "KG_PARSE_EXPECTED_ENUM_KEYWORD",
            Self::ExpectedTraitBodyStart => "KG_PARSE_EXPECTED_TRAIT_BODY_START",
            Self::ExpectedBindingKeyword => "KG_PARSE_EXPECTED_BINDING_KEYWORD",
            Self::ExpectedReturnKeyword => "KG_PARSE_EXPECTED_RETURN_KEYWORD",
            Self::ExpectedBreakKeyword => "KG_PARSE_EXPECTED_BREAK_KEYWORD",
            Self::ExpectedContinueKeyword => "KG_PARSE_EXPECTED_CONTINUE_KEYWORD",
            Self::ExpectedFunctionName => "KG_PARSE_EXPECTED_FUNCTION_NAME",
            Self::ExpectedConstName => "KG_PARSE_EXPECTED_CONST_NAME",
            Self::ExpectedStructName => "KG_PARSE_EXPECTED_STRUCT_NAME",
            Self::ExpectedEnumName => "KG_PARSE_EXPECTED_ENUM_NAME",
            Self::ExpectedFieldName => "KG_PARSE_EXPECTED_FIELD_NAME",
            Self::ExpectedVariantName => "KG_PARSE_EXPECTED_VARIANT_NAME",
            Self::ExpectedIfKeyword => "KG_PARSE_EXPECTED_IF_KEYWORD",
            Self::ExpectedMatchKeyword => "KG_PARSE_EXPECTED_MATCH_KEYWORD",
            Self::ExpectedElseBranch => "KG_PARSE_EXPECTED_ELSE_BRANCH",
            Self::ExpectedWhileKeyword => "KG_PARSE_EXPECTED_WHILE_KEYWORD",
            Self::ExpectedLoopKeyword => "KG_PARSE_EXPECTED_LOOP_KEYWORD",
            Self::ExpectedFunctionParameterListStart => {
                "KG_PARSE_EXPECTED_FUNCTION_PARAMETER_LIST_START"
            }
            Self::ExpectedFunctionParameterListEnd => {
                "KG_PARSE_EXPECTED_FUNCTION_PARAMETER_LIST_END"
            }
            Self::ExpectedClosingParen => "KG_PARSE_EXPECTED_CLOSING_PAREN",
            Self::ExpectedClosingBracket => "KG_PARSE_EXPECTED_CLOSING_BRACKET",
            Self::ExpectedParameterName => "KG_PARSE_EXPECTED_PARAMETER_NAME",
            Self::ExpectedParameterTypeSeparator => "KG_PARSE_EXPECTED_PARAMETER_TYPE_SEPARATOR",
            Self::ExpectedFieldTypeSeparator => "KG_PARSE_EXPECTED_FIELD_TYPE_SEPARATOR",
            Self::ExpectedStructLiteralBodyStart => "KG_PARSE_EXPECTED_STRUCT_LITERAL_BODY_START",
            Self::ExpectedMatchBodyStart => "KG_PARSE_EXPECTED_MATCH_BODY_START",
            Self::ExpectedMatchPattern => "KG_PARSE_EXPECTED_MATCH_PATTERN",
            Self::ExpectedMatchArmArrow => "KG_PARSE_EXPECTED_MATCH_ARM_ARROW",
            Self::ExpectedType => "KG_PARSE_EXPECTED_TYPE",
            Self::ExpectedBindingName => "KG_PARSE_EXPECTED_BINDING_NAME",
            Self::ExpectedAssignmentOperator => "KG_PARSE_EXPECTED_ASSIGNMENT_OPERATOR",
            Self::ExpectedFieldBinding => "KG_PARSE_EXPECTED_FIELD_BINDING",
            Self::ExpectedConstInitializer => "KG_PARSE_EXPECTED_CONST_INITIALIZER",
            Self::ExpectedFunctionBodyStart => "KG_PARSE_EXPECTED_FUNCTION_BODY_START",
            Self::ExpectedStructBodyStart => "KG_PARSE_EXPECTED_STRUCT_BODY_START",
            Self::ExpectedBlockEnd => "KG_PARSE_EXPECTED_BLOCK_END",
            Self::ExpectedStatementTerminator => "KG_PARSE_EXPECTED_STATEMENT_TERMINATOR",
            Self::ExpectedExpression => "KG_PARSE_EXPECTED_EXPRESSION",
            Self::MissingFunctionName => "KG_RESOLVE_MISSING_FUNCTION_NAME",
            Self::DuplicateFunction { .. } => "KG_RESOLVE_DUPLICATE_FUNCTION",
            Self::UnknownType { .. } => "KG_TYPE_UNKNOWN_TYPE",
            Self::UnknownConstType { .. } => "KG_TYPE_UNKNOWN_CONST_TYPE",
            Self::InvalidConstInitializer { .. } => "KG_TYPE_INVALID_CONST_INITIALIZER",
            Self::ConstCycle { .. } => "KG_TYPE_CONST_CYCLE",
            Self::ConstWriteNotAllowed { .. } => "KG_TYPE_CONST_WRITE_NOT_ALLOWED",
            Self::CallArityMismatch { .. } => "KG_TYPE_CALL_ARITY_MISMATCH",
            Self::ArgumentTypeMismatch { .. } => "KG_TYPE_ARGUMENT_TYPE_MISMATCH",
            Self::StandardConstraintNotSatisfied { .. } => {
                "KG_TYPE_STANDARD_CONSTRAINT_NOT_SATISFIED"
            }
            Self::InvalidAssignmentTarget { .. } => "KG_TYPE_INVALID_ASSIGNMENT_TARGET",
            Self::AssignmentTypeMismatch { .. } => "KG_TYPE_ASSIGNMENT_TYPE_MISMATCH",
            Self::ConditionTypeMismatch { .. } => "KG_TYPE_CONDITION_TYPE_MISMATCH",
            Self::BinaryOperandTypeMismatch { .. } => "KG_TYPE_BINARY_OPERAND_TYPE_MISMATCH",
            Self::UnaryOperandTypeMismatch { .. } => "KG_TYPE_UNARY_OPERAND_TYPE_MISMATCH",
            Self::ArrayElementTypeMismatch { .. } => "KG_TYPE_ARRAY_ELEMENT_TYPE_MISMATCH",
            Self::InvalidStructInitializer { .. } => "KG_TYPE_INVALID_STRUCT_INITIALIZER",
            Self::UnknownTrait { .. } => "KG_TYPE_UNKNOWN_TRAIT",
            Self::InvalidTraitImpl { .. } => "KG_TYPE_INVALID_TRAIT_IMPL",
            Self::InvalidInterfaceType { .. } => "KG_TYPE_INVALID_INTERFACE_TYPE",
            Self::TraitMethodMismatch { .. } => "KG_TYPE_TRAIT_METHOD_MISMATCH",
            Self::IfBranchTypeMismatch { .. } => "KG_TYPE_IF_BRANCH_TYPE_MISMATCH",
            Self::MatchArmTypeMismatch { .. } => "KG_TYPE_MATCH_ARM_TYPE_MISMATCH",
            Self::ReturnTypeMismatch { .. } => "KG_TYPE_RETURN_TYPE_MISMATCH",
            Self::BreakOutsideLoop => "KG_TYPE_BREAK_OUTSIDE_LOOP",
            Self::ContinueOutsideLoop => "KG_TYPE_CONTINUE_OUTSIDE_LOOP",
            Self::ProfileFeatureDisabled { .. } => "KG_PROFILE_FEATURE_DISABLED",
        }
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
            Self::StandardConstraintNotSatisfied {
                type_name,
                constraint,
                reason,
            } => write!(
                f,
                "type `{type_name}` does not satisfy standard constraint `{constraint}`: {reason}"
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
            Self::ProfileFeatureDisabled { feature } => {
                write!(f, "language profile disables {feature}")
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

#[cfg(test)]
mod tests {
    use super::{DiagnosticKind, TypePosition};

    #[test]
    fn diagnostic_kinds_expose_stable_codes() {
        assert_eq!(
            DiagnosticKind::ExpectedBindingKeyword.code(),
            "KG_PARSE_EXPECTED_BINDING_KEYWORD"
        );
        assert_eq!(
            DiagnosticKind::DuplicateFunction {
                name: "main".to_owned()
            }
            .code(),
            "KG_RESOLVE_DUPLICATE_FUNCTION"
        );
        assert_eq!(
            DiagnosticKind::UnknownType {
                type_name: "Missing".to_owned(),
                function_name: "main".to_owned(),
                position: TypePosition::Return,
            }
            .code(),
            "KG_TYPE_UNKNOWN_TYPE"
        );
    }
}
