//! Normal completion is separate from the type of a produced value. In particular,
//! a returning block does not produce Unit at the function's fallthrough boundary.
use kagari_common::cancellation::CancellationToken;

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
) -> bool {
    Completion { module, cancel }.block(block).normal
}

struct Completion<'a> {
    module: &'a Module,
    cancel: &'a CancellationToken,
}

pub(super) fn expr_can_complete(module: &Module, expr: ExprId, cancel: &CancellationToken) -> bool {
    Completion { module, cancel }.expr(expr).normal
}

impl Completion<'_> {
    fn block(&self, id: BlockId) -> Exits {
        let block = self.module.block(id);
        let mut exits = Exits::NORMAL;
        for statement in &block.statements {
            if !exits.normal || self.cancel.check().is_err() {
                return exits;
            }
            let next = match &self.module.stmt(*statement).kind {
                StmtKind::Binding { initializer, .. } => self.expr(*initializer),
                StmtKind::Assign { target, value, .. } => {
                    self.place(*target).then(self.expr(*value))
                }
                StmtKind::Return { expr } => {
                    let mut exits = expr.map_or(Exits::NORMAL, |expr| self.expr(expr));
                    exits.normal = false;
                    exits
                }
                StmtKind::Break => Exits {
                    normal: false,
                    breaks: true,
                },
                StmtKind::Continue => Exits::default(),
                StmtKind::Expr(expr) => self.expr(*expr),
                StmtKind::While { condition, .. } => self.expr(*condition),
                StmtKind::Loop { body } => Exits {
                    normal: self.block(*body).breaks,
                    breaks: false,
                },
            };
            exits = exits.then(next);
        }
        if let Some(tail) = block.tail_expr {
            exits = exits.then(self.expr(tail));
        }
        exits
    }

    fn expr(&self, id: ExprId) -> Exits {
        if self.cancel.check().is_err() {
            return Exits::NORMAL;
        }
        match &self.module.expr(id).kind {
            ExprKind::Missing | ExprKind::Name(_) | ExprKind::Literal(_) => Exits::NORMAL,
            ExprKind::Prefix { expr, .. } => self.expr(*expr),
            ExprKind::Binary { lhs, op, rhs } => {
                let right = self.expr(*rhs);
                self.expr(*lhs)
                    .then(if matches!(op, BinaryOp::AndAnd | BinaryOp::OrOr) {
                        right.either(Exits::NORMAL)
                    } else {
                        right
                    })
            }
            ExprKind::Call { callee, args } => args
                .iter()
                .fold(self.expr(*callee), |exits, arg| exits.then(self.expr(*arg))),
            ExprKind::Field { receiver, .. } => self.expr(*receiver),
            ExprKind::Index { receiver, index } => self.expr(*receiver).then(self.expr(*index)),
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.expr(*condition).then(
                self.block(*then_branch)
                    .either(else_branch.map_or(Exits::NORMAL, |expr| self.expr(expr))),
            ),
            ExprKind::Match { scrutinee, arms } => {
                let mut exits = Exits::default();
                for arm in arms {
                    exits = exits.either(self.expr(arm.expr));
                    if self.module.pattern(arm.pattern).kind.is_irrefutable() {
                        break;
                    }
                }
                self.expr(*scrutinee).then(exits)
            }
            ExprKind::StructInit { fields, .. } => {
                fields.iter().fold(Exits::NORMAL, |exits, field| {
                    exits.then(self.expr(field.value))
                })
            }
            ExprKind::Tuple(elements) | ExprKind::Array(elements) => elements
                .iter()
                .fold(Exits::NORMAL, |exits, expr| exits.then(self.expr(*expr))),
            ExprKind::Block(block) => self.block(*block),
        }
    }

    fn place(&self, id: PlaceId) -> Exits {
        if self.cancel.check().is_err() {
            return Exits::NORMAL;
        }
        match &self.module.place(id).kind {
            PlaceKind::Name(_) => Exits::NORMAL,
            PlaceKind::Expr(expr) => self.expr(*expr),
            PlaceKind::Field { base, .. } => self.place(*base),
            PlaceKind::Index { base, index } => self.place(*base).then(self.expr(*index)),
        }
    }
}
