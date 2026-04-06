use kagari_common::{Diagnostic, DiagnosticKind};
use smallvec::SmallVec;

use crate::{
    hir::{
        BinaryOp, BlockId, ExprId, ExprKind, LiteralKind, PlaceId, PlaceKind, PrefixOp, StmtKind,
    },
    lower::LoweredModule,
    resolver::{ResolvedName, ResolvedNames},
    typeck::ty::display_type_id,
    typeck::{BodyTypeEnv, FunctionTypeIndex, TypeTable},
    types::{BuiltinType, TypeId},
};

pub(crate) struct BodyChecker<'a> {
    lowered: &'a LoweredModule,
    names: &'a ResolvedNames,
    function_index: &'a FunctionTypeIndex,
    diagnostics: &'a mut SmallVec<[Diagnostic; 4]>,
    type_table: &'a mut TypeTable,
    function_name: &'a str,
    expected_return: TypeId,
    loop_depth: usize,
}

impl<'a> BodyChecker<'a> {
    pub(crate) fn new(
        lowered: &'a LoweredModule,
        names: &'a ResolvedNames,
        function_index: &'a FunctionTypeIndex,
        diagnostics: &'a mut SmallVec<[Diagnostic; 4]>,
        type_table: &'a mut TypeTable,
        function_name: &'a str,
        expected_return: TypeId,
    ) -> Self {
        Self {
            lowered,
            names,
            function_index,
            diagnostics,
            type_table,
            function_name,
            expected_return,
            loop_depth: 0,
        }
    }

    pub(crate) fn infer_block_types(&mut self, block_id: BlockId, env: &mut BodyTypeEnv) -> TypeId {
        let block = self.lowered.module.block(block_id);
        for stmt in &block.statements {
            self.check_stmt(*stmt, env);
        }

        block
            .tail_expr
            .map_or(TypeId::Builtin(BuiltinType::Unit), |expr| {
                self.infer_expr_type(expr, env)
            })
    }

