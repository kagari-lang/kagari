use std::collections::HashSet;

use kagari_common::{Diagnostic, DiagnosticKind};
use smallvec::SmallVec;

use crate::{
    builtin::{BuiltinFunction, BuiltinMethod, StringMethod, array, surface},
    hir::{
        BinaryOp, BlockId, ExprId, ExprKind, LiteralKind, MatchArm, PatternKind, PlaceId,
        PlaceKind, PrefixOp, StmtId, StmtKind,
    },
    lower::LoweredModule,
    resolver::{ResolvedName, ResolvedNames},
    typeck::ty::{TypeContext, display_type_id, resolve_type, resolve_type_in},
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
            StmtKind::Binding {
                local,
                writeability,
                ty,
                initializer,
                ..
            } => {
                let initializer_ty = self.infer_expr_type(*initializer, env);
                let local_ty = ty
                    .and_then(|ty| {
                        resolve_type_in(
                            &self.lowered.module,
                            ty,
                            TypeContext {
                                generics: &env.generics,
                            },
                        )
                    })
                    .unwrap_or(initializer_ty);
                env.locals.insert(*local, local_ty.clone());
                env.local_writeability.insert(*local, *writeability);
                self.type_table.insert_local(*local, local_ty);
            }
            StmtKind::Assign { target, value } => {
                let value_ty = self.infer_expr_type(*value, env);
                match self.resolve_assignment_target_type(*target, env) {
                    Some(expected) if expected != value_ty => self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::AssignmentTypeMismatch {
                            expected: display_type_id(&expected),
                            found: display_type_id(&value_ty),
                        })
                        .with_span(self.lowered.source_map.place_span(*target)),
                    ),
                    None => {
                        let reason = self.assignment_target_error_reason(*target, env);
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::InvalidAssignmentTarget { reason })
                                .with_span(self.lowered.source_map.place_span(*target)),
                        );
                    }
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
                self.check_condition_type(*condition, "while", env);
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

    fn resolve_assignment_target_type(
        &mut self,
        place_id: PlaceId,
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        let ty = match &self.lowered.module.place(place_id).kind {
            PlaceKind::Name(_) => {
                self.place_root_resolution(place_id)
                    .and_then(|resolved| match resolved {
                        ResolvedName::Param(_) => None,
                        ResolvedName::Local(id) => env
                            .locals
                            .get(&id)
                            .filter(|_| {
                                env.local_writeability
                                    .get(&id)
                                    .copied()
                                    .is_some_and(|writeability| writeability.is_var())
                            })
                            .cloned(),
                        ResolvedName::Const(_)
                        | ResolvedName::Function(_)
                        | ResolvedName::Module(_)
                        | ResolvedName::Struct(_)
                        | ResolvedName::Enum(_)
                        | ResolvedName::Trait(_) => None,
                    })
            }
            PlaceKind::Field { base, name } => {
                let base_ty = self.resolve_readable_place_type(*base, env)?;
                self.resolve_writable_field_type(&base_ty, name)
            }
            PlaceKind::Index { base, index } => {
                let base_ty = self.resolve_readable_place_type(*base, env)?;
                self.infer_expr_type(*index, env);
                self.resolve_index_type(*index, &base_ty)
            }
        };

        if let Some(ty) = ty.clone() {
            self.type_table.insert_place(place_id, ty);
        }

        ty
    }

    fn resolve_readable_place_type(
        &mut self,
        place_id: PlaceId,
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        let ty = match &self.lowered.module.place(place_id).kind {
            PlaceKind::Name(_) => {
                self.place_root_resolution(place_id)
                    .and_then(|resolved| match resolved {
                        ResolvedName::Param(id) => env.params.get(&id).cloned(),
                        ResolvedName::Local(id) => env.locals.get(&id).cloned(),
                        ResolvedName::Const(id) => self.top_level_index.consts.get(&id).cloned(),
                        ResolvedName::Function(_)
                        | ResolvedName::Module(_)
                        | ResolvedName::Struct(_)
                        | ResolvedName::Enum(_)
                        | ResolvedName::Trait(_) => None,
                    })
            }
            PlaceKind::Field { base, name } => {
                let base_ty = self.resolve_readable_place_type(*base, env)?;
                self.resolve_field_type(&base_ty, name)
            }
            PlaceKind::Index { base, index } => {
                let base_ty = self.resolve_readable_place_type(*base, env)?;
                self.infer_expr_type(*index, env);
                self.resolve_index_type(*index, &base_ty)
            }
        };

        if let Some(ty) = ty.clone() {
            self.type_table.insert_place(place_id, ty);
        }

        ty
    }

    fn assignment_target_error_reason(
        &mut self,
        place_id: PlaceId,
        env: &mut BodyTypeEnv,
    ) -> String {
        match &self.lowered.module.place(place_id).kind {
            PlaceKind::Name(_) => self
                .place_root_resolution(place_id)
                .map(|resolved| match resolved {
                    ResolvedName::Param(_) => {
                        "function parameters are `val` bindings and cannot be reassigned"
                            .to_string()
                    }
                    ResolvedName::Local(id) => match env.local_writeability.get(&id).copied() {
                        Some(writeability) if !writeability.is_var() => {
                            "`val` binding cannot be reassigned".to_string()
                        }
                        Some(_) => "assignment target type could not be resolved".to_string(),
                        None => "unresolved assignment target".to_string(),
                    },
                    ResolvedName::Const(_) => "`const` item cannot be reassigned".to_string(),
                    ResolvedName::Function(_) => "function item is not assignable".to_string(),
                    ResolvedName::Module(_) => "module item is not assignable".to_string(),
                    ResolvedName::Struct(_) => "struct type is not assignable".to_string(),
                    ResolvedName::Enum(_) => "enum type is not assignable".to_string(),
                    ResolvedName::Trait(_) => "trait type is not assignable".to_string(),
                })
                .unwrap_or_else(|| "unresolved assignment target".to_string()),
            PlaceKind::Field { base, name } => {
                let Some(base_ty) = self.resolve_readable_place_type(*base, env) else {
                    return self.assignment_target_error_reason(*base, env);
                };
                match self.resolve_field(&base_ty, name) {
                    Some(field) if !field.writeability.is_var() => {
                        format!("`val` field `{name}` cannot be assigned")
                    }
                    Some(_) => "assignment target type could not be resolved".to_string(),
                    None => format!("unknown field `{name}`"),
                }
            }
            PlaceKind::Index { base, index } => {
                let Some(base_ty) = self.resolve_readable_place_type(*base, env) else {
                    return self.assignment_target_error_reason(*base, env);
                };
                self.infer_expr_type(*index, env);
                if self.resolve_index_type(*index, &base_ty).is_none() {
                    "indexed value is not assignable".to_string()
                } else {
                    "assignment target type could not be resolved".to_string()
                }
            }
        }
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
                    ResolvedName::Function(id) => self
                        .function_index
                        .by_id
                        .get(&id)
                        .map(|function| function.return_type.clone()),
                    ResolvedName::Module(_)
                    | ResolvedName::Struct(_)
                    | ResolvedName::Enum(_)
                    | ResolvedName::Trait(_) => None,
                })
                .unwrap_or(TypeId::Builtin(BuiltinType::Unit)),
            ExprKind::Literal(literal) => match literal.kind {
                LiteralKind::Number => TypeId::Builtin(BuiltinType::I32),
                LiteralKind::Float => TypeId::Builtin(BuiltinType::F32),
                LiteralKind::String => TypeId::Builtin(BuiltinType::String),
                LiteralKind::Bool => TypeId::Builtin(BuiltinType::Bool),
            },
            ExprKind::Prefix { op, expr } => {
                let inner = self.infer_expr_type(*expr, env);
                match op {
                    PrefixOp::Neg => {
                        if !surface::supports_unary_negation(&inner) {
                            self.diagnostics.push(
                                Diagnostic::error(DiagnosticKind::UnaryOperandTypeMismatch {
                                    operator: "-",
                                    expected: "numeric".to_owned(),
                                    found: display_type_id(&inner),
                                })
                                .with_span(self.lowered.source_map.expr_span(*expr)),
                            );
                        }
                        inner
                    }
                    PrefixOp::Not => {
                        if inner != TypeId::Builtin(BuiltinType::Bool) {
                            self.diagnostics.push(
                                Diagnostic::error(DiagnosticKind::UnaryOperandTypeMismatch {
                                    operator: "!",
                                    expected: "bool".to_owned(),
                                    found: display_type_id(&inner),
                                })
                                .with_span(self.lowered.source_map.expr_span(*expr)),
                            );
                        }
                        TypeId::Builtin(BuiltinType::Bool)
                    }
                }
            }
            ExprKind::Binary { lhs, op, rhs } => {
                let lhs_ty = self.infer_expr_type(*lhs, env);
                let rhs_ty = self.infer_expr_type(*rhs, env);
                self.infer_binary_type(*op, *rhs, lhs_ty, rhs_ty)
            }
            ExprKind::Call { callee, args } => {
                if let Some(helper_ty) = self.infer_runtime_helper_call_type(*callee, args, env) {
                    helper_ty
                } else if let Some(method_ty) =
                    self.infer_trait_method_call_type(*callee, args, env)
                {
                    method_ty
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
                self.check_condition_type(*condition, "if", env);
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
                self.infer_struct_init_type(path, fields, expr_id, env)
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
                    .map(|expr| (*expr, self.infer_expr_type(*expr, env)))
                    .collect::<Vec<_>>();
                let element_ty = element_types
                    .first()
                    .map(|(_, ty)| ty.clone())
                    .unwrap_or(TypeId::Builtin(BuiltinType::Unit));
                for (expr, ty) in element_types.iter().skip(1) {
                    if *ty != element_ty {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::ArrayElementTypeMismatch {
                                expected: display_type_id(&element_ty),
                                found: display_type_id(ty),
                            })
                            .with_span(self.lowered.source_map.expr_span(*expr)),
                        );
                    }
                }
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
        if let Some(method_ty) = self.infer_builtin_method_call_type(callee, args, env) {
            return Some(method_ty);
        }

        let builtin = self.builtin_function(callee)?;
        match builtin {
            BuiltinFunction::TypeOf => {
                let _ = self.infer_call_args(args, env);
                Some(TypeId::Builtin(BuiltinType::String))
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
            BuiltinFunction::Print => {
                self.check_builtin_arity("print", 1, args.len(), callee);
                let arg_tys = self.infer_call_args(args, env);
                if let Some((arg, ty)) = arg_tys.first()
                    && *ty != TypeId::Builtin(BuiltinType::String)
                {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::ArgumentTypeMismatch {
                            function_name: "print".to_owned(),
                            parameter_name: "message".to_owned(),
                            expected: "String".to_owned(),
                            found: display_type_id(ty),
                        })
                        .with_span(self.lowered.source_map.expr_span(*arg)),
                    );
                }
                Some(TypeId::Builtin(BuiltinType::Unit))
            }
        }
    }

    fn infer_builtin_method_call_type(
        &mut self,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        let (method, _, receiver_ty) = self.builtin_method(callee, env)?;
        match method {
            BuiltinMethod::Array(method) => {
                self.infer_array_method_call_type(method, callee, &receiver_ty, args, env)
            }
            BuiltinMethod::Iterable(_) => None,
            BuiltinMethod::String(method) => {
                self.infer_string_method_call_type(method, callee, args, env)
            }
        }
    }

    fn infer_array_method_call_type(
        &mut self,
        method: array::Method,
        callee: ExprId,
        receiver_ty: &TypeId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        let spec = array::method_spec(method);
        match method {
            array::Method::Len => {
                let _ = self.infer_call_args(args, env);
                self.check_builtin_arity(spec.name, spec.arity, args.len(), callee);
                Some(TypeId::Builtin(BuiltinType::USize))
            }
            array::Method::Push => {
                let TypeId::Array(element_ty) = receiver_ty else {
                    return Some(TypeId::Builtin(BuiltinType::Unit));
                };
                self.check_builtin_arity(spec.name, spec.arity, args.len(), callee);
                let value_ty = args
                    .first()
                    .map(|expr| self.infer_expr_type(*expr, env))
                    .unwrap_or(TypeId::Builtin(BuiltinType::Unit));
                if value_ty != **element_ty {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::ArgumentTypeMismatch {
                            function_name: spec.name.to_string(),
                            parameter_name: "value".to_string(),
                            expected: display_type_id(element_ty),
                            found: display_type_id(&value_ty),
                        })
                        .with_span(
                            args.first()
                                .copied()
                                .map(|expr| self.lowered.source_map.expr_span(expr))
                                .unwrap_or_else(|| self.lowered.source_map.expr_span(callee)),
                        ),
                    );
                }
                for arg in args.iter().skip(1) {
                    let _ = self.infer_expr_type(*arg, env);
                }
                Some(receiver_ty.clone())
            }
            array::Method::Pop => {
                let _ = self.infer_call_args(args, env);
                self.check_builtin_arity(spec.name, spec.arity, args.len(), callee);
                Some(receiver_ty.clone())
            }
        }
    }

    fn infer_string_method_call_type(
        &mut self,
        method: StringMethod,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        let spec = BuiltinMethod::String(method).spec();
        match method {
            StringMethod::Len => {
                let _ = self.infer_call_args(args, env);
                self.check_builtin_arity(spec.name, spec.arity, args.len(), callee);
                Some(TypeId::Builtin(BuiltinType::USize))
            }
        }
    }

    fn infer_trait_method_call_type(
        &mut self,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        let expr = self.lowered.module.expr(callee);
        let ExprKind::Field { receiver, name } = &expr.kind else {
            return None;
        };
        let receiver_ty = self.infer_expr_type(*receiver, env);
        let (trait_name, self_ty) = match &receiver_ty {
            TypeId::Trait(name) => (name.clone(), receiver_ty.clone()),
            TypeId::Generic(generic_name) => {
                let trait_name = env
                    .generic_bounds
                    .get(generic_name)
                    .and_then(|bounds| {
                        bounds.iter().find(|trait_name| {
                            self.trait_method_function(trait_name, name).is_some()
                        })
                    })
                    .cloned()?;
                (trait_name, receiver_ty.clone())
            }
            _ => return None,
        };

        let method_function = self.trait_method_function(&trait_name, name)?;
        let Some(method) = self.function_index.by_id.get(&method_function) else {
            return Some(TypeId::Builtin(BuiltinType::Unit));
        };
        let params = method
            .params
            .iter()
            .filter(|param| param.name != "self")
            .collect::<Vec<_>>();
        let arg_tys = self.infer_call_args(args, env);
        if params.len() != arg_tys.len() {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::CallArityMismatch {
                    function_name: name.clone(),
                    expected: params.len(),
                    found: arg_tys.len(),
                })
                .with_span(self.lowered.source_map.expr_span(callee)),
            );
        }
        for (index, (arg_expr, arg_ty)) in arg_tys.iter().enumerate() {
            if let Some(param) = params.get(index) {
                let expected = substitute_self_type(&param.ty, &self_ty);
                if expected != *arg_ty {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::ArgumentTypeMismatch {
                            function_name: name.clone(),
                            parameter_name: param.name.clone(),
                            expected: display_type_id(&expected),
                            found: display_type_id(arg_ty),
                        })
                        .with_span(self.lowered.source_map.expr_span(*arg_expr)),
                    );
                }
            }
        }

        Some(substitute_self_type(&method.return_type, &self_ty))
    }

    fn trait_method_function(
        &self,
        trait_name: &str,
        method_name: &str,
    ) -> Option<crate::hir::FunctionId> {
        self.lowered
            .module
            .traits
            .iter()
            .find(|trait_def| trait_def.name == trait_name)
            .and_then(|trait_def| {
                trait_def
                    .methods
                    .iter()
                    .find(|method| method.name == method_name)
                    .map(|method| method.function)
            })
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

    fn resolve_field_type(&self, receiver: &TypeId, field_name: &str) -> Option<TypeId> {
        self.resolve_field(receiver, field_name)
            .and_then(|field| resolve_type(&self.lowered.module, field.ty))
    }

    fn resolve_writable_field_type(&self, receiver: &TypeId, field_name: &str) -> Option<TypeId> {
        self.resolve_field(receiver, field_name)
            .filter(|field| field.writeability.is_var())
            .and_then(|field| resolve_type(&self.lowered.module, field.ty))
    }

    fn resolve_field(&self, receiver: &TypeId, field_name: &str) -> Option<&crate::hir::Field> {
        match receiver {
            TypeId::Struct(name) => self
                .lowered
                .module
                .structs
                .iter()
                .find(|item| item.name == *name)
                .and_then(|item| item.fields.iter().find(|field| field.name == field_name)),
            _ => None,
        }
    }

    fn infer_binary_type(
        &mut self,
        op: BinaryOp,
        rhs_expr: ExprId,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
    ) -> TypeId {
        match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                if !self.matching_numeric_operands(&lhs_ty, &rhs_ty) {
                    self.emit_binary_operand_type_mismatch(
                        op,
                        "matching numeric",
                        &lhs_ty,
                        &rhs_ty,
                        rhs_expr,
                    );
                }
                lhs_ty
            }
            BinaryOp::Eq | BinaryOp::NotEq => {
                if lhs_ty != rhs_ty {
                    self.emit_binary_operand_type_mismatch(
                        op, "matching", &lhs_ty, &rhs_ty, rhs_expr,
                    );
                }
                TypeId::Builtin(BuiltinType::Bool)
            }
            BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
                if !self.matching_numeric_operands(&lhs_ty, &rhs_ty) {
                    self.emit_binary_operand_type_mismatch(
                        op,
                        "matching numeric",
                        &lhs_ty,
                        &rhs_ty,
                        rhs_expr,
                    );
                }
                TypeId::Builtin(BuiltinType::Bool)
            }
            BinaryOp::AndAnd | BinaryOp::OrOr => {
                if lhs_ty != TypeId::Builtin(BuiltinType::Bool)
                    || rhs_ty != TypeId::Builtin(BuiltinType::Bool)
                {
                    self.emit_binary_operand_type_mismatch(op, "bool", &lhs_ty, &rhs_ty, rhs_expr);
                }
                TypeId::Builtin(BuiltinType::Bool)
            }
        }
    }

    fn matching_numeric_operands(&self, lhs: &TypeId, rhs: &TypeId) -> bool {
        surface::supports_arithmetic(lhs, rhs)
    }

    fn emit_binary_operand_type_mismatch(
        &mut self,
        op: BinaryOp,
        expected: &'static str,
        lhs: &TypeId,
        rhs: &TypeId,
        span_expr: ExprId,
    ) {
        self.diagnostics.push(
            Diagnostic::error(DiagnosticKind::BinaryOperandTypeMismatch {
                operator: self.binary_operator_name(op),
                expected: expected.to_owned(),
                lhs: display_type_id(lhs),
                rhs: display_type_id(rhs),
            })
            .with_span(self.lowered.source_map.expr_span(span_expr)),
        );
    }

    fn binary_operator_name(&self, op: BinaryOp) -> &'static str {
        match op {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Eq => "==",
            BinaryOp::NotEq => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::Gt => ">",
            BinaryOp::Le => "<=",
            BinaryOp::Ge => ">=",
            BinaryOp::AndAnd => "&&",
            BinaryOp::OrOr => "||",
        }
    }

    fn check_condition_type(
        &mut self,
        expr_id: ExprId,
        context: &'static str,
        env: &mut BodyTypeEnv,
    ) {
        let ty = self.infer_expr_type(expr_id, env);
        if ty != TypeId::Builtin(BuiltinType::Bool) {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::ConditionTypeMismatch {
                    context,
                    found: display_type_id(&ty),
                })
                .with_span(self.lowered.source_map.expr_span(expr_id)),
            );
        }
    }

    fn infer_struct_init_type(
        &mut self,
        path: &str,
        fields: &[crate::hir::FieldInit],
        expr_id: ExprId,
        env: &mut BodyTypeEnv,
    ) -> TypeId {
        let field_tys = fields
            .iter()
            .map(|field| {
                (
                    field.name.as_str(),
                    field.value,
                    self.infer_expr_type(field.value, env),
                )
            })
            .collect::<Vec<_>>();

        let Some(struct_def) = self
            .lowered
            .module
            .structs
            .iter()
            .find(|item| item.name == path)
        else {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidStructInitializer {
                    struct_name: path.to_owned(),
                    reason: "unknown struct".to_owned(),
                })
                .with_span(self.lowered.source_map.expr_span(expr_id)),
            );
            return TypeId::Builtin(BuiltinType::Unit);
        };

        let mut seen = HashSet::new();
        for (name, value_expr, value_ty) in &field_tys {
            if !seen.insert((*name).to_owned()) {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidStructInitializer {
                        struct_name: path.to_owned(),
                        reason: format!("duplicate field `{name}`"),
                    })
                    .with_span(self.lowered.source_map.expr_span(*value_expr)),
                );
                continue;
            }

            let Some(field) = struct_def.fields.iter().find(|field| field.name == *name) else {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidStructInitializer {
                        struct_name: path.to_owned(),
                        reason: format!("unknown field `{name}`"),
                    })
                    .with_span(self.lowered.source_map.expr_span(*value_expr)),
                );
                continue;
            };

            let Some(expected) = resolve_type(&self.lowered.module, field.ty) else {
                continue;
            };
            if expected != *value_ty {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::AssignmentTypeMismatch {
                        expected: display_type_id(&expected),
                        found: display_type_id(value_ty),
                    })
                    .with_span(self.lowered.source_map.expr_span(*value_expr)),
                );
            }
        }

        for field in &struct_def.fields {
            if !seen.contains(&field.name) {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidStructInitializer {
                        struct_name: path.to_owned(),
                        reason: format!("missing field `{}`", field.name),
                    })
                    .with_span(self.lowered.source_map.expr_span(expr_id)),
                );
            }
        }

        TypeId::Struct(path.to_owned())
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

    fn builtin_method(
        &mut self,
        expr_id: ExprId,
        env: &mut BodyTypeEnv,
    ) -> Option<(BuiltinMethod, ExprId, TypeId)> {
        let expr = self.lowered.module.expr(expr_id);
        let ExprKind::Field { receiver, name } = &expr.kind else {
            return None;
        };
        let receiver_ty = self.infer_expr_type(*receiver, env);
        BuiltinMethod::resolve(&receiver_ty, name).map(|method| (method, *receiver, receiver_ty))
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

    fn check_builtin_arity(&mut self, name: &str, expected: usize, found: usize, callee: ExprId) {
        if expected != found {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::CallArityMismatch {
                    function_name: name.to_string(),
                    expected,
                    found,
                })
                .with_span(self.lowered.source_map.expr_span(callee)),
            );
        }
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

fn substitute_self_type(ty: &TypeId, self_ty: &TypeId) -> TypeId {
    match ty {
        TypeId::Generic(name) if name == "Self" => self_ty.clone(),
        TypeId::Tuple(elements) => TypeId::Tuple(
            elements
                .iter()
                .map(|element| substitute_self_type(element, self_ty))
                .collect::<Vec<_>>(),
        ),
        TypeId::Array(element) => TypeId::Array(Box::new(substitute_self_type(element, self_ty))),
        TypeId::Map { key, value } => TypeId::Map {
            key: Box::new(substitute_self_type(key, self_ty)),
            value: Box::new(substitute_self_type(value, self_ty)),
        },
        TypeId::Set(element) => TypeId::Set(Box::new(substitute_self_type(element, self_ty))),
        TypeId::StandardEnum { name, args } => TypeId::StandardEnum {
            name: name.clone(),
            args: args
                .iter()
                .map(|arg| substitute_self_type(arg, self_ty))
                .collect::<Vec<_>>(),
        },
        _ => ty.clone(),
    }
}
