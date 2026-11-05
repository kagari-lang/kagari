//! Select only the evaluated operand prefix before requiring a parent's operation facts.

use crate::source::elaborate::{ElaborationError, build::Builder};
use kagari_hir::{
    hir::{
        expr::{ExprKind, ops::BinaryOp},
        ids::ExprId,
    },
    typeck::table::CallTarget,
};
use std::iter;

impl Builder<'_, '_> {
    pub(super) fn diverging_prefix(
        &mut self,
        id: ExprId,
        syntax: &ExprKind,
    ) -> Result<Option<Vec<ExprId>>, ElaborationError> {
        let span = self.input.expression_span(id);
        let facts = self.require(self.input.expression_facts(id), span, "foreign expression")?;
        let operands = if let Some(host) = facts.host_path {
            if host.root == id {
                return Err(self.error(span, "recursive host read root"));
            }
            iter::once(host.root)
                .chain(host.dynamic_arguments.iter().map(|(_, id)| *id))
                .collect()
        } else {
            match syntax {
                ExprKind::Call { callee, args, .. } => {
                    // A member selector is not a value, but its evaluation can exit.
                    if !self.completes(*callee)? {
                        self.expression(*callee)?;
                        return Ok(Some(vec![*callee]));
                    }
                    let receiver = facts.call.as_ref().and_then(|call| {
                        if matches!(call.target, CallTarget::TerminatingCallee) {
                            None
                        } else {
                            call.receiver
                        }
                    });
                    receiver.into_iter().chain(args.iter().copied()).collect()
                }
                ExprKind::Prefix { expr, .. }
                | ExprKind::Cast { expr, .. }
                | ExprKind::Await { expr }
                | ExprKind::Propagate { expr }
                | ExprKind::FormatPart { expr, .. }
                | ExprKind::Field { receiver: expr, .. } => vec![*expr],
                ExprKind::Binary {
                    lhs,
                    op: BinaryOp::AndAnd | BinaryOp::OrOr,
                    ..
                } => vec![*lhs],
                ExprKind::Binary { lhs, rhs, .. } => vec![*lhs, *rhs],
                ExprKind::Index { receiver, index } => vec![*receiver, *index],
                ExprKind::ArrayRepeat { value, count } => vec![*value, *count],
                ExprKind::Range { start, end, .. } => start.iter().chain(end).copied().collect(),
                ExprKind::If { condition, .. } => vec![condition.value()],
                ExprKind::Match { scrutinee, .. } => vec![*scrutinee],
                ExprKind::StructInit { fields, base, .. } => fields
                    .iter()
                    .map(|field| field.value)
                    .chain(base.iter().copied())
                    .collect(),
                ExprKind::InterpolatedString(elements)
                | ExprKind::Tuple(elements)
                | ExprKind::Array { elements, .. } => elements.to_vec(),
                // Structural branches and nested callable roots have separate evaluation boundaries.
                ExprKind::Missing
                | ExprKind::Name { .. }
                | ExprKind::Literal(_)
                | ExprKind::Block(_)
                | ExprKind::Loop { .. }
                | ExprKind::Closure { .. }
                | ExprKind::AsyncBlock { .. } => return Ok(None),
            }
        };
        for (index, operand) in operands.iter().copied().enumerate() {
            if !self.completes(operand)? {
                let prefix = operands[..=index].to_vec();
                for value in &prefix {
                    self.expression(*value)?;
                }
                return Ok(Some(prefix));
            }
        }
        Ok(None)
    }
}
