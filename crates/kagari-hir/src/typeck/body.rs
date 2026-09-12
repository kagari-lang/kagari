use std::collections::HashSet;

use kagari_common::{Diagnostic, DiagnosticKind};
use smallvec::SmallVec;

use crate::{
    builtin::{
        BuiltinFunction,
        surface::{self, StandardIntrinsic, StandardMethodReceiver, StandardTypeConstraint},
    },
    hir::{
        BinaryOp, BlockId, ExprId, ExprKind, LiteralKind, MatchArm, PatternKind, PlaceId,
        PlaceKind, PrefixOp, StmtId, StmtKind,
    },
    lower::LoweredModule,
    resolver::{ResolvedName, ResolvedNames},
    typeck::ty::{TypeContext, display_type, display_type_id, resolve_type_in},
    typeck::{
        BodyTypeEnv, CallTarget, FunctionTypeIndex, TopLevelTypeIndex, TypeIndexes, TypeTable,
    },
    types::{BuiltinType, TypeId},
};

pub(crate) struct BodyChecker<'a> {
    declarations: &'a crate::declarations::Declarations,
    cancel: &'a kagari_common::cancellation::CancellationToken,
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
            declarations: indexes.declarations,
            cancel: indexes.cancel,
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
            if self.cancel.check().is_err() {
                return TypeId::Unknown;
            }
            self.check_stmt(*stmt, env);
        }

        block
            .tail_expr
            .map_or(TypeId::Builtin(BuiltinType::Unit), |expr| {
                self.infer_expr_type(expr, env)
            })
    }

    fn check_stmt(&mut self, stmt_id: StmtId, env: &mut BodyTypeEnv) {
        if self.cancel.check().is_err() {
            return;
        }
        let stmt = self.lowered.module.stmt(stmt_id);
        match &stmt.kind {
            StmtKind::Binding {
                local,
                writeability,
                ty,
                initializer,
                ..
            } => {
                let mut initializer_ty = self.infer_expr_type(*initializer, env);
                let local_ty = ty
                    .map(|ty| {
                        resolve_type_in(
                            &self.lowered.module,
                            ty,
                            TypeContext {
                                declarations: self.declarations,
                                generics: &env.generics,
                                self_type: None,
                            },
                            self.type_table,
                            self.cancel,
                        )
                        .unwrap_or_else(|| {
                            self.diagnostics.push(
                                Diagnostic::error(DiagnosticKind::UnknownTypeAnnotation {
                                    type_name: display_type(&self.lowered.module, ty),
                                })
                                .with_span(self.lowered.source_map.type_span(ty)),
                            );
                            TypeId::Error
                        })
                    })
                    .unwrap_or_else(|| initializer_ty.clone());
                // Empty containers acquire their concrete parameters from the
                // annotation; keep that fact on the constructor expression too.
                if ty.is_some()
                    && matches!(
                        (
                            self.type_table
                                .call_resolution(*initializer)
                                .map(|call| call.target),
                            &local_ty
                        ),
                        (
                            Some(CallTarget::StandardIntrinsic(StandardIntrinsic::MapNew)),
                            TypeId::Map { .. }
                        ) | (
                            Some(CallTarget::StandardIntrinsic(StandardIntrinsic::SetNew)),
                            TypeId::Set(_)
                        )
                    )
                {
                    initializer_ty = local_ty.clone();
                    env.exprs.insert(*initializer, initializer_ty.clone());
                    self.type_table
                        .insert_expr(*initializer, initializer_ty.clone());
                }
                if local_ty.conflicts_with(&initializer_ty) {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::AssignmentTypeMismatch {
                            expected: display_type_id(&local_ty),
                            found: display_type_id(&initializer_ty),
                        })
                        .with_span(self.lowered.source_map.expr_span(*initializer)),
                    );
                }
                env.locals.insert(*local, local_ty.clone());
                env.local_writeability.insert(*local, *writeability);
                self.type_table.insert_local(*local, local_ty);
            }
            StmtKind::Assign { target, value, op } => {
                let target_ty = self.resolve_assignment_target_type(*target, env);
                let value_ty = self.infer_expr_type(*value, env);
                if let (Some(op), Some(expected)) = (op, &target_ty) {
                    self.infer_binary_type(*op, *value, expected.clone(), value_ty.clone(), env);
                }
                match target_ty {
                    Some(expected) if expected.conflicts_with(&value_ty) => self.diagnostics.push(
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
                if found.conflicts_with(&self.expected_return) {
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
            PlaceKind::Expr(expr) => {
                self.infer_expr_type(*expr, env);
                None
            }
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
                        | ResolvedName::StandardModule(_)
                        | ResolvedName::StandardFunction(_)
                        | ResolvedName::Struct(_)
                        | ResolvedName::Enum(_)
                        | ResolvedName::Trait(_) => None,
                    })
            }
            PlaceKind::Field { base, name } => {
                let base_ty = self.resolve_readable_place_type(*base, env)?;
                let field = self.resolve_field(&base_ty, name)?;
                let id = field.id;
                let writable = field.writeability.is_var();
                self.type_table.insert_place_field(place_id, id);
                writable.then(|| self.type_table.field_type(id)).flatten()
            }
            PlaceKind::Index { base, index } => {
                let base_ty = self.resolve_readable_place_type(*base, env)?;
                self.infer_expr_type(*index, env);
                if matches!(base_ty, TypeId::Tuple(_)) {
                    self.resolve_assignment_target_type(*base, env)?;
                }
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
            PlaceKind::Expr(expr) => Some(self.infer_expr_type(*expr, env)),
            PlaceKind::Name(_) => {
                self.place_root_resolution(place_id)
                    .and_then(|resolved| match resolved {
                        ResolvedName::Param(id) => env.params.get(&id).cloned(),
                        ResolvedName::Local(id) => env.locals.get(&id).cloned(),
                        ResolvedName::Const(id) => self.top_level_index.consts.get(&id).cloned(),
                        ResolvedName::Function(_)
                        | ResolvedName::Module(_)
                        | ResolvedName::StandardModule(_)
                        | ResolvedName::StandardFunction(_)
                        | ResolvedName::Struct(_)
                        | ResolvedName::Enum(_)
                        | ResolvedName::Trait(_) => None,
                    })
            }
            PlaceKind::Field { base, name } => {
                let base_ty = self.resolve_readable_place_type(*base, env)?;
                let field = self.resolve_field(&base_ty, name)?.id;
                self.type_table.insert_place_field(place_id, field);
                self.type_table.field_type(field)
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
            PlaceKind::Expr(_) => "temporary value cannot be reassigned".to_owned(),
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
                    ResolvedName::StandardModule(_) => {
                        "standard module item is not assignable".to_string()
                    }
                    ResolvedName::StandardFunction(_) => {
                        "standard function item is not assignable".to_string()
                    }
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
            PlaceKind::Name(_) | PlaceKind::Expr(_) => place_id,
            PlaceKind::Field { base, .. } | PlaceKind::Index { base, .. } => self.place_root(*base),
        }
    }

    pub(crate) fn infer_expr_type(&mut self, expr_id: ExprId, env: &mut BodyTypeEnv) -> TypeId {
        if self.cancel.check().is_err() {
            return TypeId::Unknown;
        }
        if let Some(ty) = env.exprs.get(&expr_id).cloned() {
            return ty;
        }

        let expr = self.lowered.module.expr(expr_id);
        let ty = match &expr.kind {
            ExprKind::Missing => {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::ExpectedExpression)
                        .with_span(self.lowered.source_map.expr_span(expr_id)),
                );
                TypeId::Unknown
            }
            ExprKind::Name(name) => self
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
                    | ResolvedName::StandardModule(_)
                    | ResolvedName::StandardFunction(_)
                    | ResolvedName::Struct(_)
                    | ResolvedName::Enum(_)
                    | ResolvedName::Trait(_) => None,
                })
                .unwrap_or_else(|| {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::UnknownName { name: name.clone() })
                            .with_span(self.lowered.source_map.expr_span(expr_id)),
                    );
                    TypeId::Error
                }),
            ExprKind::Literal(literal) => match super::ScalarValue::parse(literal) {
                Ok(value) => {
                    let ty = value.ty();
                    self.type_table.insert_scalar(expr_id, value);
                    ty
                }
                Err(reason) => {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidLiteral {
                            reason: reason.to_owned(),
                        })
                        .with_span(self.lowered.source_map.expr_span(expr_id)),
                    );
                    TypeId::Error
                }
            },
            ExprKind::Prefix { op, expr } => {
                // The magnitude of MIN is not a positive i32 expression on its own.
                if matches!(op, PrefixOp::Neg)
                    && let ExprKind::Literal(literal) = &self.lowered.module.expr(*expr).kind
                    && literal.kind == LiteralKind::Number
                    && literal.text.parse::<u64>().ok() == Some(2147483648)
                {
                    let ty = TypeId::Builtin(BuiltinType::I32);
                    self.type_table
                        .insert_scalar(expr_id, super::ScalarValue::I32(i32::MIN));
                    self.type_table.insert_expr(*expr, ty.clone());
                    self.type_table.insert_expr(expr_id, ty.clone());
                    env.exprs.insert(expr_id, ty.clone());
                    return ty;
                }
                let inner = self.infer_expr_type(*expr, env);
                match op {
                    PrefixOp::Neg => {
                        if !inner.is_unresolved() && !surface::supports_unary_negation(&inner) {
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
                        if inner.conflicts_with(&TypeId::Builtin(BuiltinType::Bool)) {
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
                self.infer_binary_type(*op, *rhs, lhs_ty, rhs_ty, env)
            }
            ExprKind::Call { callee, args } => {
                if let Some(standard_ty) =
                    self.infer_standard_call_type(expr_id, *callee, args, env)
                {
                    standard_ty
                } else if let Some(helper_ty) =
                    self.infer_runtime_helper_call_type(expr_id, *callee, args, env)
                {
                    helper_ty
                } else if let Some(method_ty) =
                    self.infer_trait_method_call_type(expr_id, *callee, args, env)
                {
                    method_ty
                } else {
                    self.infer_function_call_type(expr_id, *callee, args, env)
                }
            }
            ExprKind::Field { receiver, name } => {
                let receiver_ty = self.infer_expr_type(*receiver, env);
                if let Some(field) = self.resolve_field(&receiver_ty, name).map(|field| field.id) {
                    self.type_table.insert_expr_field(expr_id, field);
                    self.type_table.field_type(field).unwrap_or(TypeId::Error)
                } else {
                    if !receiver_ty.is_unresolved() || name.is_empty() {
                        self.diagnostics.push(
                            Diagnostic::error(if name.is_empty() {
                                DiagnosticKind::ExpectedFieldName
                            } else {
                                DiagnosticKind::UnknownName { name: name.clone() }
                            })
                            .with_span(self.lowered.source_map.expr_span(expr_id)),
                        );
                    }
                    TypeId::Error
                }
            }
            ExprKind::Index { receiver, index } => {
                let receiver_ty = self.infer_expr_type(*receiver, env);
                let index_ty = self.infer_expr_type(*index, env);
                if receiver_ty.is_unresolved() || index_ty.is_unresolved() {
                    TypeId::Error
                } else {
                    let integer_index = index_ty.is_integer();
                    integer_index
                        .then(|| self.resolve_index_type(*index, &receiver_ty))
                        .flatten()
                        .unwrap_or_else(|| {
                            self.diagnostics.push(
                                Diagnostic::error(DiagnosticKind::InvalidIndexTarget {
                                    type_name: display_type_id(&receiver_ty),
                                })
                                .with_span(self.lowered.source_map.expr_span(expr_id)),
                            );
                            TypeId::Error
                        })
                }
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
                        if then_ty.conflicts_with(&else_ty) {
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
                            if found.conflicts_with(&expected) {
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
        if let PatternKind::Literal(literal) = &self.lowered.module.pattern(arm.pattern).kind {
            let span = self.lowered.source_map.pattern_span(arm.pattern);
            match super::ScalarValue::parse(literal) {
                Ok(value) => {
                    if value.ty().conflicts_with(scrutinee_ty) {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                                expected: display_type_id(scrutinee_ty),
                                found: display_type_id(&value.ty()),
                            })
                            .with_span(span),
                        );
                    }
                    self.type_table.insert_pattern_scalar(arm.pattern, value);
                }
                Err(reason) => self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidLiteral {
                        reason: reason.to_owned(),
                    })
                    .with_span(span),
                ),
            }
        }
        if let PatternKind::Name { local, .. } = self.lowered.module.pattern(arm.pattern).kind {
            arm_env.locals.insert(local, scrutinee_ty.clone());
            self.type_table.insert_local(local, scrutinee_ty.clone());
        }
        self.infer_expr_type(arm.expr, &mut arm_env)
    }

    fn infer_standard_call_type(
        &mut self,
        call_expr: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        if let Some((intrinsic, receiver, receiver_ty)) = self.standard_method(callee, env) {
            self.type_table.insert_call(
                call_expr,
                CallTarget::StandardIntrinsic(intrinsic),
                Some(receiver),
            );
            return Some(self.infer_standard_intrinsic_type(
                intrinsic,
                callee,
                Some(receiver_ty),
                args,
                env,
            ));
        }

        let intrinsic = self.standard_function(callee)?;
        self.type_table
            .insert_call(call_expr, CallTarget::StandardIntrinsic(intrinsic), None);
        Some(self.infer_standard_intrinsic_type(intrinsic, callee, None, args, env))
    }

    fn infer_standard_intrinsic_type(
        &mut self,
        intrinsic: StandardIntrinsic,
        callee: ExprId,
        receiver_ty: Option<TypeId>,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> TypeId {
        use StandardIntrinsic::*;

        let arg_tys = self.infer_call_args(args, env);
        let name = standard_intrinsic_name(intrinsic);
        let value_offset = usize::from(receiver_ty.is_none());
        let arity = receiver_ty.as_ref().map_or_else(
            || surface::standard_function_by_intrinsic(intrinsic).map(|spec| spec.arity),
            |_| surface::standard_method_by_intrinsic(intrinsic).map(|spec| spec.arity),
        );
        if let Some(expected) = arity {
            self.check_builtin_arity(name, expected, args.len(), callee);
        }

        match intrinsic {
            ArrayLen | ArrayIsEmpty | ArrayClear | ArrayPop => {
                let array_ty = receiver_ty.or_else(|| arg_tys.first().map(|(_, ty)| ty.clone()));
                let Some(TypeId::Array(element)) = array_ty else {
                    self.emit_standard_arg_error(name, "value", "array", callee, &array_ty);
                    return TypeId::Error;
                };
                match intrinsic {
                    ArrayLen => TypeId::Builtin(BuiltinType::USize),
                    ArrayIsEmpty => TypeId::Builtin(BuiltinType::Bool),
                    ArrayPop => option_type((*element).clone()),
                    ArrayClear => TypeId::Array(element),
                    _ => unreachable!(),
                }
            }
            ArrayGet | ArrayRemove => {
                let array_ty = receiver_ty.or_else(|| arg_tys.first().map(|(_, ty)| ty.clone()));
                let Some(TypeId::Array(element)) = array_ty else {
                    self.emit_standard_arg_error(name, "value", "array", callee, &array_ty);
                    return TypeId::Error;
                };
                self.check_arg_type(
                    name,
                    "index",
                    TypeId::Builtin(BuiltinType::USize),
                    value_offset,
                    &arg_tys,
                );
                option_type((*element).clone())
            }
            ArrayPush => {
                let array_ty = receiver_ty.or_else(|| arg_tys.first().map(|(_, ty)| ty.clone()));
                let Some(TypeId::Array(element)) = array_ty else {
                    self.emit_standard_arg_error(name, "value", "array", callee, &array_ty);
                    return TypeId::Error;
                };
                self.check_arg_type(name, "item", (*element).clone(), value_offset, &arg_tys);
                TypeId::Array(element)
            }
            ArrayInsert => {
                let array_ty = receiver_ty.or_else(|| arg_tys.first().map(|(_, ty)| ty.clone()));
                let Some(TypeId::Array(element)) = array_ty else {
                    self.emit_standard_arg_error(name, "value", "array", callee, &array_ty);
                    return TypeId::Error;
                };
                self.check_arg_type(
                    name,
                    "index",
                    TypeId::Builtin(BuiltinType::USize),
                    value_offset,
                    &arg_tys,
                );
                self.check_arg_type(name, "item", (*element).clone(), value_offset + 1, &arg_tys);
                TypeId::Array(element)
            }
            MapNew => TypeId::Map {
                key: Box::new(TypeId::Unknown),
                value: Box::new(TypeId::Unknown),
            },
            MapLen | MapIsEmpty | MapClear | MapKeys | MapValues | MapEntries => {
                let map_ty = receiver_ty.or_else(|| arg_tys.first().map(|(_, ty)| ty.clone()));
                let Some(TypeId::Map { key, value }) = map_ty else {
                    self.emit_standard_arg_error(name, "value", "Map<K, V>", callee, &map_ty);
                    return TypeId::Error;
                };
                self.check_standard_constraint(&key, StandardTypeConstraint::HashKey, env, callee);
                match intrinsic {
                    MapLen => TypeId::Builtin(BuiltinType::USize),
                    MapIsEmpty => TypeId::Builtin(BuiltinType::Bool),
                    MapClear => TypeId::Map { key, value },
                    MapKeys => TypeId::Array(key),
                    MapValues => TypeId::Array(value),
                    MapEntries => TypeId::Array(Box::new(TypeId::Tuple(vec![*key, *value]))),
                    _ => unreachable!(),
                }
            }
            MapContainsKey | MapGet | MapInsert | MapRemove => {
                let map_ty = receiver_ty.or_else(|| arg_tys.first().map(|(_, ty)| ty.clone()));
                let Some(TypeId::Map { key, value }) = map_ty else {
                    self.emit_standard_arg_error(name, "value", "Map<K, V>", callee, &map_ty);
                    return TypeId::Error;
                };
                self.check_standard_constraint(&key, StandardTypeConstraint::HashKey, env, callee);
                self.check_arg_type(name, "key", (*key).clone(), value_offset, &arg_tys);
                match intrinsic {
                    MapContainsKey => TypeId::Builtin(BuiltinType::Bool),
                    MapGet | MapRemove => option_type((*value).clone()),
                    MapInsert => {
                        self.check_arg_type(
                            name,
                            "item",
                            (*value).clone(),
                            value_offset + 1,
                            &arg_tys,
                        );
                        TypeId::Map { key, value }
                    }
                    _ => unreachable!(),
                }
            }
            SetNew => TypeId::Set(Box::new(TypeId::Unknown)),
            SetLen | SetIsEmpty | SetClear | SetToArray => {
                let set_ty = receiver_ty.or_else(|| arg_tys.first().map(|(_, ty)| ty.clone()));
                let Some(TypeId::Set(element)) = set_ty else {
                    self.emit_standard_arg_error(name, "value", "Set<T>", callee, &set_ty);
                    return TypeId::Error;
                };
                self.check_standard_constraint(
                    &element,
                    StandardTypeConstraint::HashKey,
                    env,
                    callee,
                );
                match intrinsic {
                    SetLen => TypeId::Builtin(BuiltinType::USize),
                    SetIsEmpty => TypeId::Builtin(BuiltinType::Bool),
                    SetClear => TypeId::Set(element),
                    SetToArray => TypeId::Array(element),
                    _ => unreachable!(),
                }
            }
            SetContains | SetInsert | SetRemove => {
                let set_ty = receiver_ty.or_else(|| arg_tys.first().map(|(_, ty)| ty.clone()));
                let Some(TypeId::Set(element)) = set_ty else {
                    self.emit_standard_arg_error(name, "value", "Set<T>", callee, &set_ty);
                    return TypeId::Error;
                };
                self.check_standard_constraint(
                    &element,
                    StandardTypeConstraint::HashKey,
                    env,
                    callee,
                );
                self.check_arg_type(name, "item", (*element).clone(), value_offset, &arg_tys);
                match intrinsic {
                    SetContains | SetRemove => TypeId::Builtin(BuiltinType::Bool),
                    SetInsert => TypeId::Set(element),
                    _ => unreachable!(),
                }
            }
            SetUnion | SetIntersection | SetDifference => {
                let set_ty = receiver_ty.or_else(|| arg_tys.first().map(|(_, ty)| ty.clone()));
                let Some(TypeId::Set(element)) = set_ty else {
                    self.emit_standard_arg_error(name, "lhs", "Set<T>", callee, &set_ty);
                    return TypeId::Error;
                };
                self.check_standard_constraint(
                    &element,
                    StandardTypeConstraint::HashKey,
                    env,
                    callee,
                );
                self.check_arg_type(
                    name,
                    "rhs",
                    TypeId::Set(element.clone()),
                    value_offset,
                    &arg_tys,
                );
                TypeId::Set(element)
            }
            StringLenBytes | StringLenChars => {
                self.check_string_receiver_or_arg(name, callee, receiver_ty, &arg_tys);
                TypeId::Builtin(BuiltinType::USize)
            }
            StringIsEmpty | StringContains | StringStartsWith | StringEndsWith => {
                self.check_string_receiver_or_arg(name, callee, receiver_ty, &arg_tys);
                if matches!(
                    intrinsic,
                    StringContains | StringStartsWith | StringEndsWith
                ) {
                    self.check_arg_type(
                        name,
                        "needle",
                        TypeId::Builtin(BuiltinType::String),
                        value_offset,
                        &arg_tys,
                    );
                }
                TypeId::Builtin(BuiltinType::Bool)
            }
            StringConcat => {
                self.check_string_receiver_or_arg(name, callee, receiver_ty, &arg_tys);
                self.check_arg_type(
                    name,
                    "rhs",
                    TypeId::Builtin(BuiltinType::String),
                    value_offset,
                    &arg_tys,
                );
                TypeId::Builtin(BuiltinType::String)
            }
            StringSlice => {
                self.check_string_receiver_or_arg(name, callee, receiver_ty, &arg_tys);
                self.check_arg_type(
                    name,
                    "start",
                    TypeId::Builtin(BuiltinType::USize),
                    value_offset,
                    &arg_tys,
                );
                self.check_arg_type(
                    name,
                    "end",
                    TypeId::Builtin(BuiltinType::USize),
                    value_offset + 1,
                    &arg_tys,
                );
                option_type(TypeId::Builtin(BuiltinType::String))
            }
            OptionIsSome | OptionIsNone | OptionUnwrapOr | OptionMap | OptionAndThen => {
                let option_ty = receiver_ty.or_else(|| arg_tys.first().map(|(_, ty)| ty.clone()));
                let Some((item_ty, _)) =
                    standard_enum_args(&option_ty, surface::StandardEnum::Option)
                else {
                    self.emit_standard_arg_error(name, "value", "Option<T>", callee, &option_ty);
                    return TypeId::Error;
                };
                match intrinsic {
                    OptionIsSome | OptionIsNone => TypeId::Builtin(BuiltinType::Bool),
                    OptionUnwrapOr => {
                        self.check_arg_type(
                            name,
                            "fallback",
                            item_ty.clone(),
                            value_offset,
                            &arg_tys,
                        );
                        item_ty
                    }
                    OptionMap | OptionAndThen => option_type(TypeId::Unknown),
                    _ => unreachable!(),
                }
            }
            ResultIsOk | ResultIsErr | ResultUnwrapOr | ResultMap | ResultMapErr
            | ResultAndThen => {
                let result_ty = receiver_ty.or_else(|| arg_tys.first().map(|(_, ty)| ty.clone()));
                let Some((ok_ty, err_ty)) =
                    standard_enum_args(&result_ty, surface::StandardEnum::Result)
                else {
                    self.emit_standard_arg_error(name, "value", "Result<T, E>", callee, &result_ty);
                    return TypeId::Error;
                };
                match intrinsic {
                    ResultIsOk | ResultIsErr => TypeId::Builtin(BuiltinType::Bool),
                    ResultUnwrapOr => {
                        self.check_arg_type(
                            name,
                            "fallback",
                            ok_ty.clone(),
                            value_offset,
                            &arg_tys,
                        );
                        ok_ty
                    }
                    ResultMap => result_type(TypeId::Unknown, err_ty),
                    ResultMapErr => result_type(ok_ty, TypeId::Unknown),
                    ResultAndThen => result_type(TypeId::Unknown, err_ty),
                    _ => unreachable!(),
                }
            }
            IterLen | IterIsEmpty | IterGet | IterToArray | IterForEach => {
                let iterable_ty = receiver_ty.or_else(|| arg_tys.first().map(|(_, ty)| ty.clone()));
                let Some(item_ty) = iterable_item_type(&iterable_ty) else {
                    self.emit_standard_arg_error(name, "value", "Iterable", callee, &iterable_ty);
                    return TypeId::Error;
                };
                match intrinsic {
                    IterLen => TypeId::Builtin(BuiltinType::USize),
                    IterIsEmpty => TypeId::Builtin(BuiltinType::Bool),
                    IterGet => {
                        self.check_arg_type(
                            name,
                            "index",
                            TypeId::Builtin(BuiltinType::USize),
                            value_offset,
                            &arg_tys,
                        );
                        option_type(item_ty)
                    }
                    IterToArray => TypeId::Array(Box::new(item_ty)),
                    IterForEach => TypeId::Builtin(BuiltinType::Unit),
                    _ => unreachable!(),
                }
            }
            MathMin | MathMax | MathClamp => {
                let Some((_, value_ty)) = arg_tys.first() else {
                    return TypeId::Error;
                };
                self.check_standard_constraint(
                    value_ty,
                    StandardTypeConstraint::OrderedNumber,
                    env,
                    callee,
                );
                for (index, (_, ty)) in arg_tys.iter().enumerate().skip(1) {
                    if ty != value_ty {
                        self.emit_arg_mismatch(name, &format!("arg{index}"), value_ty, ty, callee);
                    }
                }
                value_ty.clone()
            }
            MathAbs => {
                let Some((_, value_ty)) = arg_tys.first() else {
                    return TypeId::Error;
                };
                self.check_standard_constraint(
                    value_ty,
                    StandardTypeConstraint::SignedNumber,
                    env,
                    callee,
                );
                value_ty.clone()
            }
            MathFloor | MathCeil | MathRound | MathSqrt | MathSin | MathCos | MathTan => {
                self.check_arg_type(
                    name,
                    "value",
                    TypeId::Builtin(BuiltinType::F64),
                    0,
                    &arg_tys,
                );
                TypeId::Builtin(BuiltinType::F64)
            }
            DebugPrint | DebugPanic => {
                self.check_arg_type(
                    name,
                    "message",
                    TypeId::Builtin(BuiltinType::String),
                    0,
                    &arg_tys,
                );
                TypeId::Builtin(BuiltinType::Unit)
            }
            DebugAssert => {
                self.check_arg_type(
                    name,
                    "condition",
                    TypeId::Builtin(BuiltinType::Bool),
                    0,
                    &arg_tys,
                );
                self.check_arg_type(
                    name,
                    "message",
                    TypeId::Builtin(BuiltinType::String),
                    1,
                    &arg_tys,
                );
                TypeId::Builtin(BuiltinType::Unit)
            }
            DebugAssertEq => {
                if let Some((expr, lhs)) = arg_tys.first() {
                    self.check_standard_constraint(
                        lhs,
                        StandardTypeConstraint::Comparable,
                        env,
                        *expr,
                    );
                }
                if let (Some((_, lhs)), Some((_, rhs))) = (arg_tys.first(), arg_tys.get(1))
                    && lhs != rhs
                {
                    self.emit_arg_mismatch(name, "rhs", lhs, rhs, callee);
                }
                self.check_arg_type(
                    name,
                    "message",
                    TypeId::Builtin(BuiltinType::String),
                    2,
                    &arg_tys,
                );
                TypeId::Builtin(BuiltinType::Unit)
            }
        }
    }

    fn infer_runtime_helper_call_type(
        &mut self,
        call_expr: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        let builtin = self.builtin_function(callee)?;
        self.type_table
            .insert_call(call_expr, CallTarget::RuntimeHelper(builtin), None);
        let arity = match builtin {
            BuiltinFunction::TypeOf => 1,
            BuiltinFunction::Print => kagari_common::host_interface::standard_log().params.len(),
            BuiltinFunction::GetField => 2,
            BuiltinFunction::SetField | BuiltinFunction::SetIndex => 3,
        };
        if args.len() != arity {
            self.infer_call_args(args, env);
            self.check_builtin_arity("runtime helper", arity, args.len(), callee);
            return Some(TypeId::Error);
        }
        match builtin {
            BuiltinFunction::TypeOf => {
                let _ = self.infer_call_args(args, env);
                Some(TypeId::Builtin(BuiltinType::String))
            }
            BuiltinFunction::GetField => {
                let [base, field_name_expr] = args else {
                    return Some(TypeId::Error);
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
                    return Some(TypeId::Error);
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
                    return Some(TypeId::Error);
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
                let declaration = kagari_common::host_interface::standard_log();
                let arg_tys = self.infer_call_args(args, env);
                for ((arg, ty), parameter) in arg_tys.iter().zip(&declaration.params) {
                    let expected = crate::host::scalar_type(&parameter.ty)
                        .expect("standard log has a scalar signature");
                    if *ty != expected {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::ArgumentTypeMismatch {
                                function_name: "print".to_owned(),
                                parameter_name: parameter.name.clone(),
                                expected: display_type_id(&expected),
                                found: display_type_id(ty),
                            })
                            .with_span(self.lowered.source_map.expr_span(*arg)),
                        );
                    }
                }
                crate::host::scalar_type(&declaration.return_type)
            }
        }
    }

    fn infer_trait_method_call_type(
        &mut self,
        call_expr: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        let expr = self.lowered.module.expr(callee);
        let ExprKind::Field { receiver, name } = &expr.kind else {
            return None;
        };
        let receiver_ty = self.infer_expr_type(*receiver, env);
        let (trait_id, self_ty) = match &receiver_ty {
            TypeId::Trait(definition) => {
                let ResolvedName::Trait(id) = self.declarations.definition_target(definition)?
                else {
                    return None;
                };
                (id, receiver_ty.clone())
            }
            TypeId::Generic(generic_name) => {
                let trait_id = env.generic_bounds.get(generic_name).and_then(|bounds| {
                    bounds.iter().find_map(|bound| {
                        let super::ConstraintTarget::Trait(id) = bound else {
                            return None;
                        };
                        self.trait_method_function(*id, name).map(|_| *id)
                    })
                })?;
                (trait_id, receiver_ty.clone())
            }
            _ => return None,
        };

        let method_function = self.trait_method_function(trait_id, name)?;
        let self_owner = self
            .declarations
            .definition(ResolvedName::Trait(trait_id))?;
        self.type_table.insert_call(
            call_expr,
            CallTarget::TraitMethod(method_function),
            Some(*receiver),
        );
        let Some(method) = self.function_index.by_id.get(&method_function) else {
            return Some(TypeId::Error);
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
                let expected = param.ty.with_self(self_owner, &self_ty);
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

        Some(method.return_type.with_self(self_owner, &self_ty))
    }

    fn trait_method_function(
        &self,
        trait_id: crate::hir::TraitId,
        method_name: &str,
    ) -> Option<crate::hir::FunctionId> {
        self.lowered
            .module
            .traits
            .iter()
            .find(|trait_def| trait_def.id == trait_id)
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
        call_expr: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> TypeId {
        let arg_tys = self.infer_call_args(args, env);
        let Some(ResolvedName::Function(id)) = self.names.expr_resolution(callee) else {
            let callee_ty = self.infer_expr_type(callee, env);
            if !callee_ty.is_unresolved() {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidCallTarget {
                        type_name: display_type_id(&callee_ty),
                    })
                    .with_span(self.lowered.source_map.expr_span(callee)),
                );
            }
            return TypeId::Error;
        };
        let Some(function) = self.function_index.by_id.get(&id) else {
            return self.infer_expr_type(callee, env);
        };
        self.type_table
            .insert_call(call_expr, CallTarget::Function(id), None);
        let mut substitution = crate::types::TypeSubstitution::new();
        for (parameter, (_, actual)) in function.params.iter().zip(&arg_tys) {
            super::inference::infer(
                &parameter.ty,
                actual,
                &function.generic_params,
                &mut substitution,
            );
        }
        let type_arguments = function
            .generic_params
            .iter()
            .map(|parameter| {
                substitution.get(parameter).cloned().unwrap_or_else(|| {
                    if !arg_tys.iter().any(|(_, ty)| ty.is_unresolved()) {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::CannotInferGenericArgument {
                                function_name: function.name.clone(),
                                parameter: parameter.name.clone(),
                            })
                            .with_span(self.lowered.source_map.expr_span(callee)),
                        );
                    }
                    TypeId::Error
                })
            })
            .collect::<Vec<_>>();
        self.type_table
            .insert_type_arguments(call_expr, type_arguments);
        let declaration = self
            .lowered
            .module
            .functions
            .iter()
            .find(|function| function.id == id)
            .expect("resolved function declaration");
        let bounds = super::constraints::function_bounds(
            &self.lowered.module,
            declaration,
            self.declarations,
            self.type_table,
        );
        for parameter in &function.generic_params {
            let Some(actual) = substitution.get(parameter) else {
                continue;
            };
            for constraint in bounds.get(parameter).into_iter().flatten().copied() {
                match constraint {
                    super::ConstraintTarget::Standard(constraint) => {
                        self.check_standard_constraint(actual, constraint, env, callee)
                    }
                    super::ConstraintTarget::Trait(trait_id) => {
                        let satisfied = match actual {
                            TypeId::Generic(parameter) => {
                                env.generic_bounds.get(parameter).is_some_and(|bounds| {
                                    bounds.contains(&super::ConstraintTarget::Trait(trait_id))
                                })
                            }
                            _ => self.type_table.implements(trait_id, actual),
                        };
                        if !satisfied && !actual.is_unresolved() {
                            let trait_name = self
                                .lowered
                                .module
                                .traits
                                .iter()
                                .find(|item| item.id == trait_id)
                                .expect("resolved trait")
                                .name
                                .clone();
                            self.diagnostics.push(
                                Diagnostic::error(DiagnosticKind::GenericBoundNotSatisfied {
                                    type_name: actual.display_name(),
                                    trait_name,
                                })
                                .with_span(self.lowered.source_map.expr_span(callee)),
                            );
                        }
                    }
                }
            }
        }
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
                && param.ty.instantiate(&substitution).conflicts_with(arg_ty)
            {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::ArgumentTypeMismatch {
                        function_name: function.name.clone(),
                        parameter_name: param.name.clone(),
                        expected: display_type_id(&param.ty.instantiate(&substitution)),
                        found: display_type_id(arg_ty),
                    })
                    .with_span(self.lowered.source_map.expr_span(*arg_expr)),
                );
            }
        }
        function.return_type.instantiate(&substitution)
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
            .and_then(|field| self.type_table.field_type(field.id))
    }

    fn resolve_field(&self, receiver: &TypeId, field_name: &str) -> Option<&crate::hir::Field> {
        match receiver {
            TypeId::Struct(name) => self
                .lowered
                .module
                .structs
                .iter()
                .find(|item| {
                    self.declarations.definition(ResolvedName::Struct(item.id)) == Some(name)
                })
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
        env: &BodyTypeEnv,
    ) -> TypeId {
        if lhs_ty.is_unresolved() || rhs_ty.is_unresolved() {
            return TypeId::Error;
        }
        match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                if !self.matching_numeric_operands(&lhs_ty, &rhs_ty, env) {
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
                if lhs_ty != rhs_ty
                    || !type_satisfies_standard_constraint(
                        &lhs_ty,
                        StandardTypeConstraint::Comparable,
                        env,
                    )
                {
                    self.emit_binary_operand_type_mismatch(
                        op, "matching", &lhs_ty, &rhs_ty, rhs_expr,
                    );
                }
                TypeId::Builtin(BuiltinType::Bool)
            }
            BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
                if !self.matching_numeric_operands(&lhs_ty, &rhs_ty, env) {
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

    fn matching_numeric_operands(&self, lhs: &TypeId, rhs: &TypeId, env: &BodyTypeEnv) -> bool {
        surface::supports_arithmetic(lhs, rhs)
            || (lhs == rhs
                && matches!(lhs, TypeId::Generic(_))
                && type_satisfies_standard_constraint(
                    lhs,
                    StandardTypeConstraint::OrderedNumber,
                    env,
                ))
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
        if ty.conflicts_with(&TypeId::Builtin(BuiltinType::Bool)) {
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
            return TypeId::Error;
        };

        let mut seen = HashSet::new();
        let resolved = super::ResolvedStructInit {
            structure: struct_def.id,
            fields: fields
                .iter()
                .map(|init| {
                    struct_def
                        .fields
                        .iter()
                        .find(|field| field.name == init.name)
                        .map(|field| field.id)
                })
                .collect(),
        };
        for ((name, value_expr, value_ty), target) in field_tys.iter().zip(&resolved.fields) {
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

            let Some(field) = target else {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidStructInitializer {
                        struct_name: path.to_owned(),
                        reason: format!("unknown field `{name}`"),
                    })
                    .with_span(self.lowered.source_map.expr_span(*value_expr)),
                );
                continue;
            };

            let Some(expected) = self.type_table.field_type(*field) else {
                continue;
            };
            if expected.conflicts_with(value_ty) {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::AssignmentTypeMismatch {
                        expected: display_type_id(&expected),
                        found: display_type_id(value_ty),
                    })
                    .with_span(self.lowered.source_map.expr_span(*value_expr)),
                );
            }
        }

        self.type_table.insert_struct_init(expr_id, resolved);
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

        self.declarations
            .definition(ResolvedName::Struct(struct_def.id))
            .cloned()
            .map(TypeId::Struct)
            .unwrap_or(TypeId::Error)
    }

    fn resolve_index_type(&self, index_expr: ExprId, receiver: &TypeId) -> Option<TypeId> {
        if !self.type_table.expr_type(index_expr)?.is_integer() {
            return None;
        }
        match receiver {
            TypeId::Array(element) => Some((**element).clone()),
            TypeId::Tuple(elements) => self
                .tuple_index(index_expr)
                .and_then(|index| elements.get(index).cloned()),
            _ => None,
        }
    }

    fn tuple_index(&self, index_expr: ExprId) -> Option<usize> {
        match self.type_table.scalar_value(index_expr)? {
            super::ScalarValue::I32(value) => usize::try_from(*value).ok(),
            _ => None,
        }
    }
    fn standard_function(&self, expr_id: ExprId) -> Option<StandardIntrinsic> {
        let expr = self.lowered.module.expr(expr_id);
        let ExprKind::Name(name) = &expr.kind else {
            return None;
        };
        if let Some(ResolvedName::StandardFunction(intrinsic)) = self.names.expr_resolution(expr_id)
        {
            return Some(intrinsic);
        }
        self.standard_function_path(name)
    }

    fn standard_function_path(&self, name: &str) -> Option<StandardIntrinsic> {
        if let Some((module_alias, function_name)) = name.rsplit_once("::")
            && let Some(module) = self.standard_module_path(module_alias)
        {
            return surface::standard_function(module, function_name)
                .map(|function| function.intrinsic);
        }
        None
    }

    fn standard_module_path(&self, path: &str) -> Option<surface::StandardModule> {
        if let Some(module) = surface::standard_module(path).map(|module| module.kind) {
            return Some(module);
        }
        if !path.contains("::")
            && let Some(ResolvedName::StandardModule(module)) = self
                .names
                .items
                .standard_modules
                .get(path)
                .copied()
                .map(ResolvedName::StandardModule)
        {
            return Some(module);
        }
        None
    }

    fn standard_method(
        &mut self,
        expr_id: ExprId,
        env: &mut BodyTypeEnv,
    ) -> Option<(StandardIntrinsic, ExprId, TypeId)> {
        let expr = self.lowered.module.expr(expr_id);
        let ExprKind::Field { receiver, name } = &expr.kind else {
            return None;
        };
        let receiver_ty = self.infer_expr_type(*receiver, env);
        let receiver_kind = standard_method_receiver(&receiver_ty)?;
        surface::standard_method(receiver_kind, name)
            .map(|method| (method.intrinsic, *receiver, receiver_ty))
    }

    fn check_arg_type(
        &mut self,
        function_name: &str,
        parameter_name: &str,
        expected: TypeId,
        index: usize,
        args: &[(ExprId, TypeId)],
    ) {
        let Some((arg_expr, found)) = args.get(index) else {
            return;
        };
        if *found != expected {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::ArgumentTypeMismatch {
                    function_name: function_name.to_owned(),
                    parameter_name: parameter_name.to_owned(),
                    expected: display_type_id(&expected),
                    found: display_type_id(found),
                })
                .with_span(self.lowered.source_map.expr_span(*arg_expr)),
            );
        }
    }

    fn emit_arg_mismatch(
        &mut self,
        function_name: &str,
        parameter_name: &str,
        expected: &TypeId,
        found: &TypeId,
        span_expr: ExprId,
    ) {
        self.diagnostics.push(
            Diagnostic::error(DiagnosticKind::ArgumentTypeMismatch {
                function_name: function_name.to_owned(),
                parameter_name: parameter_name.to_owned(),
                expected: display_type_id(expected),
                found: display_type_id(found),
            })
            .with_span(self.lowered.source_map.expr_span(span_expr)),
        );
    }

    fn emit_standard_arg_error(
        &mut self,
        function_name: &str,
        parameter_name: &str,
        expected: &str,
        span_expr: ExprId,
        found: &Option<TypeId>,
    ) {
        self.diagnostics.push(
            Diagnostic::error(DiagnosticKind::ArgumentTypeMismatch {
                function_name: function_name.to_owned(),
                parameter_name: parameter_name.to_owned(),
                expected: expected.to_owned(),
                found: found
                    .as_ref()
                    .map(display_type_id)
                    .unwrap_or_else(|| "<missing>".to_owned()),
            })
            .with_span(self.lowered.source_map.expr_span(span_expr)),
        );
    }

    fn check_string_receiver_or_arg(
        &mut self,
        function_name: &str,
        callee: ExprId,
        receiver_ty: Option<TypeId>,
        args: &[(ExprId, TypeId)],
    ) {
        let found = receiver_ty.or_else(|| args.first().map(|(_, ty)| ty.clone()));
        if found != Some(TypeId::Builtin(BuiltinType::String)) {
            self.emit_standard_arg_error(function_name, "value", "String", callee, &found);
        }
    }

    fn check_standard_constraint(
        &mut self,
        ty: &TypeId,
        constraint: StandardTypeConstraint,
        env: &BodyTypeEnv,
        span_expr: ExprId,
    ) {
        if type_satisfies_standard_constraint(ty, constraint, env) {
            return;
        }
        self.diagnostics.push(
            Diagnostic::error(DiagnosticKind::StandardConstraintNotSatisfied {
                type_name: display_type_id(ty),
                constraint: surface::standard_constraint_name(constraint).to_owned(),
                reason: standard_constraint_reason(constraint).to_owned(),
            })
            .with_span(self.lowered.source_map.expr_span(span_expr)),
        );
    }

    fn builtin_function(&self, expr_id: ExprId) -> Option<BuiltinFunction> {
        if self.names.expr_resolution(expr_id).is_some() {
            return None;
        }
        let expr = self.lowered.module.expr(expr_id);
        let ExprKind::Name(name) = &expr.kind else {
            return None;
        };
        BuiltinFunction::from_name(name)
    }

    fn string_literal_value(&self, expr_id: ExprId) -> Option<String> {
        match self.type_table.scalar_value(expr_id)? {
            super::ScalarValue::String(value) => Some(value.clone()),
            _ => None,
        }
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

fn standard_method_receiver(ty: &TypeId) -> Option<StandardMethodReceiver> {
    match ty {
        TypeId::Array(_) => Some(StandardMethodReceiver::Array),
        TypeId::Map { .. } => Some(StandardMethodReceiver::Map),
        TypeId::Set(_) => Some(StandardMethodReceiver::Set),
        TypeId::Builtin(BuiltinType::String) => Some(StandardMethodReceiver::String),
        TypeId::StandardEnum {
            kind: surface::StandardEnum::Option,
            ..
        } => Some(StandardMethodReceiver::Option),
        TypeId::StandardEnum {
            kind: surface::StandardEnum::Result,
            ..
        } => Some(StandardMethodReceiver::Result),
        _ if surface::iterable_protocol(ty).is_some() => Some(StandardMethodReceiver::Iterable),
        _ => None,
    }
}

fn option_type(item: TypeId) -> TypeId {
    TypeId::StandardEnum {
        kind: surface::StandardEnum::Option,
        args: vec![item],
    }
}

fn result_type(ok: TypeId, err: TypeId) -> TypeId {
    TypeId::StandardEnum {
        kind: surface::StandardEnum::Result,
        args: vec![ok, err],
    }
}

fn standard_enum_args(
    ty: &Option<TypeId>,
    expected: surface::StandardEnum,
) -> Option<(TypeId, TypeId)> {
    let Some(TypeId::StandardEnum { kind, args }) = ty else {
        return None;
    };
    if *kind != expected || args.len() != expected.spec().arity {
        return None;
    }
    let first = args.first()?.clone();
    let second = args
        .get(1)
        .cloned()
        .unwrap_or(TypeId::Builtin(BuiltinType::Unit));
    Some((first, second))
}

fn iterable_item_type(ty: &Option<TypeId>) -> Option<TypeId> {
    match surface::iterable_protocol(ty.as_ref()?)? {
        surface::IterableProtocol::Array { item } => Some(item),
        surface::IterableProtocol::Map { key, value } => Some(TypeId::Tuple(vec![key, value])),
        surface::IterableProtocol::Set { item } => Some(item),
        surface::IterableProtocol::String { .. } => Some(TypeId::Builtin(BuiltinType::String)),
    }
}

fn type_satisfies_standard_constraint(
    ty: &TypeId,
    constraint: StandardTypeConstraint,
    env: &BodyTypeEnv,
) -> bool {
    match ty {
        TypeId::Tuple(members) | TypeId::StandardEnum { args: members, .. }
            if constraint == StandardTypeConstraint::Comparable =>
        {
            members
                .iter()
                .all(|ty| type_satisfies_standard_constraint(ty, constraint, env))
        }
        TypeId::Generic(name) => env
            .generic_bounds
            .get(name)
            .is_some_and(|bounds| bounds.contains(&super::ConstraintTarget::Standard(constraint))),
        _ => match constraint {
            StandardTypeConstraint::HashKey => surface::supports_hash_key(ty),
            StandardTypeConstraint::Iterable => surface::iterable_protocol(ty).is_some(),
            StandardTypeConstraint::OrderedNumber => surface::supports_ordering(ty, ty),
            StandardTypeConstraint::SignedNumber => surface::supports_unary_negation(ty),
            StandardTypeConstraint::Comparable => ty.supports_equality(),
        },
    }
}

fn standard_constraint_reason(constraint: StandardTypeConstraint) -> &'static str {
    match constraint {
        StandardTypeConstraint::HashKey => {
            "only bool, integer, and String keys have specified hash semantics"
        }
        StandardTypeConstraint::Iterable => "type is not part of the standard iterable protocol",
        StandardTypeConstraint::OrderedNumber => "type is not an ordered numeric type",
        StandardTypeConstraint::SignedNumber => "type is not a signed numeric type",
        StandardTypeConstraint::Comparable => "type does not have standard equality semantics",
    }
}