    fn check_stmt(&mut self, stmt_id: crate::hir::StmtId, env: &mut BodyTypeEnv) {
        let stmt = self.lowered.module.stmt(stmt_id);
        match &stmt.kind {
            StmtKind::Let {
                local,
                ty,
                initializer,
                ..
            } => {
                let initializer_ty = self.infer_expr_type(*initializer, env);
                let local_ty = ty
                    .and_then(|ty| crate::typeck::ty::resolve_type(&self.lowered.module, ty))
                    .unwrap_or(initializer_ty);
                env.locals.insert(*local, local_ty);
            }
            StmtKind::Assign { target, value } => {
                let value_ty = self.infer_expr_type(*value, env);
                match self.resolve_place_type(*target, env) {
                    Some(expected) if expected != value_ty => self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::AssignmentTypeMismatch {
                            expected: display_type_id(expected).to_string(),
                            found: display_type_id(value_ty).to_string(),
                        })
                        .with_span(self.lowered.source_map.place_span(*target)),
                    ),
                    None => self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidAssignmentTarget)
                            .with_span(self.lowered.source_map.place_span(*target)),
                    ),
                    _ => {}
                }
            }
            StmtKind::Return { expr } => {
                let found = expr.map_or(TypeId::Builtin(BuiltinType::Unit), |expr| {
                    self.infer_expr_type(expr, env)
                });
                if found != self.expected_return {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::ReturnTypeMismatch {
                            function_name: self.function_name.to_string(),
                            expected: display_type_id(self.expected_return).to_string(),
                            found: display_type_id(found).to_string(),
                        })
                        .with_span(self.lowered.source_map.stmt_span(stmt_id)),
                    );
                }
            }
            StmtKind::While { condition, body } => {
                let _ = self.infer_expr_type(*condition, env);
                self.loop_depth += 1;
                let _ = self.infer_block_types(*body, env);
                self.loop_depth -= 1;
            }
            StmtKind::Loop { body } => {
                self.loop_depth += 1;
                let _ = self.infer_block_types(*body, env);
                self.loop_depth -= 1;
            }
            StmtKind::Break => {
                if self.loop_depth == 0 {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::BreakOutsideLoop)
                            .with_span(self.lowered.source_map.stmt_span(stmt_id)),
                    );
                }
            }
            StmtKind::Continue => {
                if self.loop_depth == 0 {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::ContinueOutsideLoop)
                            .with_span(self.lowered.source_map.stmt_span(stmt_id)),
                    );
                }
            }
            StmtKind::Expr(expr) => {
                let _ = self.infer_expr_type(*expr, env);
            }
        }
    }

    fn resolve_place_type(&self, place_id: PlaceId, env: &BodyTypeEnv) -> Option<TypeId> {
        match &self.lowered.module.place(place_id).kind {
            PlaceKind::Name(_) => {
                self.names
                    .place_resolution(place_id)
                    .and_then(|resolved| match resolved {
                        ResolvedName::Param(id) => env.params.get(&id).copied(),
                        ResolvedName::Local(id) => env.locals.get(&id).copied(),
                        ResolvedName::Function(_)
                        | ResolvedName::Struct(_)
                        | ResolvedName::Enum(_) => None,
                    })
            }
        }
    }

    fn infer_expr_type(&mut self, expr_id: ExprId, env: &mut BodyTypeEnv) -> TypeId {
        if let Some(ty) = env.exprs.get(&expr_id).copied() {
            return ty;
        }

        let expr = self.lowered.module.expr(expr_id);
        let ty = match &expr.kind {
            ExprKind::Name(_) => self
                .names
                .expr_resolution(expr_id)
                .and_then(|resolved| match resolved {
                    ResolvedName::Param(id) => env.params.get(&id).copied(),
                    ResolvedName::Local(id) => env.locals.get(&id).copied(),
                    ResolvedName::Function(id) => {
                        self.function_index.by_id.get(&id).map(|f| f.return_type)
                    }
                    ResolvedName::Struct(_) | ResolvedName::Enum(_) => None,
                })
                .unwrap_or(TypeId::Builtin(BuiltinType::Unit)),
            ExprKind::Literal(literal) => match literal.kind {
                LiteralKind::Number => TypeId::Builtin(BuiltinType::I32),
                LiteralKind::Float => TypeId::Builtin(BuiltinType::F32),
                LiteralKind::String => TypeId::Builtin(BuiltinType::Str),
                LiteralKind::Bool => TypeId::Builtin(BuiltinType::Bool),
            },
            ExprKind::Prefix { op, expr } => {
                let inner = self.infer_expr_type(*expr, env);
                match op {
                    PrefixOp::Neg => inner,
                    PrefixOp::Not => TypeId::Builtin(BuiltinType::Bool),
                }
            }
            ExprKind::Binary { lhs, op, rhs } => {
                let lhs_ty = self.infer_expr_type(*lhs, env);
                let _ = self.infer_expr_type(*rhs, env);
                match op {
                    BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::Lt
                    | BinaryOp::Gt
                    | BinaryOp::Le
                    | BinaryOp::Ge
                    | BinaryOp::AndAnd
                    | BinaryOp::OrOr => TypeId::Builtin(BuiltinType::Bool),
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => lhs_ty,
                }
            }
            ExprKind::Call { callee, args } => {
                for arg in args {
                    let _ = self.infer_expr_type(*arg, env);
                }
                self.infer_expr_type(*callee, env)
            }
            ExprKind::Field { receiver, .. } => {
                let _ = self.infer_expr_type(*receiver, env);
                TypeId::Builtin(BuiltinType::Unit)
            }
            ExprKind::Index { receiver, index } => {
                let _ = self.infer_expr_type(*receiver, env);
                let _ = self.infer_expr_type(*index, env);
                TypeId::Builtin(BuiltinType::Unit)
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let _ = self.infer_expr_type(*condition, env);
                let then_ty = self.infer_block_types(*then_branch, env);
                match else_branch {
                    Some(else_expr) => {
                        let else_ty = self.infer_expr_type(*else_expr, env);
                        if then_ty != else_ty {
                            self.diagnostics.push(
                                Diagnostic::error(DiagnosticKind::IfBranchTypeMismatch {
                                    expected: display_type_id(then_ty).to_string(),
                                    found: display_type_id(else_ty).to_string(),
                                })
                                .with_span(self.lowered.source_map.expr_span(*else_expr)),
                            );
                        }
                        then_ty
                    }
                    None => TypeId::Builtin(BuiltinType::Unit),
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                let _ = self.infer_expr_type(*scrutinee, env);
                let mut arm_iter = arms.iter();
                match arm_iter.next() {
                    Some(first_arm) => {
                        let expected = self.infer_expr_type(first_arm.expr, env);
                        for arm in arm_iter {
                            let found = self.infer_expr_type(arm.expr, env);
                            if found != expected {
                                self.diagnostics.push(
                                    Diagnostic::error(DiagnosticKind::MatchArmTypeMismatch {
                                        expected: display_type_id(expected).to_string(),
                                        found: display_type_id(found).to_string(),
                                    })
                                    .with_span(self.lowered.source_map.expr_span(arm.expr)),
                                );
                            }
                        }
                        expected
                    }
                    None => TypeId::Builtin(BuiltinType::Unit),
                }
            }
            ExprKind::StructInit { fields, .. } => {
                for field in fields {
                    let _ = self.infer_expr_type(field.value, env);
                }
                TypeId::Builtin(BuiltinType::Unit)
            }
            ExprKind::Tuple(elements) | ExprKind::Array(elements) => {
                for expr in elements {
                    let _ = self.infer_expr_type(*expr, env);
                }
                TypeId::Builtin(BuiltinType::Unit)
            }
            ExprKind::Block(block) => self.infer_block_types(*block, env),
        };

        env.exprs.insert(expr_id, ty);
        self.type_table.insert_expr(expr_id, ty);
        ty
    }
}
