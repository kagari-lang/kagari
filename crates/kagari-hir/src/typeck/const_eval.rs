//! Scalar const facts belong to semantic analysis, never to a backend.
use std::collections::HashMap;

use kagari_common::arithmetic::{self, IntegerBinaryOp};
use kagari_common::{Diagnostic, DiagnosticKind, cancellation::CancellationToken};
use smallvec::SmallVec;

use crate::{
    hir::{BinaryOp, ConstId, ExprId, ExprKind, PrefixOp},
    lower::LoweredModule,
    resolver::{ResolvedName, ResolvedNames},
};

use super::{ScalarValue, TypeTable};

pub(super) fn evaluate_constants(
    lowered: &LoweredModule,
    names: &ResolvedNames,
    type_table: &TypeTable,
    cancel: &CancellationToken,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
    budget: &mut super::const_budget::ConstBudget,
) -> HashMap<ConstId, ScalarValue> {
    let mut evaluator = Evaluator {
        lowered,
        names,
        type_table,
        cancel,
        diagnostics,
        cache: HashMap::new(),
        budget,
    };
    for item in &lowered.module.consts {
        if evaluator.budget.exhausted || cancel.check().is_err() {
            break;
        }
        evaluator.constant(item.id);
    }
    evaluator
        .cache
        .into_iter()
        .filter_map(|(id, value)| value.map(|value| (id, value)))
        .collect()
}

struct Evaluator<'a> {
    lowered: &'a LoweredModule,
    names: &'a ResolvedNames,
    type_table: &'a TypeTable,
    cancel: &'a CancellationToken,
    diagnostics: &'a mut SmallVec<[Diagnostic; 4]>,
    // None also breaks cycles, which the const capability validator diagnoses.
    cache: HashMap<ConstId, Option<ScalarValue>>,
    budget: &'a mut super::const_budget::ConstBudget,
}

impl Evaluator<'_> {
    fn constant(&mut self, id: ConstId) -> Option<ScalarValue> {
        if let Some(value) = self.cache.get(&id) {
            return value.clone();
        }
        self.cancel.check().ok()?;
        self.cache.insert(id, None);
        let item = self.lowered.module.constant(id);
        let value = self.expression(id, item.initializer);
        self.cache.insert(id, value.clone());
        value
    }

    fn expression(&mut self, owner: ConstId, id: ExprId) -> Option<ScalarValue> {
        self.cancel.check().ok()?;
        if !self
            .budget
            .enter(self.lowered.source_map.expr_span(id), self.diagnostics)
        {
            return None;
        }
        let result = self.expression_inner(owner, id);
        self.budget.leave();
        result
    }

    fn expression_inner(&mut self, owner: ConstId, id: ExprId) -> Option<ScalarValue> {
        self.cancel.check().ok()?;
        if let Some(value) = self.type_table.scalar_value(id) {
            return Some(value.clone());
        }
        let value = match &self.lowered.module.expr(id).kind {
            ExprKind::Tuple(elements) if elements.is_empty() => Ok(ScalarValue::Unit),
            ExprKind::Literal(_) => return None, // Invalid literals were diagnosed during checking.
            ExprKind::Name { .. } => match self.names.expr_resolution(id)? {
                ResolvedName::Const(id) => return self.constant(id),
                _ => return None,
            },
            ExprKind::Prefix { op, expr } => match (op, self.expression(owner, *expr)?) {
                (PrefixOp::Neg, ScalarValue::I32(value)) => arithmetic::i32_neg(value)
                    .map(ScalarValue::I32)
                    .map_err(|error| error.message()),
                (PrefixOp::Neg, ScalarValue::F32(value)) => Ok(ScalarValue::F32(-value)),
                (PrefixOp::Not, ScalarValue::Bool(value)) => Ok(ScalarValue::Bool(!value)),
                _ => return None,
            },
            ExprKind::Binary { lhs, op, rhs } => {
                let lhs = self.expression(owner, *lhs)?;
                match (op, &lhs) {
                    (BinaryOp::AndAnd, ScalarValue::Bool(false)) => return Some(lhs),
                    (BinaryOp::OrOr, ScalarValue::Bool(true)) => return Some(lhs),
                    _ => {}
                }
                let rhs = self.expression(owner, *rhs)?;
                binary(*op, lhs, rhs)?
            }
            _ => return None,
        };
        value
            .map_err(|reason| {
                let item = self.lowered.module.constant(owner);
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidConstInitializer {
                        const_name: item.name.clone(),
                        reason: reason.to_owned(),
                    })
                    .with_span(self.lowered.source_map.expr_span(id)),
                );
            })
            .ok()
    }
}

fn binary(
    op: BinaryOp,
    lhs: ScalarValue,
    rhs: ScalarValue,
) -> Option<Result<ScalarValue, &'static str>> {
    use ScalarValue::*;
    let arithmetic_op = match op {
        BinaryOp::Add => Some(IntegerBinaryOp::Add),
        BinaryOp::Sub => Some(IntegerBinaryOp::Sub),
        BinaryOp::Mul => Some(IntegerBinaryOp::Mul),
        BinaryOp::Div => Some(IntegerBinaryOp::Div),
        BinaryOp::Rem => Some(IntegerBinaryOp::Rem),
        _ => None,
    };
    if let Some(op) = arithmetic_op {
        return Some(match (lhs, rhs) {
            (I32(lhs), I32(rhs)) => arithmetic::i32_binary(op, lhs, rhs)
                .map(I32)
                .map_err(|error| error.message()),
            (F32(lhs), F32(rhs)) => Ok(F32(match op {
                IntegerBinaryOp::Add => lhs + rhs,
                IntegerBinaryOp::Sub => lhs - rhs,
                IntegerBinaryOp::Mul => lhs * rhs,
                IntegerBinaryOp::Div => lhs / rhs,
                IntegerBinaryOp::Rem => lhs % rhs,
            })),
            _ => return None,
        });
    }
    macro_rules! compare {
        ($lhs:expr, $rhs:expr) => {
            match op {
                BinaryOp::Eq => $lhs == $rhs,
                BinaryOp::NotEq => $lhs != $rhs,
                BinaryOp::Lt => $lhs < $rhs,
                BinaryOp::Gt => $lhs > $rhs,
                BinaryOp::Le => $lhs <= $rhs,
                BinaryOp::Ge => $lhs >= $rhs,
                _ => return None,
            }
        };
    }
    let result = match (lhs, rhs) {
        (Unit, Unit) => match op {
            BinaryOp::Eq => true,
            BinaryOp::NotEq => false,
            _ => return None,
        },
        (I32(lhs), I32(rhs)) => compare!(lhs, rhs),
        (F32(lhs), F32(rhs)) => compare!(lhs, rhs),
        (String(lhs), String(rhs)) => compare!(lhs, rhs),
        (Bool(lhs), Bool(rhs)) => match op {
            BinaryOp::Eq => lhs == rhs,
            BinaryOp::NotEq => lhs != rhs,
            BinaryOp::AndAnd => lhs && rhs,
            BinaryOp::OrOr => lhs || rhs,
            _ => return None,
        },
        _ => return None,
    };
    Some(Ok(Bool(result)))
}
