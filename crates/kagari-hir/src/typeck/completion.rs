//! Normal completion is separate from the type of a produced value. In particular,
//! a returning block does not produce Unit at the function's fallthrough boundary.
use std::collections::HashMap;

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

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Node {
    Expr(ExprId),
    Block(BlockId),
    Place(PlaceId),
    Stmt(crate::hir::StmtId),
    Branches(BlockId, Option<ExprId>),
    ShortCircuit(ExprId),
    Guarded(ExprId, ExprId),
    Normal,
}

type Nodes<'a> = Box<dyn Iterator<Item = Node> + 'a>;

enum Task<'a> {
    Record(Node),
    MatchArms(&'a [crate::hir::MatchArm]),
    Then(Exits),
    Visit(Node),
    Walk {
        nodes: Nodes<'a>,
        exits: Exits,
        alternatives: bool,
    },
    Resume {
        nodes: Nodes<'a>,
        exits: Exits,
        alternatives: bool,
    },
    Return,
    Break,
    Loop,
    ShortCircuit,
}

impl<'a> Completion<'a> {
    fn block(&self, id: BlockId) -> Result<Exits, Cancelled> {
        self.run(Task::Visit(Node::Block(id)))
    }

    fn expr(&self, id: ExprId) -> Result<Exits, Cancelled> {
        self.run(Task::Visit(Node::Expr(id)))
    }

