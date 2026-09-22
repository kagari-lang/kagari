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
    aggregates: &'a crate::aggregates::AggregateCatalog,
    imported_functions: &'a crate::imports::ImportedFunctions,
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
            imported_functions: indexes.imported_functions,
            aggregates: indexes.aggregates,
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
        self.infer_block_types_expected(block_id, env, None)
    }

    pub(crate) fn infer_block_types_expected(
        &mut self,
        block_id: BlockId,
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> TypeId {
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
                self.infer_expr_type_expected(expr, env, expected)
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
                let annotation = ty.map(|ty| {
                    let resolved = resolve_type_in(
                        &self.lowered.module,
                        ty,
                        TypeContext {
                            declarations: self.declarations,
                            generics: &env.generics,
                            self_type: None,
                        },
                        self.type_table,
                        self.cancel,
                    );
                    if resolved.is_unresolved() {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::UnknownTypeAnnotation {
                                type_name: display_type(&self.lowered.module, ty),
                            })
                            .with_span(self.lowered.source_map.type_span(ty)),
                        );
                    }
                    super::check::validate_standard_type_constraints(
                        &resolved,
                        &env.generic_bounds,
                        self.lowered.source_map.type_span(ty),
                        self.diagnostics,
                        self.cancel,
                    );
                    resolved
                });
                let initializer_ty =
                    self.infer_expr_type_expected(*initializer, env, annotation.as_ref());
                let local_ty = annotation.unwrap_or_else(|| initializer_ty.clone());
                super::applications::validate(
                    &local_ty,
                    &env.generic_bounds,
                    self.aggregates,
                    self.type_table,
                    self.lowered.source_map.stmt_span(stmt_id),
                    self.diagnostics,
                    self.cancel,
                );
                let Ok(completes) = super::completion::expr_can_complete(
                    &self.lowered.module,
                    *initializer,
                    self.cancel,
                ) else {
                    return;
                };
                if completes && local_ty.conflicts_with(&initializer_ty) {
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
                let value_ty = self.infer_expr_type_expected(*value, env, target_ty.as_ref());
                let Ok(completes) =
                    super::completion::expr_can_complete(&self.lowered.module, *value, self.cancel)
                else {
                    return;
                };
                if completes && let (Some(op), Some(expected)) = (op, &target_ty) {
                    self.infer_binary_type(
                        *op,
                        *value,
                        Some(expected.clone()),
                        Some(value_ty.clone()),
                        env,
                    );
                }
                match target_ty {
                    Some(expected) if completes && expected.conflicts_with(&value_ty) => {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::AssignmentTypeMismatch {
                                expected: display_type_id(&expected),
                                found: display_type_id(&value_ty),
                            })
                            .with_span(self.lowered.source_map.place_span(*target)),
                        )
                    }
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
                let expected = self.expected_return.clone();
                let found = expr.map_or(TypeId::Builtin(BuiltinType::Unit), |expr| {
                    self.infer_expr_type_expected(expr, env, Some(&expected))
                });
                if let Some(expr) = expr {
                    let Ok(completes) = super::completion::expr_can_complete(
                        &self.lowered.module,
                        *expr,
                        self.cancel,
                    ) else {
                        return;
                    };
                    if !completes {
                        return;
                    }
                }
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
                if self.check_condition_type(*condition, "while", env).is_err() {
                    return;
                }
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
        if let Some(ty) = self.infer_host_field_write(place_id, env) {
            self.type_table.insert_place(place_id, ty.clone());
            return Some(ty);
        }
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
                        | ResolvedName::SourceItem { .. }
                        | ResolvedName::SourceImport(_)
                        | ResolvedName::HostType(_)
                        | ResolvedName::HostModule(_)
                        | ResolvedName::Module(_)
                        | ResolvedName::StandardModule(_)
                        | ResolvedName::HostFunction(_)
                        | ResolvedName::StandardFunction(_)
                        | ResolvedName::RuntimeHelper(_)
                        | ResolvedName::Struct(_)
                        | ResolvedName::Enum(_)
                        | ResolvedName::Trait(_) => None,
                    })
            }
            PlaceKind::Field { base, name } => {
                let base_ty = self.resolve_readable_place_type(*base, env)?;
                let field = self.resolve_field(&base_ty, name)?;
                let id = field.id.clone();
                let ty = field.ty.clone();
                let writable = field.writeability.is_var();
                self.type_table.insert_place_field(place_id, id);
                writable.then_some(ty)
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
                        | ResolvedName::SourceItem { .. }
                        | ResolvedName::SourceImport(_)
                        | ResolvedName::HostType(_)
                        | ResolvedName::HostModule(_)
                        | ResolvedName::Module(_)
                        | ResolvedName::StandardModule(_)
                        | ResolvedName::HostFunction(_)
                        | ResolvedName::StandardFunction(_)
                        | ResolvedName::RuntimeHelper(_)
                        | ResolvedName::Struct(_)
                        | ResolvedName::Enum(_)
                        | ResolvedName::Trait(_) => None,
                    })
            }
            PlaceKind::Field { base, name } => {
                let base_ty = self.resolve_readable_place_type(*base, env)?;
                let field = self.resolve_field(&base_ty, name)?;
                let (id, ty) = (field.id.clone(), field.ty.clone());
                self.type_table.insert_place_field(place_id, id);
                Some(ty)
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
                    ResolvedName::HostFunction(_) | ResolvedName::Function(_) => {
                        "function item is not assignable".to_string()
                    }
                    ResolvedName::SourceItem { .. }
                    | ResolvedName::SourceImport(_)
                    | ResolvedName::HostType(_)
                    | ResolvedName::HostModule(_)
                    | ResolvedName::Module(_) => "module item is not assignable".to_string(),
                    ResolvedName::StandardModule(_) => {
                        "standard module item is not assignable".to_string()
                    }
                    ResolvedName::StandardFunction(_) | ResolvedName::RuntimeHelper(_) => {
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
        self.infer_expr_type_expected(expr_id, env, None)
    }

    fn infer_expr_type_expected(
        &mut self,
        expr_id: ExprId,
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> TypeId {
        if self.cancel.check().is_err() {
            return TypeId::Unknown;
        }
        if let Some(ty) = env.exprs.get(&expr_id).cloned() {
            return ty;
        }

        if let Some(ty) = self.infer_host_field_read(expr_id, env) {
            env.exprs.insert(expr_id, ty.clone());
            self.type_table.insert_expr(expr_id, ty.clone());
            return ty;
        }
        let expr = self.lowered.module.expr(expr_id);
        let mut ty = match &expr.kind {
            ExprKind::Missing => {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::ExpectedExpression)
                        .with_span(self.lowered.source_map.expr_span(expr_id)),
                );
                TypeId::Unknown
            }
            ExprKind::Name { explicit_type, .. }
                if explicit_type.is_some() || self.enum_member_owner(expr_id).is_some() =>
            {
                self.infer_enum_constructor(expr_id, expr_id, &[], env, expected)
                    .expect("resolved enum member owner")
            }
            ExprKind::Name { name, .. } => self
                .names
                .expr_resolution(expr_id)
                .and_then(|resolved| match resolved {
                    ResolvedName::Param(id) => env.params.get(&id).cloned(),
                    ResolvedName::Local(id) => env.locals.get(&id).cloned(),
                    ResolvedName::Const(id) => self.top_level_index.consts.get(&id).cloned(),
                    ResolvedName::Function(_)
                    | ResolvedName::RuntimeHelper(_)
                    | ResolvedName::SourceItem { .. }
                    | ResolvedName::SourceImport(_)
                    | ResolvedName::HostType(_)
                    | ResolvedName::HostModule(_)
                    | ResolvedName::Module(_)
                    | ResolvedName::StandardModule(_)
                    | ResolvedName::HostFunction(_)
                    | ResolvedName::StandardFunction(_)
                    | ResolvedName::Struct(_)
                    | ResolvedName::Enum(_)
                    | ResolvedName::Trait(_) => None,
                })
                .unwrap_or_else(|| {
                    self.diagnostics.push(
                        Diagnostic::error(if self.names.expr_resolution(expr_id).is_some() {
                            DiagnosticKind::InvalidValueTarget { name: name.clone() }
                        } else {
                            DiagnosticKind::UnknownName { name: name.clone() }
                        })
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
                let Ok(completes) =
                    super::completion::expr_can_complete(&self.lowered.module, *expr, self.cancel)
                else {
                    return TypeId::Unknown;
                };
                match op {
                    PrefixOp::Neg => {
                        if completes
                            && super::constraints::known_type_violates_constraint(
                                &inner,
                                StandardTypeConstraint::SignedNumber,
                                &env.generic_bounds,
                            )
                        {
                            self.diagnostics.push(
                                Diagnostic::error(DiagnosticKind::UnaryOperandTypeMismatch {
                                    operator: "-",
                                    expected: "numeric".to_owned(),
                                    found: display_type_id(&inner),
                                })
                                .with_span(self.lowered.source_map.expr_span(*expr)),
                            );
                        }
                        if completes { inner } else { TypeId::Unknown }
                    }
                    PrefixOp::Not => {
                        if completes && inner.conflicts_with(&TypeId::Builtin(BuiltinType::Bool)) {
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
                let Ok(lhs_completes) =
                    super::completion::expr_can_complete(&self.lowered.module, *lhs, self.cancel)
                else {
                    return TypeId::Unknown;
                };
                let lhs_ty = lhs_completes.then_some(lhs_ty);
                let rhs_context = match op {
                    BinaryOp::AndAnd | BinaryOp::OrOr => Some(TypeId::Builtin(BuiltinType::Bool)),
                    _ => lhs_ty.clone(),
                };
                let rhs_ty = self.infer_expr_type_expected(*rhs, env, rhs_context.as_ref());
                let Ok(rhs_completes) =
                    super::completion::expr_can_complete(&self.lowered.module, *rhs, self.cancel)
                else {
                    return TypeId::Unknown;
                };
                self.infer_binary_type(*op, *rhs, lhs_ty, rhs_completes.then_some(rhs_ty), env)
            }
            ExprKind::Call { callee, args } => {
                if let Some(ty) = self.infer_enum_constructor(expr_id, *callee, args, env, expected)
                {
                    ty
                } else if let Some(ty) = self.infer_host_call_type(expr_id, *callee, args, env) {
                    ty
                } else if let Some(ty) = self.infer_host_method_call(expr_id, *callee, args, env) {
                    ty
                } else if let Some(standard_ty) =
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
                    self.infer_function_call_type(expr_id, *callee, args, env, expected)
                }
            }
            ExprKind::Field { receiver, name } => {
                let receiver_ty = self.infer_expr_type(*receiver, env);
                if name.is_empty() {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::ExpectedFieldName)
                            .with_span(self.lowered.source_map.expr_span(expr_id)),
                    );
                    TypeId::Error
                } else {
                    self.checked_member_type(&receiver_ty, name, expr_id, false)
                }
            }
            ExprKind::Index { receiver, index } => {
                let receiver_ty = self.infer_expr_type(*receiver, env);
                let index_ty = self.infer_expr_type(*index, env);
                self.checked_index_type(*index, &receiver_ty, &index_ty, expr_id)
                    .unwrap_or(TypeId::Error)
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let Ok(condition_completes) = self.check_condition_type(*condition, "if", env)
                else {
                    return TypeId::Unknown;
                };
                let mut then_ty = self.infer_block_types_expected(*then_branch, env, expected);
                match else_branch {
                    Some(else_expr) => {
                        let Ok(then_completes) = super::completion::block_can_complete(
                            &self.lowered.module,
                            *then_branch,
                            self.cancel,
                        ) else {
                            return TypeId::Unknown;
                        };
                        let else_context = expected
                            .or((condition_completes && then_completes).then_some(&then_ty));
                        let else_ty = self.infer_expr_type_expected(*else_expr, env, else_context);
                        let Ok(else_completes) = super::completion::expr_can_complete(
                            &self.lowered.module,
                            *else_expr,
                            self.cancel,
                        ) else {
                            return TypeId::Unknown;
                        };
                        if !condition_completes {
                            TypeId::Unknown
                        } else {
                            if !then_completes {
                                then_ty = else_ty;
                            } else if else_completes && then_ty.conflicts_with(&else_ty) {
                                self.diagnostics.push(
                                    Diagnostic::error(DiagnosticKind::IfBranchTypeMismatch {
                                        expected: display_type_id(&then_ty),
                                        found: display_type_id(&else_ty),
                                    })
                                    .with_span(self.lowered.source_map.expr_span(*else_expr)),
                                );
                            } else if else_completes {
                                then_ty.recover_from(&else_ty);
                            }
                            then_ty
                        }
                    }
                    None if condition_completes => TypeId::Builtin(BuiltinType::Unit),
                    None => TypeId::Unknown,
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                let scrutinee_ty = self.infer_expr_type(*scrutinee, env);
                let Ok(scrutinee_completes) = super::completion::expr_can_complete(
                    &self.lowered.module,
                    *scrutinee,
                    self.cancel,
                ) else {
                    return TypeId::Unknown;
                };
                let scrutinee_ty = if scrutinee_completes {
                    scrutinee_ty
                } else {
                    TypeId::Unknown
                };
                let mut result: Option<TypeId> = None;
                let mut reachable = scrutinee_completes;
                for arm in arms {
                    if self.cancel.check().is_err() {
                        return TypeId::Unknown;
                    }
                    let arm_context = expected.or(if reachable { result.as_ref() } else { None });
                    let found = self.infer_match_arm_type(arm, &scrutinee_ty, env, arm_context);
                    if !reachable {
                        continue;
                    }
                    reachable = !self
                        .lowered
                        .module
                        .pattern(arm.pattern)
                        .kind
                        .is_irrefutable();
                    let Ok(completes) = super::completion::expr_can_complete(
                        &self.lowered.module,
                        arm.expr,
                        self.cancel,
                    ) else {
                        return TypeId::Unknown;
                    };
                    if !completes {
                        continue;
                    }
                    if let Some(result) = &mut result {
                        if found.conflicts_with(result) {
                            self.diagnostics.push(
                                Diagnostic::error(DiagnosticKind::MatchArmTypeMismatch {
                                    expected: display_type_id(result),
                                    found: display_type_id(&found),
                                })
                                .with_span(self.lowered.source_map.expr_span(arm.expr)),
                            );
                        }
                        result.recover_from(&found);
                    } else {
                        result = Some(found);
                    }
                }
                result.unwrap_or(TypeId::Builtin(BuiltinType::Unit))
            }
            ExprKind::StructInit {
                path,
                fields,
                explicit_type,
            } => {
                let explicit = explicit_type.map(|ty| self.resolve_constructor_type(ty, env));
                self.infer_struct_init_type(
                    path,
                    fields,
                    expr_id,
                    env,
                    explicit.as_ref().or(expected),
                )
            }
            ExprKind::Tuple(elements) => {
                let mut types = Vec::with_capacity(elements.len());
                for (index, expr) in elements.iter().enumerate() {
                    if self.cancel.check().is_err() {
                        return TypeId::Unknown;
                    }
                    let member = match expected {
                        Some(TypeId::Tuple(types)) if types.len() == elements.len() => {
                            types.get(index)
                        }
                        _ => None,
                    };
                    types.push(self.infer_expr_type_expected(*expr, env, member));
                }
                TypeId::Tuple(types)
            }
            ExprKind::Array(elements) => {
                let member = match expected {
                    Some(TypeId::Array(element)) => Some(element.as_ref()),
                    _ => None,
                };
                let mut element_ty: Option<TypeId> = None;
                let mut reachable = true;
                for expr in elements {
                    if self.cancel.check().is_err() {
                        return TypeId::Unknown;
                    }
                    let ty =
                        self.infer_expr_type_expected(*expr, env, member.or(element_ty.as_ref()));
                    if !reachable {
                        continue;
                    }
                    let Ok(completes) = super::completion::expr_can_complete(
                        &self.lowered.module,
                        *expr,
                        self.cancel,
                    ) else {
                        return TypeId::Unknown;
                    };
                    if !completes {
                        reachable = false;
                        continue;
                    }
                    if let Some(element_ty) = &mut element_ty {
                        if ty.conflicts_with(element_ty) {
                            self.diagnostics.push(
                                Diagnostic::error(DiagnosticKind::ArrayElementTypeMismatch {
                                    expected: display_type_id(element_ty),
                                    found: display_type_id(&ty),
                                })
                                .with_span(self.lowered.source_map.expr_span(*expr)),
                            );
                        }
                        element_ty.recover_from(&ty);
                    } else {
                        element_ty = Some(ty);
                    }
                }
                TypeId::Array(Box::new(element_ty.unwrap_or_else(|| {
                    member
                        .cloned()
                        .unwrap_or(TypeId::Builtin(BuiltinType::Unit))
                })))
            }
            ExprKind::Block(block) => self.infer_block_types_expected(*block, env, expected),
        };

        if let Some(expected) = expected
            && matches!(
                (
                    self.type_table
                        .call_resolution(expr_id)
                        .map(|call| call.target),
                    expected
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
            ty = expected.clone();
        }

        super::applications::validate(
            &ty,
            &env.generic_bounds,
            self.aggregates,
            self.type_table,
            self.lowered.source_map.expr_span(expr_id),
            self.diagnostics,
            self.cancel,
        );
        env.exprs.insert(expr_id, ty.clone());
        self.type_table.insert_expr(expr_id, ty.clone());
        ty
    }

    fn infer_match_arm_type(
        &mut self,
        arm: &MatchArm,
        scrutinee_ty: &TypeId,
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
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
        self.infer_expr_type_expected(arm.expr, &mut arm_env, expected)
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

    fn infer_standard_args(
        &mut self,
        intrinsic: StandardIntrinsic,
        receiver: Option<&TypeId>,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> Vec<(ExprId, TypeId)> {
        use StandardIntrinsic::*;
        let mut actual = Vec::new();
        let mut remaining = args;
        if receiver.is_none()
            && let Some((first, rest)) = args.split_first()
        {
            actual.push((*first, self.infer_expr_type(*first, env)));
            remaining = rest;
        }
        let base = receiver.or_else(|| actual.first().map(|(_, ty)| ty));
        let context = match (intrinsic, base) {
            (ArrayPush, Some(TypeId::Array(item))) => vec![(**item).clone()],
            (ArrayInsert, Some(TypeId::Array(item))) => {
                vec![TypeId::Builtin(BuiltinType::USize), (**item).clone()]
            }
            (MapContainsKey | MapGet | MapRemove, Some(TypeId::Map { key, .. })) => {
                vec![(**key).clone()]
            }
            (MapInsert, Some(TypeId::Map { key, value })) => {
                vec![(**key).clone(), (**value).clone()]
            }
            (SetContains | SetInsert | SetRemove, Some(TypeId::Set(item))) => {
                vec![(**item).clone()]
            }
            (SetUnion | SetIntersection | SetDifference, Some(ty @ TypeId::Set(_))) => {
                vec![ty.clone()]
            }
            (
                OptionUnwrapOr,
                Some(TypeId::StandardEnum {
                    kind: surface::StandardEnum::Option,
                    args,
                }),
            )
            | (
                ResultUnwrapOr,
                Some(TypeId::StandardEnum {
                    kind: surface::StandardEnum::Result,
                    args,
                }),
            ) => args.first().cloned().into_iter().collect(),
            (MathMin | MathMax, Some(ty)) => vec![ty.clone()],
            (MathClamp, Some(ty)) => vec![ty.clone(), ty.clone()],
            (DebugAssertEq, Some(ty)) => vec![ty.clone(), TypeId::Builtin(BuiltinType::String)],
            _ => Vec::new(),
        };
        actual.extend(self.infer_typed_args(remaining, context.into_iter(), env));
        actual
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

        let arg_tys = self.infer_standard_args(intrinsic, receiver_ty.as_ref(), args, env);
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
                let mut result = TypeId::Unknown;
                for (index, (argument, ty)) in arg_tys.iter().enumerate() {
                    let Ok(completes) = super::completion::expr_can_complete(
                        &self.lowered.module,
                        *argument,
                        self.cancel,
                    ) else {
                        return TypeId::Unknown;
                    };
                    if !completes {
                        continue;
                    }
                    self.check_standard_constraint(
                        ty,
                        StandardTypeConstraint::OrderedNumber,
                        env,
                        *argument,
                    );
                    if result.conflicts_with(ty) {
                        self.emit_arg_mismatch(
                            name,
                            &format!("arg{index}"),
                            &result,
                            ty,
                            *argument,
                        );
                    }
                    result.recover_from(ty);
                }
                result
            }
            MathAbs => {
                let Some((argument, value_ty)) = arg_tys.first() else {
                    return TypeId::Error;
                };
                let Ok(completes) = super::completion::expr_can_complete(
                    &self.lowered.module,
                    *argument,
                    self.cancel,
                ) else {
                    return TypeId::Unknown;
                };
                if !completes {
                    return TypeId::Unknown;
                }
                self.check_standard_constraint(
                    value_ty,
                    StandardTypeConstraint::SignedNumber,
                    env,
                    *argument,
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
                let mut previous: Option<&TypeId> = None;
                for (expr, operand) in arg_tys.iter().take(2) {
                    let Ok(completes) = super::completion::expr_can_complete(
                        &self.lowered.module,
                        *expr,
                        self.cancel,
                    ) else {
                        return TypeId::Unknown;
                    };
                    if !completes {
                        continue;
                    }
                    self.check_standard_constraint(
                        operand,
                        StandardTypeConstraint::Comparable,
                        env,
                        *expr,
                    );
                    if let Some(lhs) = previous
                        && lhs.conflicts_with(operand)
                    {
                        self.emit_arg_mismatch(name, "rhs", lhs, operand, *expr);
                    }
                    previous = Some(operand);
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

    fn infer_host_call_type(
        &mut self,
        call: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        let ResolvedName::HostFunction(id) = self.names.expr_resolution(callee)? else {
            return None;
        };
        let declaration = self.names.hosts.function(id)?.clone();
        self.type_table
            .insert_call(call, CallTarget::HostFunction(id), None);
        Some(self.infer_host_signature(&declaration, &declaration.symbol, callee, args, env))
    }
    fn infer_host_method_call(
        &mut self,
        call: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        let ExprKind::Field { receiver, name } = &self.lowered.module.expr(callee).kind else {
            return None;
        };
        let receiver = *receiver;
        let ty = self.infer_expr_type(receiver, env);
        let TypeId::Host(owner) = ty else {
            return None;
        };
        let id = self.names.hosts.method(&owner, name)?;
        let declaration = self.names.hosts.function(id)?.clone();
        self.type_table
            .insert_call(call, CallTarget::HostFunction(id), Some(receiver));
        let mut operands = vec![receiver];
        operands.extend_from_slice(args);
        Some(self.infer_host_signature(&declaration, &declaration.symbol, callee, &operands, env))
    }

    fn infer_host_signature(
        &mut self,
        declaration: &kagari_common::host_interface::HostFunctionDeclaration,
        name: &str,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> TypeId {
        let args = self.infer_typed_args(
            args,
            declaration
                .params
                .iter()
                .map(|parameter| crate::host::signature_type(&parameter.ty)),
            env,
        );
        if args.len() != declaration.params.len() {
            let implicit = usize::from(declaration.method_owner().is_some());
            self.check_builtin_arity(
                name,
                declaration.params.len() - implicit,
                args.len().saturating_sub(implicit),
                callee,
            );
        }
        for (index, parameter) in declaration.params.iter().enumerate() {
            if self.cancel.check().is_err() {
                return TypeId::Unknown;
            }
            self.check_arg_type(
                name,
                &parameter.name,
                crate::host::signature_type(&parameter.ty),
                index,
                &args,
            );
        }
        crate::host::signature_type(&declaration.return_type)
    }

    fn infer_runtime_helper_call_type(
        &mut self,
        call_expr: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        let ResolvedName::RuntimeHelper(builtin) = self.names.expr_resolution(callee)? else {
            return None;
        };
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
                let Some(field_name) =
                    self.checked_reflection_field_name(*field_name_expr, env, "get_field")
                else {
                    return Some(TypeId::Error);
                };
                Some(self.checked_member_type(&base_ty, &field_name, *field_name_expr, false))
            }
            BuiltinFunction::SetField => {
                let [base, field_name_expr, value] = args else {
                    return Some(TypeId::Error);
                };
                let base_ty = self.infer_expr_type(*base, env);
                self.check_const_write(*base);
                let field_name =
                    self.checked_reflection_field_name(*field_name_expr, env, "set_field");
                let expected = field_name
                    .as_ref()
                    .map(|name| self.checked_member_type(&base_ty, name, *field_name_expr, true));
                self.check_reflection_assignment_value(*value, expected.as_ref(), env);
                if field_name.is_none() {
                    return Some(TypeId::Error);
                }
                Some(base_ty)
            }
            BuiltinFunction::SetIndex => {
                let [base, index, value] = args else {
                    return Some(TypeId::Error);
                };
                let base_ty = self.infer_expr_type(*base, env);
                self.check_const_write(*base);
                let index_ty = self.infer_expr_type(*index, env);
                let expected = self.checked_index_type(*index, &base_ty, &index_ty, *index);
                self.check_reflection_assignment_value(*value, expected.as_ref(), env);
                Some(base_ty)
            }
            BuiltinFunction::Print => Some(self.infer_host_signature(
                &kagari_common::host_interface::standard_log(),
                "print",
                callee,
                args,
                env,
            )),
        }
    }
    fn check_reflection_assignment_value(
        &mut self,
        value: ExprId,
        expected: Option<&TypeId>,
        env: &mut BodyTypeEnv,
    ) {
        let found = self.infer_expr_type_expected(value, env, expected);
        let Ok(completes) =
            super::completion::expr_can_complete(&self.lowered.module, value, self.cancel)
        else {
            return;
        };
        if completes
            && let Some(expected) = expected
            && expected.conflicts_with(&found)
        {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::AssignmentTypeMismatch {
                    expected: display_type_id(expected),
                    found: display_type_id(&found),
                })
                .with_span(self.lowered.source_map.expr_span(value)),
            );
        }
    }

    fn checked_reflection_field_name(
        &mut self,
        expression: ExprId,
        env: &mut BodyTypeEnv,
        helper: &str,
    ) -> Option<String> {
        let ty = self.infer_expr_type(expression, env);
        let expected = TypeId::Builtin(BuiltinType::String);
        if ty.conflicts_with(&expected) {
            self.emit_arg_mismatch(helper, "field", &expected, &ty, expression);
            return None;
        }
        if ty.is_unresolved() {
            return None;
        }
        let name = self.string_literal_value(expression);
        if name.is_none() {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::ReflectionFieldNameNotConstant)
                    .with_span(self.lowered.source_map.expr_span(expression)),
            );
        }
        name
    }

    fn checked_member_type(
        &mut self,
        receiver: &TypeId,
        name: &str,
        site: ExprId,
        write: bool,
    ) -> TypeId {
        if let Some(field) = self.resolve_field(receiver, name) {
            self.type_table.insert_expr_field(site, field.id.clone());
            if write && !field.writeability.is_var() {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidAssignmentTarget {
                        reason: format!("field `{name}` is read-only"),
                    })
                    .with_span(self.lowered.source_map.expr_span(site)),
                );
            }
            // Read-only targets still provide RHS context and independent errors.
            return field.ty;
        }
        if !matches!(receiver, TypeId::Unknown | TypeId::Error) {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::UnknownName {
                    name: name.to_owned(),
                })
                .with_span(self.lowered.source_map.expr_span(site)),
            );
        }
        TypeId::Error
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
        let trait_ids = match &receiver_ty {
            TypeId::Trait(ty) => vec![ty.declaration.clone()],
            TypeId::Generic(parameter) => env
                .generic_bounds
                .get(parameter)?
                .iter()
                .filter_map(|bound| match bound {
                    super::ConstraintTarget::Trait(id) => Some(id.clone()),
                    _ => None,
                })
                .collect(),
            _ => return None,
        };
        let mut candidates = Vec::new();
        for owner in trait_ids {
            if let Some(contract) = self.aggregates.trait_(&owner) {
                for method in &contract.methods {
                    if method.name == *name && !candidates.contains(&method.id) {
                        candidates.push(method.id.clone());
                    }
                }
            }
        }
        if candidates.is_empty() {
            return None;
        }
        if candidates.len() != 1 {
            self.infer_call_args(args, env);
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::AmbiguousMethod { name: name.clone() })
                    .with_span(self.lowered.source_map.expr_span(callee)),
            );
            return Some(TypeId::Error);
        }
        let method = self
            .aggregates
            .trait_method(&candidates[0])
            .expect("catalog method")
            .clone();
        let self_owner = &method.owner;
        let self_ty = receiver_ty;
        self.type_table.insert_call(
            call_expr,
            CallTarget::TraitMethod(method.id.clone()),
            Some(*receiver),
        );
        let params = method
            .params
            .iter()
            .filter(|param| param.name != "self")
            .collect::<Vec<_>>();
        let param_types = params
            .iter()
            .map(|param| param.ty.with_self(self_owner, &self_ty))
            .collect::<Vec<_>>();
        let arg_tys = self.infer_typed_args(args, param_types.iter().cloned(), env);
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
        for (index, param) in params.iter().enumerate() {
            if self.cancel.check().is_err() {
                return Some(TypeId::Unknown);
            }
            self.check_arg_type(
                name,
                &param.name,
                param_types[index].clone(),
                index,
                &arg_tys,
            );
        }

        Some(method.return_type.with_self(self_owner, &self_ty))
    }

    fn enum_member_owner(&self, expr: ExprId) -> Option<kagari_common::identity::DefinitionId> {
        let member = self.names.qualified_member(expr)?;
        if let ResolvedName::Enum(id) = member.owner {
            return self
                .declarations
                .definition(ResolvedName::Enum(id))
                .cloned();
        }
        let TypeId::Enum(id) = &self
            .declarations
            .imported_types()
            .resolved(member.owner)?
            .ty
        else {
            return None;
        };
        Some(id.declaration.clone())
    }

    fn resolve_constructor_type(&mut self, ty: crate::hir::TypeRefId, env: &BodyTypeEnv) -> TypeId {
        let resolved = resolve_type_in(
            &self.lowered.module,
            ty,
            TypeContext {
                declarations: self.declarations,
                generics: &env.generics,
                self_type: None,
            },
            self.type_table,
            self.cancel,
        );
        if resolved.is_unresolved() {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::UnknownTypeAnnotation {
                    type_name: display_type(&self.lowered.module, ty),
                })
                .with_span(self.lowered.source_map.type_span(ty)),
            );
        }
        super::check::validate_standard_type_constraints(
            &resolved,
            &env.generic_bounds,
            self.lowered.source_map.type_span(ty),
            self.diagnostics,
            self.cancel,
        );
        resolved
    }

    fn infer_enum_constructor(
        &mut self,
        expression: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> Option<TypeId> {
        let explicit = match &self.lowered.module.expr(callee).kind {
            ExprKind::Name { explicit_type, .. } => {
                explicit_type.map(|ty| self.resolve_constructor_type(ty, env))
            }
            _ => None,
        };
        let enumeration = match self.enum_member_owner(callee) {
            Some(owner) => owner,
            None if explicit.is_some() => {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidValueTarget {
                        name: "explicit enum constructor".into(),
                    })
                    .with_span(self.lowered.source_map.expr_span(callee)),
                );
                self.infer_call_args(args, env);
                return Some(TypeId::Error);
            }
            None => return None,
        };
        let expected = explicit.as_ref().or(expected);
        let member = self
            .names
            .qualified_member(callee)
            .expect("qualified owner");
        let signature = self
            .aggregates
            .enumeration(&enumeration)
            .expect("enum signature catalog");
        let variant = signature
            .variants
            .iter()
            .find(|variant| variant.name == member.name)
            .cloned();
        let name = format!("{}::{}", signature.declaration.name, member.name);
        let generic_params = signature.generic_params.clone();
        let target = super::ResolvedEnumConstructor {
            enumeration: enumeration.clone(),
            variant: variant.as_ref().map(|variant| variant.id.clone()),
        };
        self.type_table
            .insert_enum_constructor(callee, target.clone());
        self.type_table.insert_enum_constructor(expression, target);
        // Every argument is checked once, even when the variant is absent or its
        // signature is erroneous. Known target facts survive argument failures.
        let mut substitution = crate::types::TypeSubstitution::new();
        if let Some(TypeId::Enum(nominal)) = expected
            && nominal.declaration == enumeration
            && nominal.arguments.len() == generic_params.len()
        {
            substitution.extend(
                generic_params
                    .iter()
                    .cloned()
                    .zip(nominal.arguments.iter().cloned()),
            );
        }
        let actual = self.infer_generic_args(
            args,
            variant
                .iter()
                .flat_map(|variant| variant.payload.iter().cloned()),
            &generic_params,
            &mut substitution,
            env,
        );
        if self.cancel.check().is_err() {
            return Some(TypeId::Unknown);
        }
        let Ok(completes) =
            super::completion::expr_can_complete(&self.lowered.module, expression, self.cancel)
        else {
            return Some(TypeId::Unknown);
        };
        let arguments = self.finish_inferred_arguments(
            &mut substitution,
            &generic_params,
            &name,
            callee,
            !completes,
        );
        let result = TypeId::Enum(crate::types::NominalType {
            declaration: enumeration,
            arguments,
        });
        self.type_table.insert_expr(callee, result.clone());
        env.exprs.insert(callee, result.clone());
        if let Some(variant) = variant {
            if actual.len() != variant.payload.len() {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::CallArityMismatch {
                        function_name: name.clone(),
                        expected: variant.payload.len(),
                        found: actual.len(),
                    })
                    .with_span(self.lowered.source_map.expr_span(callee)),
                );
            }
            for (index, expected) in variant.payload.iter().enumerate() {
                if self.cancel.check().is_err() {
                    return Some(TypeId::Unknown);
                }
                self.check_arg_type(
                    &name,
                    &format!("payload[{index}]"),
                    expected.instantiate(&substitution),
                    index,
                    &actual,
                );
            }
        } else {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::UnknownName { name })
                    .with_span(self.lowered.source_map.expr_span(callee)),
            );
        }
        Some(result)
    }

    fn infer_function_call_type(
        &mut self,
        call_expr: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> TypeId {
        if let Some(imported) = self
            .names
            .expr_resolution(callee)
            .and_then(|name| self.imported_functions.get(name))
        {
            let arg_tys = self.infer_typed_args(
                args,
                imported.signature.params.iter().map(|p| p.ty.clone()),
                env,
            );
            self.type_table
                .insert_call(call_expr, CallTarget::SourceFunction(imported.id), None);
            if !imported.signature.generic_params.is_empty() {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::PublicGenericFunction {
                        name: imported.signature.name.clone(),
                    })
                    .with_span(self.lowered.source_map.expr_span(callee)),
                );
                return TypeId::Error;
            }
            self.check_function_arguments(
                &imported.signature,
                &Default::default(),
                callee,
                &arg_tys,
            );
            return imported.signature.return_type.clone();
        }
        let Some(ResolvedName::Function(id)) = self.names.expr_resolution(callee) else {
            self.infer_call_args(args, env);
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
            self.infer_call_args(args, env);
            return self.infer_expr_type(callee, env);
        };
        self.type_table
            .insert_call(call_expr, CallTarget::Function(id), None);
        let mut substitution = crate::types::TypeSubstitution::new();
        if let Some(expected) = expected
            && super::inference::infer(
                &function.return_type,
                expected,
                &function.generic_params,
                &mut substitution,
                self.cancel,
            )
            .is_err()
        {
            return TypeId::Unknown;
        }
        let arg_tys = self.infer_generic_args(
            args,
            function.params.iter().map(|parameter| parameter.ty.clone()),
            &function.generic_params,
            &mut substitution,
            env,
        );
        if self.cancel.check().is_err() {
            return TypeId::Unknown;
        }
        let mut suppress_missing = arg_tys.iter().any(|(_, ty)| ty.is_unresolved());
        for (argument, _) in &arg_tys {
            let Ok(completes) =
                super::completion::expr_can_complete(&self.lowered.module, *argument, self.cancel)
            else {
                return TypeId::Unknown;
            };
            suppress_missing |= !completes;
        }
        let type_arguments = self.finish_inferred_arguments(
            &mut substitution,
            &function.generic_params,
            &function.name,
            callee,
            suppress_missing,
        );
        self.type_table
            .insert_type_arguments(call_expr, type_arguments);
        for parameter in &function.generic_params {
            let Some(actual) = substitution.get(parameter) else {
                continue;
            };
            for constraint in function
                .bounds
                .get(parameter)
                .into_iter()
                .flatten()
                .cloned()
            {
                match constraint {
                    super::ConstraintTarget::Standard(constraint) => {
                        self.check_standard_constraint(actual, constraint, env, callee)
                    }
                    super::ConstraintTarget::Trait(trait_id) => {
                        let satisfied = match actual {
                            TypeId::Generic(parameter) => {
                                env.generic_bounds.get(parameter).is_some_and(|bounds| {
                                    bounds
                                        .contains(&super::ConstraintTarget::Trait(trait_id.clone()))
                                })
                            }
                            _ => self.type_table.implements(&trait_id, actual),
                        };
                        if !satisfied && !actual.is_unresolved() {
                            let trait_name = self
                                .declarations
                                .get(&crate::declarations::DeclarationId::Definition(trait_id))
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
        self.check_function_arguments(function, &substitution, callee, &arg_tys);
        function.return_type.instantiate(&substitution)
    }

    fn check_function_arguments(
        &mut self,
        function: &super::TypedFunction,
        substitution: &crate::types::TypeSubstitution,
        callee: ExprId,
        arg_tys: &[(ExprId, TypeId)],
    ) {
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
        for (index, param) in function.params.iter().enumerate() {
            if self.cancel.check().is_err() {
                return;
            }
            self.check_arg_type(
                &function.name,
                &param.name,
                param.ty.instantiate(substitution),
                index,
                arg_tys,
            );
        }
    }

    fn const_root_name(&self, expr_id: ExprId) -> Option<String> {
        match &self.lowered.module.expr(expr_id).kind {
            ExprKind::Name { .. } => match self.names.expr_resolution(expr_id) {
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

    fn infer_host_field_write(
        &mut self,
        place_id: PlaceId,
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        let PlaceKind::Field { .. } = self.lowered.module.place(place_id).kind else {
            return None;
        };
        let mut root = place_id;
        let mut chain = Vec::new();
        while let PlaceKind::Field { base, name } = &self.lowered.module.place(root).kind {
            chain.push((root, name.clone()));
            root = *base;
        }
        chain.reverse();
        let mut ty = self.resolve_readable_place_type(root, env)?;
        let mut prefix = 0;
        while !matches!(ty, TypeId::Host(_)) && prefix + 1 < chain.len() {
            root = chain[prefix].0;
            ty = self.resolve_readable_place_type(root, env)?;
            prefix += 1;
        }
        chain.drain(..prefix);
        let TypeId::Host(owner) = &ty else {
            return None;
        };
        let owner = owner.clone();
        let mut fields = Vec::new();
        for (place, name) in chain {
            let field = if let TypeId::Host(id) = &ty {
                self.names
                    .hosts
                    .nominal_type(id)
                    .and_then(|id| self.names.hosts.type_declaration(id))
                    .and_then(|owner| owner.fields.iter().find(|field| field.name == name))
                    .cloned()
            } else {
                None
            };
            let Some(field) = field else {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::UnknownName { name })
                        .with_span(self.lowered.source_map.place_span(place)),
                );
                return Some(TypeId::Error);
            };
            self.type_table.insert_place_field(place, field.id.clone());
            fields.push(field.id);
            ty = crate::host::signature_type(&field.ty);
            self.type_table.insert_place(place, ty.clone());
        }
        let resolved =
            self.names
                .hosts
                .field_path(&owner, &fields)
                .and_then(|(declaration, contract)| {
                    if declaration.access != kagari_common::host_interface::PathAccess::ReadWrite {
                        Err("field path is read-only")
                    } else {
                        Ok((declaration, contract))
                    }
                });
        match resolved {
            Ok((declaration, contract)) => self.type_table.insert_host_place_path(
                place_id,
                super::ResolvedHostPlacePath {
                    root,
                    declaration,
                    contract,
                },
            ),
            Err(reason) => self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidHostPath {
                    reason: reason.into(),
                })
                .with_span(self.lowered.source_map.place_span(place_id)),
            ),
        }
        Some(ty)
    }

    fn infer_host_field_read(&mut self, expr_id: ExprId, env: &mut BodyTypeEnv) -> Option<TypeId> {
        let ExprKind::Field { .. } = self.lowered.module.expr(expr_id).kind else {
            return None;
        };
        let mut root = expr_id;
        let mut chain = Vec::new();
        while let ExprKind::Field { receiver, name } = &self.lowered.module.expr(root).kind {
            chain.push((root, name.clone()));
            root = *receiver;
        }
        chain.reverse();
        let mut ty = self.infer_expr_type(root, env);
        let mut prefix = 0;
        while !matches!(ty, TypeId::Host(_)) && prefix + 1 < chain.len() {
            root = chain[prefix].0;
            ty = self.infer_expr_type(root, env);
            prefix += 1;
        }
        chain.drain(..prefix);
        let TypeId::Host(owner) = &ty else {
            return None;
        };
        let owner = owner.clone();
        let mut fields = Vec::new();
        for (expr, name) in chain {
            let field = if let TypeId::Host(id) = &ty {
                self.names
                    .hosts
                    .nominal_type(id)
                    .and_then(|id| self.names.hosts.type_declaration(id))
                    .and_then(|owner| owner.fields.iter().find(|field| field.name == name))
                    .cloned()
            } else {
                None
            };
            let Some(field) = field else {
                self.diagnostics.push(
                    Diagnostic::error(if name.is_empty() {
                        DiagnosticKind::ExpectedFieldName
                    } else {
                        DiagnosticKind::UnknownName { name }
                    })
                    .with_span(self.lowered.source_map.expr_span(expr)),
                );
                return Some(TypeId::Error);
            };
            self.type_table.insert_expr_field(expr, field.id.clone());
            fields.push(field.id);
            ty = crate::host::signature_type(&field.ty);
            self.type_table.insert_expr(expr, ty.clone());
            env.exprs.insert(expr, ty.clone());
        }
        match self.names.hosts.field_path(&owner, &fields) {
            Ok((declaration, contract)) => self.type_table.insert_host_path(
                expr_id,
                super::ResolvedHostPath {
                    root,
                    declaration,
                    contract,
                },
            ),
            Err(reason) => self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidHostPath {
                    reason: reason.into(),
                })
                .with_span(self.lowered.source_map.expr_span(expr_id)),
            ),
        }
        Some(ty)
    }

    fn resolve_field(
        &self,
        receiver: &TypeId,
        field_name: &str,
    ) -> Option<crate::aggregates::FieldSignature> {
        let TypeId::Struct(id) = receiver else {
            return None;
        };
        let structure = self.aggregates.structure(&id.declaration)?;
        if structure.generic_params.len() != id.arguments.len() {
            return None;
        }
        let substitution = structure
            .generic_params
            .iter()
            .cloned()
            .zip(id.arguments.iter().cloned())
            .collect();
        let mut field = structure
            .fields
            .iter()
            .find(|field| field.name == field_name)?
            .clone();
        field.ty = field.ty.instantiate(&substitution);
        Some(field)
    }

    fn resolve_struct_id(&self, path: &str) -> Option<kagari_common::identity::DefinitionId> {
        if let Some(binding) = self.declarations.names.lookup(path) {
            match binding.target()? {
                target @ ResolvedName::Struct(_) => {
                    return self.declarations.definition(target).cloned();
                }
                ResolvedName::SourceImport(_) => {}
                _ => return None,
            }
        }
        let TypeId::Struct(id) = &self.declarations.imported_types().get(path)?.ty else {
            return None;
        };
        Some(id.declaration.clone())
    }

    fn infer_binary_type(
        &mut self,
        op: BinaryOp,
        rhs_expr: ExprId,
        lhs_ty: Option<TypeId>,
        rhs_ty: Option<TypeId>,
        env: &BodyTypeEnv,
    ) -> TypeId {
        let produces_operands = lhs_ty.is_some() && rhs_ty.is_some();
        // An absent operand has no value constraint. Recovery types still retain
        // their own expression diagnostics and known counterpart constraints.
        let lhs_ty = lhs_ty.unwrap_or(TypeId::Unknown);
        let rhs_ty = rhs_ty.unwrap_or(TypeId::Unknown);
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
                if !produces_operands {
                    TypeId::Unknown
                } else if matches!(lhs_ty, TypeId::Unknown | TypeId::Error)
                    || matches!(rhs_ty, TypeId::Unknown | TypeId::Error)
                {
                    TypeId::Error
                } else {
                    lhs_ty
                }
            }
            BinaryOp::Eq | BinaryOp::NotEq => {
                if lhs_ty.conflicts_with(&rhs_ty)
                    || [&lhs_ty, &rhs_ty].into_iter().any(|ty| {
                        super::constraints::known_type_violates_constraint(
                            ty,
                            StandardTypeConstraint::Comparable,
                            &env.generic_bounds,
                        )
                    })
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
                if lhs_ty.conflicts_with(&TypeId::Builtin(BuiltinType::Bool))
                    || rhs_ty.conflicts_with(&TypeId::Builtin(BuiltinType::Bool))
                {
                    self.emit_binary_operand_type_mismatch(op, "bool", &lhs_ty, &rhs_ty, rhs_expr);
                }
                TypeId::Builtin(BuiltinType::Bool)
            }
        }
    }

    fn matching_numeric_operands(&self, lhs: &TypeId, rhs: &TypeId, env: &BodyTypeEnv) -> bool {
        if matches!(lhs, TypeId::Unknown | TypeId::Error)
            || matches!(rhs, TypeId::Unknown | TypeId::Error)
        {
            return ![lhs, rhs].into_iter().any(|ty| {
                super::constraints::known_type_violates_constraint(
                    ty,
                    StandardTypeConstraint::OrderedNumber,
                    &env.generic_bounds,
                )
            });
        }
        surface::supports_arithmetic(lhs, rhs)
            || (lhs == rhs
                && matches!(lhs, TypeId::Generic(_))
                && super::constraints::type_satisfies_standard_constraint(
                    lhs,
                    StandardTypeConstraint::OrderedNumber,
                    &env.generic_bounds,
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
    ) -> Result<bool, kagari_common::cancellation::Cancelled> {
        let ty = self.infer_expr_type(expr_id, env);
        let completes =
            super::completion::expr_can_complete(&self.lowered.module, expr_id, self.cancel)?;
        if completes && ty.conflicts_with(&TypeId::Builtin(BuiltinType::Bool)) {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::ConditionTypeMismatch {
                    context,
                    found: display_type_id(&ty),
                })
                .with_span(self.lowered.source_map.expr_span(expr_id)),
            );
        }
        Ok(completes)
    }

    fn infer_struct_init_type(
        &mut self,
        path: &str,
        fields: &[crate::hir::FieldInit],
        expr_id: ExprId,
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> TypeId {
        let Some(struct_def) = self
            .resolve_struct_id(path)
            .and_then(|id| self.aggregates.structure(&id))
            .cloned()
        else {
            for field in fields {
                self.infer_expr_type(field.value, env);
            }
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidStructInitializer {
                    struct_name: path.to_owned(),
                    reason: "unknown struct".to_owned(),
                })
                .with_span(self.lowered.source_map.expr_span(expr_id)),
            );
            return TypeId::Error;
        };

        let mut substitution = crate::types::TypeSubstitution::new();
        if let Some(TypeId::Struct(nominal)) = expected
            && nominal.declaration == struct_def.id
            && nominal.arguments.len() == struct_def.generic_params.len()
        {
            substitution.extend(
                struct_def
                    .generic_params
                    .iter()
                    .cloned()
                    .zip(nominal.arguments.iter().cloned()),
            );
        }
        let mut field_tys = Vec::with_capacity(fields.len());
        let mut completes = true;
        for field in fields {
            if self.cancel.check().is_err() {
                return TypeId::Unknown;
            }
            let parameter = struct_def
                .fields
                .iter()
                .find(|member| member.name == field.name);
            let expected = parameter.map(|member| {
                member
                    .ty
                    .argument_context(&substitution, &struct_def.generic_params)
            });
            let actual = self.infer_expr_type_expected(field.value, env, expected.as_ref());
            let Ok(field_completes) = super::completion::expr_can_complete(
                &self.lowered.module,
                field.value,
                self.cancel,
            ) else {
                return TypeId::Unknown;
            };
            completes &= field_completes;
            if field_completes
                && let Some(parameter) = parameter
                && super::inference::infer(
                    &parameter.ty,
                    &actual,
                    &struct_def.generic_params,
                    &mut substitution,
                    self.cancel,
                )
                .is_err()
            {
                return TypeId::Unknown;
            }
            field_tys.push((field.name.as_str(), field.value, actual, field_completes));
        }
        let arguments = self.finish_inferred_arguments(
            &mut substitution,
            &struct_def.generic_params,
            path,
            expr_id,
            !completes,
        );
        let mut seen = HashSet::new();
        let resolved = super::ResolvedStructInit {
            structure: struct_def.id.clone(),
            fields: fields
                .iter()
                .map(|init| {
                    struct_def
                        .fields
                        .iter()
                        .find(|field| field.name == init.name)
                        .map(|field| field.id.clone())
                })
                .collect(),
        };
        for ((name, value_expr, value_ty, field_completes), target) in
            field_tys.iter().zip(&resolved.fields)
        {
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

            let Some(expected) = self
                .aggregates
                .field(field)
                .map(|field| field.ty.instantiate(&substitution))
            else {
                continue;
            };
            if *field_completes && expected.conflicts_with(value_ty) {
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

        TypeId::Struct(crate::types::NominalType {
            declaration: struct_def.id,
            arguments,
        })
    }

    fn checked_index_type(
        &mut self,
        index: ExprId,
        receiver: &TypeId,
        index_ty: &TypeId,
        site: ExprId,
    ) -> Option<TypeId> {
        let result = self.resolve_index_type(index, receiver);
        if result.is_none()
            && !matches!(receiver, TypeId::Unknown | TypeId::Error)
            && !matches!(index_ty, TypeId::Unknown | TypeId::Error)
        {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidIndexTarget {
                    type_name: display_type_id(receiver),
                })
                .with_span(self.lowered.source_map.expr_span(site)),
            );
        }
        result
    }

    fn resolve_index_type(&self, index_expr: ExprId, receiver: &TypeId) -> Option<TypeId> {
        if !super::completion::expr_can_complete(&self.lowered.module, index_expr, self.cancel)
            .ok()?
        {
            return match receiver {
                TypeId::Array(element) => Some((**element).clone()),
                // No index value exists to select a particular Tuple member.
                TypeId::Tuple(_) => Some(TypeId::Unknown),
                _ => None,
            };
        }
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
        match self.names.expr_resolution(expr_id) {
            Some(ResolvedName::StandardFunction(intrinsic)) => Some(intrinsic),
            _ => None,
        }
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
        let Ok(completes) =
            super::completion::expr_can_complete(&self.lowered.module, *arg_expr, self.cancel)
        else {
            return;
        };
        if !completes {
            return;
        }
        if found.conflicts_with(&expected) {
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
        let Some(found) = found
            .as_ref()
            .filter(|ty| !matches!(ty, TypeId::Unknown | TypeId::Error))
        else {
            // Missing operands already have an arity diagnostic; whole error
            // operands have their own diagnostic from expression checking.
            return;
        };
        self.diagnostics.push(
            Diagnostic::error(DiagnosticKind::ArgumentTypeMismatch {
                function_name: function_name.to_owned(),
                parameter_name: parameter_name.to_owned(),
                expected: expected.to_owned(),
                found: display_type_id(found),
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
        super::check::validate_standard_constraint_type(
            ty,
            constraint,
            &env.generic_bounds,
            self.lowered.source_map.expr_span(span_expr),
            self.diagnostics,
        );
    }

    fn string_literal_value(&self, expr_id: ExprId) -> Option<String> {
        match self.type_table.scalar_value(expr_id)? {
            super::ScalarValue::String(value) => Some(value.clone()),
            _ => None,
        }
    }
    fn finish_inferred_arguments(
        &mut self,
        substitution: &mut crate::types::TypeSubstitution,
        parameters: &[crate::types::GenericParameterType],
        name: &str,
        site: ExprId,
        suppress_missing: bool,
    ) -> Vec<TypeId> {
        let mut arguments = Vec::with_capacity(parameters.len());
        for parameter in parameters {
            let inferred = substitution.get(parameter);
            let unknown = inferred.is_some_and(TypeId::contains_unknown);
            if unknown || (inferred.is_none() && !suppress_missing) {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::CannotInferGenericArgument {
                        function_name: name.to_owned(),
                        parameter: parameter.name.clone(),
                    })
                    .with_span(self.lowered.source_map.expr_span(site)),
                );
            }
            let argument = inferred
                .map(TypeId::diagnose_unknowns)
                .unwrap_or(TypeId::Error);
            // Result facts and subsequent member/argument checks consume exactly
            // the same recovery substitution, including previously absent binders.
            substitution.insert(parameter.clone(), argument.clone());
            arguments.push(argument);
        }
        arguments
    }

    fn infer_generic_args(
        &mut self,
        args: &[ExprId],
        parameters: impl Iterator<Item = TypeId>,
        generics: &[crate::types::GenericParameterType],
        substitution: &mut crate::types::TypeSubstitution,
        env: &mut BodyTypeEnv,
    ) -> Vec<(ExprId, TypeId)> {
        let mut parameters = parameters.fuse();
        let mut actual = Vec::new();
        for argument in args {
            if self.cancel.check().is_err() {
                break;
            }
            let parameter = parameters.next();
            let expected = parameter
                .as_ref()
                .map(|ty| ty.argument_context(substitution, generics));
            let ty = self.infer_expr_type_expected(*argument, env, expected.as_ref());
            let Ok(completes) =
                super::completion::expr_can_complete(&self.lowered.module, *argument, self.cancel)
            else {
                break;
            };
            if completes
                && !generics.is_empty()
                && let Some(parameter) = parameter
                && super::inference::infer(&parameter, &ty, generics, substitution, self.cancel)
                    .is_err()
            {
                break;
            }
            actual.push((*argument, ty));
        }
        actual
    }

    fn infer_typed_args(
        &mut self,
        args: &[ExprId],
        expected: impl Iterator<Item = TypeId>,
        env: &mut BodyTypeEnv,
    ) -> Vec<(ExprId, TypeId)> {
        self.infer_generic_args(args, expected, &[], &mut Default::default(), env)
    }

    fn infer_call_args(&mut self, args: &[ExprId], env: &mut BodyTypeEnv) -> Vec<(ExprId, TypeId)> {
        self.infer_typed_args(args, std::iter::empty(), env)
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
