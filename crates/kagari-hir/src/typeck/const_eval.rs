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
use kagari_contract::scalar::BuiltinType;
use std::collections::HashMap;
use {
    kagari_common::{
        arithmetic::{self, IntegerBinaryOp},
        cancellation::CancellationToken,
        integer::{self, IntegerOp},
    },
    kagari_source::diagnostic::{Diagnostic, DiagnosticKind},
};

use smallvec::SmallVec;

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
                (PrefixOp::Neg, ScalarValue::I32(value)) => arithmetic::i32_neg(value)
                    .map(ScalarValue::I32)
                    .map_err(|error| error.message()),
                (PrefixOp::Neg, ScalarValue::F32(value)) => Ok(ScalarValue::F32(-value)),
                (PrefixOp::Neg, ScalarValue::F64(value)) => Ok(ScalarValue::F64(-value)),
                (PrefixOp::Neg, ScalarValue::Integer { value, ty }) => {
                    ScalarValue::integer(-value, ty)
                }
                (PrefixOp::Not, ScalarValue::Bool(value)) => Ok(ScalarValue::Bool(!value)),
                (PrefixOp::Not, value) => {
                    scalar_bits(IntegerOp::BitNot, value, ScalarValue::I32(0))?
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
            (ScalarValue::I32(lhs), ScalarValue::I32(rhs)) => arithmetic::i32_binary(op, lhs, rhs)
                .map(ScalarValue::I32)
                .map_err(|error| error.message()),
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
                if matches!(op, IntegerBinaryOp::Rem)
                    && ty.integer_layout().is_some_and(|(bits, signed)| {
                        signed && lhs == integer::bounds(bits, signed).0 && rhs == -1
                    })
                {
                    return Some(Err("integer overflow"));
                }
                let value = match op {
                    IntegerBinaryOp::Add => lhs.checked_add(rhs),
                    IntegerBinaryOp::Sub => lhs.checked_sub(rhs),
                    IntegerBinaryOp::Mul => lhs.checked_mul(rhs),
                    IntegerBinaryOp::Div => lhs.checked_div(rhs),
                    IntegerBinaryOp::Rem => lhs.checked_rem(rhs),
                };
                value
                    .ok_or("integer overflow or division by zero")
                    .and_then(|value| ScalarValue::integer(value, ty))
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
        (ScalarValue::I32(lhs), ScalarValue::I32(rhs)) => compare!(lhs, rhs),
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
    let unpack = |v| match v {
        ScalarValue::I32(v) => Some((i128::from(v), BuiltinType::I32)),
        ScalarValue::Integer { value, ty } => Some((value, ty)),
        _ => None,
    };
    let (lhs, ty) = unpack(lhs)?;
    let (rhs, _) = unpack(rhs)?;
    let (bits, signed) = ty.integer_layout()?;
    Some(
        integer::integer_operation(op, lhs, rhs, bits, signed)
            .and_then(|v| ScalarValue::integer(v, ty)),
    )
}
