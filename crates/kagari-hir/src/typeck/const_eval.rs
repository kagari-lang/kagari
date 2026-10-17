//! Scalar const facts belong to semantic analysis, never to a backend.

use super::const_budget::ConstBudget;
use crate::{
    hir::{
        expr::{
            ExprKind,
            ops::{BinaryOp, PrefixOp},
        },
        ids::{ConstId, ExprId},
    },
    lower::LoweredModule,
    resolver::resolved::{ResolvedName, ResolvedNames},
    typeck::{scalar::ScalarValue, table::TypeTable},
    types::TypeId,
};
use kagari_common::cancellation::CancellationToken;
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::{arithmetic::IntegerBinaryOp, integer, integer::IntegerOp, scalar::IntegerType};
use smallvec::SmallVec;
use std::collections::HashMap;

#[cfg(test)]
mod tests;

pub(super) fn evaluate_constants(
    lowered: &LoweredModule,
    names: &ResolvedNames,
    type_table: &TypeTable,
    cancel: &CancellationToken,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
    budget: &mut ConstBudget,
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
    budget: &'a mut ConstBudget,
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
            ExprKind::Cast { expr, .. } => {
                let TypeId::Builtin(target) = self.type_table.expr_type(id)? else {
                    return None;
                };
                return self.expression(owner, *expr)?.cast_numeric(target);
            }
            ExprKind::Prefix { op, expr } => match (op, self.expression(owner, *expr)?) {
                (PrefixOp::Neg, ScalarValue::F32(value)) => Ok(ScalarValue::F32(-value)),
                (PrefixOp::Neg, ScalarValue::F64(value)) => Ok(ScalarValue::F64(-value)),
                (PrefixOp::Neg, ScalarValue::Integer { value, ty }) => {
                    integer_value(IntegerOp::CheckedSub, 0, value, ty)
                }
                (PrefixOp::Not, ScalarValue::Bool(value)) => Ok(ScalarValue::Bool(!value)),
                (PrefixOp::Not, ScalarValue::Integer { value, ty }) => {
                    integer_value(IntegerOp::BitNot, value, 0, ty)
                }
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
    let bit = match op {
        BinaryOp::BitAnd => Some(IntegerOp::BitAnd),
        BinaryOp::BitOr => Some(IntegerOp::BitOr),
        BinaryOp::BitXor => Some(IntegerOp::BitXor),
        BinaryOp::Shl => Some(IntegerOp::Shl),
        BinaryOp::Shr => Some(IntegerOp::Shr),
        _ => None,
    };
    if let Some(op) = bit {
        return scalar_bits(op, lhs, rhs);
    }
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
            (ScalarValue::F32(lhs), ScalarValue::F32(rhs)) => Ok(ScalarValue::F32(match op {
                IntegerBinaryOp::Add => lhs + rhs,
                IntegerBinaryOp::Sub => lhs - rhs,
                IntegerBinaryOp::Mul => lhs * rhs,
                IntegerBinaryOp::Div => lhs / rhs,
                IntegerBinaryOp::Rem => lhs % rhs,
            })),
            (ScalarValue::F64(lhs), ScalarValue::F64(rhs)) => Ok(ScalarValue::F64(match op {
                IntegerBinaryOp::Add => lhs + rhs,
                IntegerBinaryOp::Sub => lhs - rhs,
                IntegerBinaryOp::Mul => lhs * rhs,
                IntegerBinaryOp::Div => lhs / rhs,
                IntegerBinaryOp::Rem => lhs % rhs,
            })),
            (
                ScalarValue::Integer { value: lhs, ty },
                ScalarValue::Integer {
                    value: rhs,
                    ty: right,
                },
            ) if ty == right => {
                let op = match op {
                    IntegerBinaryOp::Add => IntegerOp::CheckedAdd,
                    IntegerBinaryOp::Sub => IntegerOp::CheckedSub,
                    IntegerBinaryOp::Mul => IntegerOp::CheckedMul,
                    IntegerBinaryOp::Div => IntegerOp::CheckedDiv,
                    IntegerBinaryOp::Rem => IntegerOp::CheckedRem,
                };
                integer_value(op, lhs, rhs, ty)
            }
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
        (ScalarValue::Unit, ScalarValue::Unit) => match op {
            BinaryOp::Eq => true,
            BinaryOp::NotEq => false,
            _ => return None,
        },
        (ScalarValue::F32(lhs), ScalarValue::F32(rhs)) => compare!(lhs, rhs),
        (ScalarValue::F64(lhs), ScalarValue::F64(rhs)) => compare!(lhs, rhs),
        (
            ScalarValue::Integer { value: lhs, ty },
            ScalarValue::Integer {
                value: rhs,
                ty: right,
            },
        ) if ty == right => compare!(lhs, rhs),
        (ScalarValue::String(lhs), ScalarValue::String(rhs)) => compare!(lhs, rhs),
        (ScalarValue::Bool(lhs), ScalarValue::Bool(rhs)) => match op {
            BinaryOp::Eq => lhs == rhs,
            BinaryOp::NotEq => lhs != rhs,
            BinaryOp::AndAnd => lhs && rhs,
            BinaryOp::OrOr => lhs || rhs,
            _ => return None,
        },
        _ => return None,
    };
    Some(Ok(ScalarValue::Bool(result)))
}

fn scalar_bits(
    op: IntegerOp,
    lhs: ScalarValue,
    rhs: ScalarValue,
) -> Option<Result<ScalarValue, &'static str>> {
    let ScalarValue::Integer { value: lhs, ty } = lhs else {
        return None;
    };
    let ScalarValue::Integer { value: rhs, .. } = rhs else {
        return None;
    };
    Some(integer_value(op, lhs, rhs, ty))
}

fn integer_value(
    op: IntegerOp,
    lhs: i128,
    rhs: i128,
    ty: IntegerType,
) -> Result<ScalarValue, &'static str> {
    let (bits, signed) = ty.layout();
    integer::integer_operation(op, lhs, rhs, bits, signed)
        .and_then(|value| ScalarValue::integer(value, ty))
}
