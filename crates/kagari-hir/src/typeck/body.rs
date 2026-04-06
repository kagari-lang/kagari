use kagari_common::{Diagnostic, DiagnosticKind};
use smallvec::SmallVec;

use crate::{
    builtin::BuiltinFunction,
    hir::{
        BinaryOp, BlockId, ExprId, ExprKind, LiteralKind, MatchArm, PatternKind, PlaceId,
        PlaceKind, PrefixOp, StmtId, StmtKind,
    },
    lower::LoweredModule,
    resolver::{ResolvedName, ResolvedNames},
    typeck::ty::{display_type_id, resolve_type},
    typeck::{BodyTypeEnv, FunctionTypeIndex, TopLevelTypeIndex, TypeIndexes, TypeTable},
    types::{BuiltinType, TypeId},
};

pub(crate) struct BodyChecker<'a> {
    lowered: &'a LoweredModule,
    names: &'a ResolvedNames,
    function_index: &'a FunctionTypeIndex,
    top_level_index: &'a TopLevelTypeIndex,
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
        indexes: TypeIndexes<'a>,
        diagnostics: &'a mut SmallVec<[Diagnostic; 4]>,
        type_table: &'a mut TypeTable,
        function_name: &'a str,
        expected_return: TypeId,
    ) -> Self {
        Self {
            lowered,
            names,
            function_index: indexes.function_index,
            top_level_index: indexes.top_level_index,
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

    fn check_stmt(&mut self, stmt_id: StmtId, env: &mut BodyTypeEnv) {
        let stmt = self.lowered.module.stmt(stmt_id);
        match &stmt.kind {
            StmtKind::Let {
                local,
                mutable,
                ty,
                initializer,
                ..
            } => {
                let initializer_ty = self.infer_expr_type(*initializer, env);
                let local_ty = ty
                    .and_then(|ty| resolve_type(&self.lowered.module, ty))
                    .unwrap_or(initializer_ty);
                env.locals.insert(*local, local_ty.clone());
                env.local_mutability.insert(*local, *mutable);
                self.type_table.insert_local(*local, local_ty);
            }
            StmtKind::Assign { target, value } => {
                let value_ty = self.infer_expr_type(*value, env);
                match self.resolve_place_type(*target, env) {
                    Some(expected) if expected != value_ty => self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::AssignmentTypeMismatch {
                            expected: display_type_id(&expected),
                            found: display_type_id(&value_ty),
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
                            expected: display_type_id(&self.expected_return),
                            found: display_type_id(&found),
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

    fn resolve_place_type(&mut self, place_id: PlaceId, env: &mut BodyTypeEnv) -> Option<TypeId> {
        let ty = match &self.lowered.module.place(place_id).kind {
            PlaceKind::Name(_) => {
                self.place_root_resolution(place_id)
                    .and_then(|resolved| match resolved {
                        ResolvedName::Param(_) => None,
                        ResolvedName::Local(id) => env
                            .locals
                            .get(&id)
                            .filter(|_| env.local_mutability.get(&id).copied().unwrap_or(false))
                            .cloned(),
                        ResolvedName::Static(id) => self
                            .top_level_index
                            .statics
                            .get(&id)
                            .filter(|item| item.mutable)
                            .map(|item| item.ty.clone()),
                        ResolvedName::Const(_)
                        | ResolvedName::Function(_)
                        | ResolvedName::Struct(_)
                        | ResolvedName::Enum(_) => None,
                    })
            }
            PlaceKind::Field { base, name } => self
                .resolve_place_type(*base, env)
                .and_then(|base_ty| self.resolve_field_type(&base_ty, name)),
            PlaceKind::Index { base, index } => {
                let base_ty = self.resolve_place_type(*base, env)?;
                self.infer_expr_type(*index, env);
                self.resolve_index_type(*index, &base_ty)
            }
        };

        if let Some(ty) = ty.clone() {
            self.type_table.insert_place(place_id, ty);
        }

        ty
    }

    fn place_root_resolution(&self, place_id: PlaceId) -> Option<ResolvedName> {
        let root = self.place_root(place_id);
        self.names.place_resolution(root)
    }

    fn place_root(&self, place_id: PlaceId) -> PlaceId {
        match &self.lowered.module.place(place_id).kind {
            PlaceKind::Name(_) => place_id,
            PlaceKind::Field { base, .. } | PlaceKind::Index { base, .. } => self.place_root(*base),
        }
    }

    pub(crate) fn infer_expr_type(&mut self, expr_id: ExprId, env: &mut BodyTypeEnv) -> TypeId {
        if let Some(ty) = env.exprs.get(&expr_id).cloned() {
            return ty;
        }

        let expr = self.lowered.module.expr(expr_id);
        let ty = match &expr.kind {
            ExprKind::Name(_) => self
                .names
                .expr_resolution(expr_id)
                .and_then(|resolved| match resolved {
                    ResolvedName::Param(id) => env.params.get(&id).cloned(),
                    ResolvedName::Local(id) => env.locals.get(&id).cloned(),
                    ResolvedName::Const(id) => self.top_level_index.consts.get(&id).cloned(),
                    ResolvedName::Static(id) => self
                        .top_level_index
                        .statics
                        .get(&id)
                        .map(|item| item.ty.clone()),
                    ResolvedName::Function(id) => self
                        .function_index
                        .by_id
                        .get(&id)
                        .map(|function| function.return_type.clone()),
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
                if let Some(helper_ty) = self.infer_runtime_helper_call_type(*callee, args, env) {
                    helper_ty
                } else {
                    self.infer_function_call_type(*callee, args, env)
                }
            }
            ExprKind::Field { receiver, name } => {
                let receiver_ty = self.infer_expr_type(*receiver, env);
                self.resolve_field_type(&receiver_ty, name)
                    .unwrap_or(TypeId::Builtin(BuiltinType::Unit))
            }
            ExprKind::Index { receiver, index } => {
                let receiver_ty = self.infer_expr_type(*receiver, env);
                self.infer_expr_type(*index, env);
                self.resolve_index_type(*index, &receiver_ty)
                    .unwrap_or(TypeId::Builtin(BuiltinType::Unit))
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.infer_expr_type(*condition, env);
                let then_ty = self.infer_block_types(*then_branch, env);
                match else_branch {
                    Some(else_expr) => {
                        let else_ty = self.infer_expr_type(*else_expr, env);
                        if then_ty != else_ty {
                            self.diagnostics.push(
                                Diagnostic::error(DiagnosticKind::IfBranchTypeMismatch {
                                    expected: display_type_id(&then_ty),
                                    found: display_type_id(&else_ty),
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
                let scrutinee_ty = self.infer_expr_type(*scrutinee, env);
                let mut arm_iter = arms.iter();
                match arm_iter.next() {
                    Some(first_arm) => {
                        let expected = self.infer_match_arm_type(first_arm, &scrutinee_ty, env);
                        for arm in arm_iter {
                            let found = self.infer_match_arm_type(arm, &scrutinee_ty, env);
                            if found != expected {
                                self.diagnostics.push(
                                    Diagnostic::error(DiagnosticKind::MatchArmTypeMismatch {
                                        expected: display_type_id(&expected),
                                        found: display_type_id(&found),
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
            ExprKind::StructInit { path, fields } => {
                for field in fields {
                    self.infer_expr_type(field.value, env);
                }
                self.resolve_named_type(path)
                    .unwrap_or(TypeId::Builtin(BuiltinType::Unit))
            }
            ExprKind::Tuple(elements) => TypeId::Tuple(
                elements
                    .iter()
                    .map(|expr| self.infer_expr_type(*expr, env))
                    .collect::<Vec<_>>(),
            ),
            ExprKind::Array(elements) => {
                let element_types = elements
                    .iter()
                    .map(|expr| self.infer_expr_type(*expr, env))
                    .collect::<Vec<_>>();
                let element_ty = element_types
                    .first()
                    .cloned()
                    .unwrap_or(TypeId::Builtin(BuiltinType::Unit));
                TypeId::Array(Box::new(element_ty))
            }
            ExprKind::Block(block) => self.infer_block_types(*block, env),
        };

        env.exprs.insert(expr_id, ty.clone());
        self.type_table.insert_expr(expr_id, ty.clone());
        ty
    }

    fn infer_match_arm_type(
        &mut self,
        arm: &MatchArm,
        scrutinee_ty: &TypeId,
        env: &mut BodyTypeEnv,
    ) -> TypeId {
        let mut arm_env = env.clone();
        if let PatternKind::Name { local, .. } = self.lowered.module.pattern(arm.pattern).kind {
            arm_env.locals.insert(local, scrutinee_ty.clone());
            self.type_table.insert_local(local, scrutinee_ty.clone());
        }
        self.infer_expr_type(arm.expr, &mut arm_env)
    }

    fn infer_runtime_helper_call_type(
        &mut self,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        let builtin = self.builtin_function(callee)?;
        match builtin {
            BuiltinFunction::TypeOf => {
                let _ = self.infer_call_args(args, env);
                Some(TypeId::Builtin(BuiltinType::Str))
            }
            BuiltinFunction::GetField => {
                let [base, field_name_expr] = args else {
                    return Some(TypeId::Builtin(BuiltinType::Unit));
                };
                let base_ty = self.infer_expr_type(*base, env);
                let _ = self.infer_expr_type(*field_name_expr, env);
                let field_name = self.string_literal_value(*field_name_expr)?;
                Some(
                    self.resolve_field_type(&base_ty, &field_name)
                        .unwrap_or(TypeId::Builtin(BuiltinType::Unit)),
                )
            }
            BuiltinFunction::SetField => {
                let [base, field_name_expr, value] = args else {
                    return Some(TypeId::Builtin(BuiltinType::Unit));
                };
                let base_ty = self.infer_expr_type(*base, env);
                self.check_const_write(*base);
                let _ = self.infer_expr_type(*field_name_expr, env);
                let value_ty = self.infer_expr_type(*value, env);
                let field_name = self.string_literal_value(*field_name_expr)?;
                if let Some(expected) = self.resolve_field_type(&base_ty, &field_name)
                    && expected != value_ty
                {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::AssignmentTypeMismatch {
                            expected: display_type_id(&expected),
                            found: display_type_id(&value_ty),
                        })
                        .with_span(self.lowered.source_map.expr_span(*value)),
                    );
                }
                Some(base_ty)
            }
            BuiltinFunction::SetIndex => {
                let [base, index, value] = args else {
                    return Some(TypeId::Builtin(BuiltinType::Unit));
                };
                let base_ty = self.infer_expr_type(*base, env);
                self.check_const_write(*base);
                self.infer_expr_type(*index, env);
                let value_ty = self.infer_expr_type(*value, env);
                if let Some(expected) = self.resolve_index_type(*index, &base_ty)
                    && expected != value_ty
                {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::AssignmentTypeMismatch {
                            expected: display_type_id(&expected),
                            found: display_type_id(&value_ty),
                        })
                        .with_span(self.lowered.source_map.expr_span(*value)),
                    );
                }
                Some(base_ty)
            }
        }
    }

    fn infer_function_call_type(
        &mut self,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> TypeId {
        let arg_tys = self.infer_call_args(args, env);
        let Some(ResolvedName::Function(id)) = self.names.expr_resolution(callee) else {
            return self.infer_expr_type(callee, env);
        };
        let Some(function) = self.function_index.by_id.get(&id) else {
            return self.infer_expr_type(callee, env);
        };
        if function.params.len() != arg_tys.len() {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::CallArityMismatch {
                    function_name: function.name.clone(),
                    expected: function.params.len(),
                    found: arg_tys.len(),
                })
                .with_span(self.lowered.source_map.expr_span(callee)),
            );
        }
        for (index, (arg_expr, arg_ty)) in arg_tys.iter().enumerate() {
            if let Some(param) = function.params.get(index)
                && param.ty != *arg_ty
            {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::ArgumentTypeMismatch {
                        function_name: function.name.clone(),
                        parameter_name: param.name.clone(),
                        expected: display_type_id(&param.ty),
                        found: display_type_id(arg_ty),
                    })
                    .with_span(self.lowered.source_map.expr_span(*arg_expr)),
                );
            }
        }
        function.return_type.clone()
    }

    fn const_root_name(&self, expr_id: ExprId) -> Option<String> {
        match &self.lowered.module.expr(expr_id).kind {
            ExprKind::Name(_) => match self.names.expr_resolution(expr_id) {
                Some(ResolvedName::Const(id)) => self
                    .lowered
                    .module
                    .consts
                    .iter()
                    .find(|item| item.id == id)
                    .map(|item| item.name.clone()),
                _ => None,
            },
            ExprKind::Field { receiver, .. } | ExprKind::Index { receiver, .. } => {
                self.const_root_name(*receiver)
            }
            _ => None,
        }
    }

    fn resolve_named_type(&self, name: &str) -> Option<TypeId> {
        TypeId::from_name(name)
            .or_else(|| {
                self.lowered
                    .module
                    .structs
                    .iter()
                    .find(|item| item.name == name)
                    .map(|_| TypeId::Struct(name.to_owned()))
            })
            .or_else(|| {
                self.lowered
                    .module
                    .enums
                    .iter()
                    .find(|item| item.name == name)
                    .map(|_| TypeId::Enum(name.to_owned()))
            })
    }

    fn resolve_field_type(&self, receiver: &TypeId, field_name: &str) -> Option<TypeId> {
        match receiver {
            TypeId::Struct(name) => self
                .lowered
                .module
                .structs
                .iter()
                .find(|item| item.name == *name)
                .and_then(|item| item.fields.iter().find(|field| field.name == field_name))
                .and_then(|field| resolve_type(&self.lowered.module, field.ty)),
            _ => None,
        }
    }

    fn resolve_index_type(&self, index_expr: ExprId, receiver: &TypeId) -> Option<TypeId> {
        match receiver {
            TypeId::Array(element) => Some((**element).clone()),
            TypeId::Tuple(elements) => self
                .tuple_index(index_expr)
                .and_then(|index| elements.get(index).cloned()),
            _ => None,
        }
    }

    fn tuple_index(&self, index_expr: ExprId) -> Option<usize> {
        let expr = self.lowered.module.expr(index_expr);
        let ExprKind::Literal(literal) = &expr.kind else {
            return None;
        };
        if literal.kind != LiteralKind::Number {
            return None;
        }
        literal.text.parse::<usize>().ok()
    }

    fn builtin_function(&self, expr_id: ExprId) -> Option<BuiltinFunction> {
        let expr = self.lowered.module.expr(expr_id);
        let ExprKind::Name(name) = &expr.kind else {
            return None;
        };
        BuiltinFunction::from_name(name)
    }

    fn string_literal_value(&self, expr_id: ExprId) -> Option<String> {
        let expr = self.lowered.module.expr(expr_id);
        let ExprKind::Literal(literal) = &expr.kind else {
            return None;
        };
        if literal.kind != LiteralKind::String {
            return None;
        }
        Some(
            literal
                .text
                .strip_prefix('"')
                .and_then(|text| text.strip_suffix('"'))
                .unwrap_or(&literal.text)
                .to_owned(),
        )
    }

    fn infer_call_args(&mut self, args: &[ExprId], env: &mut BodyTypeEnv) -> Vec<(ExprId, TypeId)> {
        args.iter()
            .map(|arg| (*arg, self.infer_expr_type(*arg, env)))
            .collect()
    }

    fn check_const_write(&mut self, expr_id: ExprId) {
        if let Some(const_name) = self.const_root_name(expr_id) {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::ConstWriteNotAllowed { const_name })
                    .with_span(self.lowered.source_map.expr_span(expr_id)),
            );
        }
    }
}
