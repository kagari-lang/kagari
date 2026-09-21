//! Normal completion is separate from the type of a produced value. In particular,
//! a returning block does not produce Unit at the function's fallthrough boundary.
use kagari_common::cancellation::{CancellationToken, Cancelled};

use crate::hir::{BinaryOp, BlockId, ExprId, ExprKind, Module, PlaceId, PlaceKind, StmtKind};

#[derive(Clone, Copy, Default)]
struct Exits {
    normal: bool,
    breaks: bool,
}

impl Exits {
    const NORMAL: Self = Self {
        normal: true,
        breaks: false,
    };

    fn then(self, next: Self) -> Self {
        Self {
            normal: self.normal && next.normal,
            breaks: self.breaks || (self.normal && next.breaks),
        }
    }

    fn either(self, other: Self) -> Self {
        Self {
            normal: self.normal || other.normal,
            breaks: self.breaks || other.breaks,
        }
    }
}

pub(super) fn block_can_complete(
    module: &Module,
    block: BlockId,
    cancel: &CancellationToken,
) -> Result<bool, Cancelled> {
    Ok(Completion { module, cancel }.block(block)?.normal)
}

struct Completion<'a> {
    module: &'a Module,
    cancel: &'a CancellationToken,
}

pub(super) fn expr_can_complete(
    module: &Module,
    expr: ExprId,
    cancel: &CancellationToken,
) -> Result<bool, Cancelled> {
    Ok(Completion { module, cancel }.expr(expr)?.normal)
}

impl Completion<'_> {
    fn sequence(
        &self,
        initial: Exits,
        expressions: impl IntoIterator<Item = ExprId>,
    ) -> Result<Exits, Cancelled> {
        let mut exits = initial;
        for expression in expressions {
            self.cancel.check()?;
            if !exits.normal {
                break;
            }
            exits = exits.then(self.expr(expression)?);
        }
        Ok(exits)
    }

    fn block(&self, id: BlockId) -> Result<Exits, Cancelled> {
        self.cancel.check()?;
        let block = self.module.block(id);
        let mut exits = Exits::NORMAL;
        for statement in &block.statements {
            self.cancel.check()?;
            if !exits.normal {
                break;
            }
            let next = match &self.module.stmt(*statement).kind {
                StmtKind::Binding { initializer, .. } => self.expr(*initializer)?,
                StmtKind::Assign { target, value, .. } => {
                    self.sequence(self.place(*target)?, [*value])?
                }
                StmtKind::Return { expr } => {
                    let mut exits = self.sequence(Exits::NORMAL, expr.iter().copied())?;
                    exits.normal = false;
                    exits
                }
                StmtKind::Break => Exits {
                    normal: false,
                    breaks: true,
                },
                StmtKind::Continue => Exits::default(),
                StmtKind::Expr(expr) => self.expr(*expr)?,
                StmtKind::While { condition, .. } => self.expr(*condition)?,
                StmtKind::Loop { body } => Exits {
                    normal: self.block(*body)?.breaks,
                    breaks: false,
                },
            };
            exits = exits.then(next);
        }
        self.sequence(exits, block.tail_expr.iter().copied())
    }

    fn expr(&self, id: ExprId) -> Result<Exits, Cancelled> {
        self.cancel.check()?;
        Ok(match &self.module.expr(id).kind {
            ExprKind::Missing | ExprKind::Name(_) | ExprKind::Literal(_) => Exits::NORMAL,
            ExprKind::Prefix { expr, .. } => self.expr(*expr)?,
            ExprKind::Binary { lhs, op, rhs } => {
                let left = self.expr(*lhs)?;
                if !left.normal {
                    return Ok(left);
                }
                let right = self.expr(*rhs)?;
                left.then(if matches!(op, BinaryOp::AndAnd | BinaryOp::OrOr) {
                    right.either(Exits::NORMAL)
                } else {
                    right
                })
            }
            ExprKind::Call { callee, args } => {
                self.sequence(self.expr(*callee)?, args.iter().copied())?
            }
            ExprKind::Field { receiver, .. } => self.expr(*receiver)?,
            ExprKind::Index { receiver, index } => {
                self.sequence(self.expr(*receiver)?, [*index])?
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let condition = self.expr(*condition)?;
                if !condition.normal {
                    return Ok(condition);
                }
                let then_exits = self.block(*then_branch)?;
                let else_exits = match else_branch {
                    Some(expr) => self.expr(*expr)?,
                    None => Exits::NORMAL,
                };
                condition.then(then_exits.either(else_exits))
            }
            ExprKind::Match { scrutinee, arms } => {
                let scrutinee = self.expr(*scrutinee)?;
                if !scrutinee.normal {
                    return Ok(scrutinee);
                }
                let mut exits = Exits::default();
                for arm in arms {
                    self.cancel.check()?;
                    exits = exits.either(self.expr(arm.expr)?);
                    if self.module.pattern(arm.pattern).kind.is_irrefutable() {
                        break;
                    }
                }
                scrutinee.then(exits)
            }
            ExprKind::StructInit { fields, .. } => {
                self.sequence(Exits::NORMAL, fields.iter().map(|field| field.value))?
            }
            ExprKind::Tuple(elements) | ExprKind::Array(elements) => {
                self.sequence(Exits::NORMAL, elements.iter().copied())?
            }
            ExprKind::Block(block) => self.block(*block)?,
        })
    }

    fn place(&self, id: PlaceId) -> Result<Exits, Cancelled> {
        self.cancel.check()?;
        match &self.module.place(id).kind {
            PlaceKind::Name(_) => Ok(Exits::NORMAL),
            PlaceKind::Expr(expr) => self.expr(*expr),
            PlaceKind::Field { base, .. } => self.place(*base),
            PlaceKind::Index { base, index } => self.sequence(self.place(*base)?, [*index]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_is_not_a_completion_fact() {
        let lowered = crate::lower::lower_module(&kagari_common::SourceFile::new(
            "cancel.kgr",
            "fn empty() {} fn value() { 7 }",
        ));
        let token = CancellationToken::default();
        token.cancel();
        for function in &lowered.module.functions {
            assert_eq!(
                block_can_complete(&lowered.module, function.body, &token),
                Err(Cancelled)
            );
        }
        let expr = lowered
            .module
            .block(lowered.module.functions[1].body)
            .tail_expr
            .unwrap();
        assert_eq!(
            expr_can_complete(&lowered.module, expr, &token),
            Err(Cancelled)
        );
    }

    #[test]
    fn cancellation_during_a_sequence_stops_before_the_next_operand() {
        let lowered = crate::lower::lower_module(&kagari_common::SourceFile::new(
            "cancel.kgr",
            "fn main() { 7 }",
        ));
        let expr = lowered
            .module
            .block(lowered.module.functions[0].body)
            .tail_expr
            .unwrap();
        let token = CancellationToken::default();
        let mut visited = 0;
        let expressions = std::iter::from_fn(|| {
            visited += 1;
            assert_eq!(visited, 1, "cancelled traversal requested another operand");
            token.cancel();
            Some(expr)
        });
        let result = Completion {
            module: &lowered.module,
            cancel: &token,
        }
        .sequence(Exits::NORMAL, expressions);
        assert!(matches!(result, Err(Cancelled)));
        assert_eq!(visited, 1);
    }
}
