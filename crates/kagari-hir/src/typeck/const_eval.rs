//! Scalar const facts belong to semantic analysis, never to a backend.
use std::collections::HashMap;

use kagari_common::arithmetic::{self, IntegerBinaryOp};
use kagari_common::{Diagnostic, DiagnosticKind, cancellation::CancellationToken};
use smallvec::SmallVec;

use crate::{
    hir::{BinaryOp, ConstId, ExprId, ExprKind, LiteralKind, PrefixOp},
    lower::LoweredModule,
    resolver::{ResolvedName, ResolvedNames},
};

#[derive(Debug, Clone, PartialEq)]
pub enum ConstValue {
    Unit,
    Bool(bool),
    I32(i32),
    F32(f32),
    String(String),
}

pub(super) fn evaluate_constants(
    lowered: &LoweredModule,
    names: &ResolvedNames,
    cancel: &CancellationToken,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
) -> HashMap<ConstId, ConstValue> {
    let mut evaluator = Evaluator {
        lowered,
        names,
        cancel,
        diagnostics,
        cache: HashMap::new(),
    };
    for item in &lowered.module.consts {
        if cancel.check().is_err() {
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
    cancel: &'a CancellationToken,
    diagnostics: &'a mut SmallVec<[Diagnostic; 4]>,
    // None also breaks cycles, which the const capability validator diagnoses.
    cache: HashMap<ConstId, Option<ConstValue>>,
}

impl Evaluator<'_> {
    fn constant(&mut self, id: ConstId) -> Option<ConstValue> {
        if let Some(value) = self.cache.get(&id) {
            return value.clone();
        }
        self.cancel.check().ok()?;
        self.cache.insert(id, None);
        let item = self
            .lowered
            .module
            .consts
            .iter()
            .find(|item| item.id == id)?;
        let value = self.expression(id, item.initializer);
        self.cache.insert(id, value.clone());
        value
    }

    fn expression(&mut self, owner: ConstId, id: ExprId) -> Option<ConstValue> {
        self.cancel.check().ok()?;
        let value = match &self.lowered.module.expr(id).kind {
            ExprKind::Tuple(elements) if elements.is_empty() => Ok(ConstValue::Unit),
            ExprKind::Literal(literal) => match literal.kind {
                LiteralKind::Number => literal
                    .text
                    .parse()
                    .map(ConstValue::I32)
                    .map_err(|_| "integer literal is outside the i32 range"),
                LiteralKind::Float => literal
                    .text
                    .parse()
                    .map(ConstValue::F32)
                    .map_err(|_| "invalid f32 literal"),
                LiteralKind::Bool => Ok(ConstValue::Bool(literal.text == "true")),
                LiteralKind::String => Ok(ConstValue::String(
                    literal
                        .text
                        .strip_prefix('"')
                        .and_then(|text| text.strip_suffix('"'))
                        .unwrap_or(&literal.text)
                        .to_owned(),
                )),
            },
            ExprKind::Name(_) => match self.names.expr_resolution(id)? {
                ResolvedName::Const(id) => return self.constant(id),
                _ => return None,
            },
            ExprKind::Prefix { op, expr } => match (op, self.expression(owner, *expr)?) {
                (PrefixOp::Neg, ConstValue::I32(value)) => arithmetic::i32_neg(value)
                    .map(ConstValue::I32)
                    .map_err(|error| error.message()),
                (PrefixOp::Neg, ConstValue::F32(value)) => Ok(ConstValue::F32(-value)),
                (PrefixOp::Not, ConstValue::Bool(value)) => Ok(ConstValue::Bool(!value)),
                _ => return None,
            },
            ExprKind::Binary { lhs, op, rhs } => {
                let lhs = self.expression(owner, *lhs)?;
                match (op, &lhs) {
                    (BinaryOp::AndAnd, ConstValue::Bool(false)) => return Some(lhs),
                    (BinaryOp::OrOr, ConstValue::Bool(true)) => return Some(lhs),
                    _ => {}
                }
                let rhs = self.expression(owner, *rhs)?;
                binary(*op, lhs, rhs)?
            }
            _ => return None,
        };
        value
            .map_err(|reason| {
                let item = self
                    .lowered
                    .module
                    .consts
                    .iter()
                    .find(|item| item.id == owner)
                    .unwrap();
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
    lhs: ConstValue,
    rhs: ConstValue,
) -> Option<Result<ConstValue, &'static str>> {
    use ConstValue::*;
    let arithmetic_op = match op {
        BinaryOp::Add => Some(IntegerBinaryOp::Add),
        BinaryOp::Sub => Some(IntegerBinaryOp::Sub),
        BinaryOp::Mul => Some(IntegerBinaryOp::Mul),
        BinaryOp::Div => Some(IntegerBinaryOp::Div),
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
