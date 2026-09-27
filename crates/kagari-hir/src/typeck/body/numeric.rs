use crate::builtin::surface;
use crate::hir::ExprId;
use crate::hir::Literal;
use crate::hir::LiteralKind;
use crate::typeck::ScalarValue;
use crate::typeck::body::BodyChecker;
use crate::types::BuiltinType;
use crate::types::TypeId;
use kagari_common::Diagnostic;
use kagari_common::DiagnosticKind;
use kagari_common::literal;

impl BodyChecker<'_> {
    pub(super) fn infer_numeric_literal(
        &mut self,
        site: ExprId,
        literal: &Literal,
        expected: Option<&TypeId>,
        negative: bool,
    ) -> TypeId {
        let (_, suffix) = literal::numeric_literal_parts(&literal.text);
        let suffix = suffix.and_then(surface::builtin_type);
        let fallback = if literal.kind == LiteralKind::Float {
            BuiltinType::F64
        } else {
            BuiltinType::I32
        };
        let ty = if let Some(suffix) = suffix {
            TypeId::Builtin(suffix)
        } else if self.body_inference {
            let variable = self.solver.numeric_variable(site, fallback);
            if let Some(expected) = expected {
                let _ = self.solver.constrain(&variable, expected, self.cancel);
            }
            self.solver.resolve(&variable)
        } else {
            expected.cloned().unwrap_or(TypeId::Builtin(fallback))
        };
        if self.solving && matches!(ty, TypeId::Inference(_)) {
            return ty;
        }
        let target = match ty {
            TypeId::Builtin(ty) => Some(ty),
            _ => None,
        };
        match ScalarValue::parse_expected(literal, target, negative) {
            Ok(value) => {
                let ty = value.ty();
                self.type_table.insert_scalar(site, value);
                ty
            }
            Err(reason) => {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidLiteral {
                        reason: reason.into(),
                    })
                    .with_span(self.lowered.source_map.expr_span(site)),
                );
                TypeId::Error
            }
        }
    }
}