fn standard_intrinsic_name(intrinsic: StandardIntrinsic) -> &'static str {
    use StandardIntrinsic::*;

    match intrinsic {
        ArrayLen => "std::array::len",
        ArrayIsEmpty => "std::array::is_empty",
        ArrayGet => "std::array::get",
        ArrayPush => "std::array::push",
        ArrayPop => "std::array::pop",
        ArrayInsert => "std::array::insert",
        ArrayRemove => "std::array::remove",
        ArrayClear => "std::array::clear",
        MapNew => "std::map::new",
        MapLen => "std::map::len",
        MapIsEmpty => "std::map::is_empty",
        MapContainsKey => "std::map::contains_key",
        MapGet => "std::map::get",
        MapInsert => "std::map::insert",
        MapRemove => "std::map::remove",
        MapClear => "std::map::clear",
        MapKeys => "std::map::keys",
        MapValues => "std::map::values",
        MapEntries => "std::map::entries",
        SetNew => "std::set::new",
        SetLen => "std::set::len",
        SetIsEmpty => "std::set::is_empty",
        SetContains => "std::set::contains",
        SetInsert => "std::set::insert",
        SetRemove => "std::set::remove",
        SetClear => "std::set::clear",
        SetToArray => "std::set::to_array",
        SetUnion => "std::set::union",
        SetIntersection => "std::set::intersection",
        SetDifference => "std::set::difference",
        StringLenBytes => "std::string::len_bytes",
        StringLenChars => "std::string::len_chars",
        StringIsEmpty => "std::string::is_empty",
        StringConcat => "std::string::concat",
        StringContains => "std::string::contains",
        StringStartsWith => "std::string::starts_with",
        StringEndsWith => "std::string::ends_with",
        StringSlice => "std::string::slice",
        OptionIsSome => "std::option::is_some",
        OptionIsNone => "std::option::is_none",
        OptionUnwrapOr => "std::option::unwrap_or",
        OptionMap => "std::option::map",
        OptionAndThen => "std::option::and_then",
        ResultIsOk => "std::result::is_ok",
        ResultIsErr => "std::result::is_err",
        ResultUnwrapOr => "std::result::unwrap_or",
        ResultMap => "std::result::map",
        ResultMapErr => "std::result::map_err",
        ResultAndThen => "std::result::and_then",
        IterLen => "std::iter::len",
        IterIsEmpty => "std::iter::is_empty",
        IterGet => "std::iter::get",
        IterToArray => "std::iter::to_array",
        IterForEach => "std::iter::for_each",
        MathMin => "std::math::min",
        MathMax => "std::math::max",
        MathClamp => "std::math::clamp",
        MathAbs => "std::math::abs",
        MathFloor => "std::math::floor",
        MathCeil => "std::math::ceil",
        MathRound => "std::math::round",
        MathSqrt => "std::math::sqrt",
        MathSin => "std::math::sin",
        MathCos => "std::math::cos",
        MathTan => "std::math::tan",
        DebugPrint => "std::debug::print",
        DebugAssert => "std::debug::assert",
        DebugAssertEq => "std::debug::assert_eq",
        DebugPanic => "std::debug::panic",
    }
}