    #[cfg(test)]
    fn sequence<'b>(
        &'b self,
        initial: Exits,
        expressions: impl IntoIterator<Item = ExprId> + 'b,
    ) -> Result<Exits, Cancelled> {
        self.run(Task::Walk {
            nodes: Box::new(expressions.into_iter().map(Node::Expr)),
            exits: initial,
            alternatives: false,
        })
    }

    fn run<'b>(&'b self, first: Task<'b>) -> Result<Exits, Cancelled> {
        let mut work = vec![first];
        // Facts belong to this immutable module traversal only. Cancellation
        // discards the whole cache; no partial result escapes into another query.
        let mut facts = HashMap::new();
        let mut value = Exits::NORMAL;
        while let Some(task) = work.pop() {
            self.cancel.check()?;
            match task {
                Task::Record(node) => {
                    facts.insert(node, value);
                }
                Task::Walk {
                    mut nodes,
                    exits,
                    alternatives,
                } => {
                    if !alternatives && !exits.normal {
                        value = exits;
                        continue;
                    }
                    if let Some(node) = nodes.next() {
                        // Iterators may obtain their next operand after cancellation.
                        self.cancel.check()?;
                        work.push(Task::Resume {
                            nodes,
                            exits,
                            alternatives,
                        });
                        work.push(Task::Visit(node));
                    } else {
                        value = exits;
                    }
                }
                Task::Resume {
                    nodes,
                    exits,
                    alternatives,
                } => {
                    work.push(Task::Walk {
                        nodes,
                        exits: if alternatives {
                            exits.either(value)
                        } else {
                            exits.then(value)
                        },
                        alternatives,
                    });
                }
                Task::Return => value.normal = false,
                Task::Break => {
                    value = Exits {
                        normal: false,
                        breaks: value.breaks || value.normal,
                    }
                }
                Task::Loop => {
                    value = Exits {
                        normal: value.breaks,
                        breaks: false,
                    }
                }
                Task::ShortCircuit => value = value.either(Exits::NORMAL),
                Task::Visit(node) => {
                    if let Some(cached) = facts.get(&node) {
                        value = *cached;
                        continue;
                    }
                    work.push(Task::Record(node));
                    let nodes: Nodes<'b> = match node {
                        Node::Normal => {
                            value = Exits::NORMAL;
                            continue;
                        }
                        Node::ShortCircuit(expr) => {
                            work.push(Task::ShortCircuit);
                            work.push(Task::Visit(Node::Expr(expr)));
                            continue;
                        }
                        Node::Guarded(guard, expr) => {
                            Box::new([Node::Expr(guard), Node::ShortCircuit(expr)].into_iter())
                        }
                        Node::Branches(then, otherwise) => {
                            work.push(Task::Walk {
                                nodes: Box::new(
                                    [
                                        Node::Block(then),
                                        otherwise.map_or(Node::Normal, Node::Expr),
                                    ]
                                    .into_iter(),
                                ),
                                exits: Exits::default(),
                                alternatives: true,
                            });
                            continue;
                        }
                        Node::Block(id) => {
                            let block = self.module.block(id);
                            Box::new(
                                block
                                    .statements
                                    .iter()
                                    .copied()
                                    .map(Node::Stmt)
                                    .chain(block.tail_expr.iter().copied().map(Node::Expr)),
                            )
                        }
                        Node::Stmt(id) => match &self.module.stmt(id).kind {
                            StmtKind::Binding { initializer, .. } | StmtKind::Expr(initializer) => {
                                work.push(Task::Visit(Node::Expr(*initializer)));
                                continue;
                            }
                            StmtKind::Assign { target, value, .. } => {
                                Box::new([Node::Place(*target), Node::Expr(*value)].into_iter())
                            }
                            StmtKind::Return { expr } => {
                                work.push(Task::Return);
                                work.push(Task::Visit(expr.map_or(Node::Normal, Node::Expr)));
                                continue;
                            }
                            StmtKind::Break => {
                                value = Exits {
                                    normal: false,
                                    breaks: true,
                                };
                                continue;
                            }
                            StmtKind::BreakValue(expr) => {
                                work.push(Task::Break);
                                work.push(Task::Visit(Node::Expr(*expr)));
                                continue;
                            }
                            StmtKind::Continue => {
                                value = Exits::default();
                                continue;
                            }
                            StmtKind::While { condition, .. } => {
                                work.push(Task::Visit(Node::Expr(condition.value())));
                                continue;
                            }
                            StmtKind::Loop { body } => {
                                work.push(Task::Loop);
                                work.push(Task::Visit(Node::Block(*body)));
                                continue;
                            }
                            StmtKind::For { iterable, .. } => {
                                work.push(Task::Visit(Node::Expr(*iterable)));
                                continue;
                            }
                        },
                        Node::Place(id) => match &self.module.place(id).kind {
                            PlaceKind::Name(_) => {
                                value = Exits::NORMAL;
                                continue;
                            }
                            PlaceKind::Expr(expr) => {
                                work.push(Task::Visit(Node::Expr(*expr)));
                                continue;
                            }
                            PlaceKind::Field { base, .. } => {
                                work.push(Task::Visit(Node::Place(*base)));
                                continue;
                            }
                            PlaceKind::Index { base, index } => {
                                Box::new([Node::Place(*base), Node::Expr(*index)].into_iter())
                            }
                        },
                        Node::Expr(id) => match &self.module.expr(id).kind {
                            ExprKind::Missing
                            | ExprKind::Name { .. }
                            | ExprKind::Literal(_)
                            | ExprKind::Closure { .. } => {
                                value = Exits::NORMAL;
                                continue;
                            }
                            ExprKind::Propagate { expr }
                            | ExprKind::Prefix { expr, .. }
                            | ExprKind::Field { receiver: expr, .. } => {
                                work.push(Task::Visit(Node::Expr(*expr)));
                                continue;
                            }
                            ExprKind::Binary { lhs, op, rhs } => Box::new(
                                [
                                    Node::Expr(*lhs),
                                    if matches!(op, BinaryOp::AndAnd | BinaryOp::OrOr) {
                                        Node::ShortCircuit(*rhs)
                                    } else {
                                        Node::Expr(*rhs)
                                    },
                                ]
                                .into_iter(),
                            ),
                            ExprKind::Range { start, end, .. } => {
                                Box::new([Node::Expr(*start), Node::Expr(*end)].into_iter())
                            }
                            ExprKind::Call { callee, args } => Box::new(
                                std::iter::once(Node::Expr(*callee))
                                    .chain(args.iter().copied().map(Node::Expr)),
                            ),
                            ExprKind::Index { receiver, index } => {
                                Box::new([Node::Expr(*receiver), Node::Expr(*index)].into_iter())
                            }
                            ExprKind::If {
                                condition,
                                then_branch,
                                else_branch,
                            } => Box::new(
                                [
                                    Node::Expr(condition.value()),
                                    Node::Branches(*then_branch, *else_branch),
                                ]
                                .into_iter(),
                            ),
                            ExprKind::Match { scrutinee, arms } => {
                                // A sequential scrutinee precedes an alternative arm walk.
                                work.push(Task::MatchArms(arms));
                                work.push(Task::Visit(Node::Expr(*scrutinee)));
                                continue;
                            }
                            ExprKind::StructInit { fields, .. } => {
                                Box::new(fields.iter().map(|field| Node::Expr(field.value)))
                            }
                            ExprKind::Tuple(elements) | ExprKind::Array(elements) => {
                                Box::new(elements.iter().copied().map(Node::Expr))
                            }
                            ExprKind::Block(block) => {
                                work.push(Task::Visit(Node::Block(*block)));
                                continue;
                            }
                            ExprKind::Loop { body } => {
                                work.push(Task::Loop);
                                work.push(Task::Visit(Node::Block(*body)));
                                continue;
                            }
                        },
                    };
                    work.push(Task::Walk {
                        nodes,
                        exits: Exits::NORMAL,
                        alternatives: false,
                    });
                }
                Task::MatchArms(arms) => {
                    if value.normal {
                        work.push(Task::Then(value));
                        let nodes = arms.iter().scan(false, |stopped, arm| {
                            if *stopped {
                                return None;
                            }
                            *stopped = arm.guard.is_none()
                                && self.module.pattern_is_irrefutable(arm.pattern);
                            Some(match arm.guard {
                                Some(guard) => Node::Guarded(guard, arm.expr),
                                None => Node::Expr(arm.expr),
                            })
                        });
                        work.push(Task::Walk {
                            nodes: Box::new(nodes),
                            exits: Exits::default(),
                            alternatives: true,
                        });
                    }
                }
                Task::Then(previous) => value = previous.then(value),
            }
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deeply_nested_expressions_blocks_and_places_use_the_work_stack() {
        use crate::hir::{BlockData, ExprData, PlaceData, PrefixOp};
        for (source, expected) in [
            ("fn main() { 7 }", true),
            ("fn main() { if true { return; } else { return; } }", false),
        ] {
            let mut lowered = crate::lower::lower_module(&kagari_common::SourceFile::new(
                "deep-completion.kgr",
                source,
            ));
            let module = &mut lowered.module;
            let mut expr = module.block(module.functions[0].body).tail_expr.unwrap();
            let arena = expr.arena();
            let owner = expr.owner();
            for depth in 0..20_000 {
                let kind = match depth % 3 {
                    0 => ExprKind::Prefix {
                        op: PrefixOp::Neg,
                        expr,
                    },
                    1 => ExprKind::Tuple(smallvec::smallvec![expr]),
                    _ => {
                        let block = BlockId::new(arena, owner, module.body.blocks.len());
                        module.body.blocks.push((
                            owner,
                            BlockData {
                                statements: Default::default(),
                                tail_expr: Some(expr),
                            },
                        ));
                        ExprKind::Block(block)
                    }
                };
                expr = ExprId::new(arena, owner, module.body.exprs.len());
                module.body.exprs.push((owner, ExprData { kind }));
            }
            let token = CancellationToken::default();
            assert_eq!(expr_can_complete(module, expr, &token), Ok(expected));

            let mut place = PlaceId::new(arena, owner, module.body.places.len());
            module.body.places.push((
                owner,
                PlaceData {
                    kind: PlaceKind::Expr(expr),
                },
            ));
            for _ in 0..20_000 {
                let base = place;
                place = PlaceId::new(arena, owner, module.body.places.len());
                module.body.places.push((
                    owner,
                    PlaceData {
                        kind: PlaceKind::Field {
                            base,
                            name: "field".into(),
                        },
                    },
                ));
            }
            assert_eq!(
                Completion {
                    module,
                    cancel: &token
                }
                .run(Task::Visit(Node::Place(place)))
                .unwrap()
                .normal,
                expected
            );
        }
    }

    #[test]
    fn a_terminated_sequence_never_requests_a_later_operand() {
        let lowered = crate::lower::lower_module(&kagari_common::SourceFile::new(
            "stopped-completion.kgr",
            "fn main() { if true { return; } else { return; } }",
        ));
        let expr = lowered
            .module
            .block(lowered.module.functions[0].body)
            .tail_expr
            .unwrap();
        let mut visits = 0;
        let expressions = std::iter::from_fn(|| {
            visits += 1;
            assert_eq!(visits, 1, "requested operand after termination");
            Some(expr)
        });
        let token = CancellationToken::default();
        let result = Completion {
            module: &lowered.module,
            cancel: &token,
        }
        .sequence(Exits::NORMAL, expressions)
        .unwrap();
        assert!(!result.normal);
        assert_eq!(visits, 1);
    }

    #[test]
    fn shared_subtrees_are_reused_without_losing_loop_exit_context() {
        use crate::hir::{BlockData, ExprData, StmtData};
        let mut lowered = crate::lower::lower_module(&kagari_common::SourceFile::new(
            "shared-completion.kgr",
            "fn main() { 7 }",
        ));
        let module = &mut lowered.module;
        let mut expr = module.block(module.functions[0].body).tail_expr.unwrap();
        let arena = expr.arena();
        let owner = expr.owner();
        // Expanding these shared edges instead of caching node facts would
        // require 2^48 leaf visits, despite only 49 expression nodes.
        for _ in 0..48 {
            let previous = expr;
            expr = ExprId::new(arena, owner, module.body.exprs.len());
            module.body.exprs.push((
                owner,
                ExprData {
                    kind: ExprKind::Tuple(smallvec::smallvec![previous, previous]),
                },
            ));
        }
        let token = CancellationToken::default();
        assert_eq!(expr_can_complete(module, expr, &token), Ok(true));

        let breaking = crate::hir::StmtId::new(arena, owner, module.body.stmts.len());
        module.body.stmts.push((
            owner,
            StmtData {
                kind: StmtKind::Break,
            },
        ));
        let body = BlockId::new(arena, owner, module.body.blocks.len());
        module.body.blocks.push((
            owner,
            BlockData {
                statements: smallvec::smallvec![breaking],
                tail_expr: None,
            },
        ));
        let looping = crate::hir::StmtId::new(arena, owner, module.body.stmts.len());
        module.body.stmts.push((
            owner,
            StmtData {
                kind: StmtKind::Loop { body },
            },
        ));
        // The same block's break exits are consumed by a loop, but propagate
        // when that block is visited directly, including after a cache hit.
        let nodes = [Node::Stmt(looping), Node::Block(body)].into_iter();
        let exits = Completion {
            module,
            cancel: &token,
        }
        .run(Task::Walk {
            nodes: Box::new(nodes),
            exits: Exits::NORMAL,
            alternatives: false,
        })
        .unwrap();
        assert!(!exits.normal);
        assert!(exits.breaks);
    }

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
