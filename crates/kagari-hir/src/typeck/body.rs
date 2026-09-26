mod conversions;
mod iteration;
mod operators;
mod standard;

use std::collections::{BTreeMap, HashMap, HashSet};

use kagari_common::{Diagnostic, DiagnosticKind};
use smallvec::SmallVec;

use crate::{
    builtin::{
        BuiltinFunction,
        surface::{self, StandardIntrinsic, StandardMethodReceiver, StandardTypeConstraint},
    },
    hir::pattern::PatternBound,
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

#[derive(Clone)]
enum HostPathNode<Id> {
    Member { node: Id, name: String },
    Index { node: Id, argument: ExprId },
}

impl<Id: Copy> HostPathNode<Id> {
    fn id(&self) -> Id {
        match self {
            Self::Member { node, .. } | Self::Index { node, .. } => *node,
        }
    }
}

#[derive(Clone)]
enum LoopResult {
    Statement,
    Expression(Box<LoopValue>),
}

#[derive(Clone)]
struct LoopValue {
    expected: Option<TypeId>,
    found: Option<TypeId>,
}

pub(crate) struct BodyChecker<'a> {
    aggregates: &'a crate::aggregates::AggregateCatalog,
    imported_functions: &'a crate::imports::ImportedFunctions,
    declarations: &'a crate::declarations::Declarations,
    cancel: &'a kagari_common::cancellation::CancellationToken,
    lowered: &'a LoweredModule,
    names: &'a ResolvedNames,
    function_index: &'a FunctionTypeIndex,
    top_level_index: &'a TopLevelTypeIndex,
    const_values: Option<&'a std::collections::HashMap<crate::hir::ConstId, super::ScalarValue>>,
    diagnostics: &'a mut SmallVec<[Diagnostic; 4]>,
    type_table: &'a mut TypeTable,
    function_name: &'a str,
    expected_return: TypeId,
    loop_depth: usize,
    loop_results: Vec<LoopResult>,
    inference_depth: usize,
    closure_returns: Vec<Vec<TypeId>>,
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
            const_values: indexes.const_values,
            diagnostics,
            type_table,
            function_name,
            expected_return,
            loop_depth: 0,
            loop_results: Vec::new(),
            inference_depth: 0,
            closure_returns: Vec::new(),
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
                self.infer_expr_with_coercion(expr, env, expected)
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
                            implementation: None,
                        },
                        self.type_table,
                        self.cancel,
                    );
                    if resolved.contains_projection() {
                        super::applications::validate(
                            &resolved,
                            &env.generic_bounds,
                            (self.aggregates, &self.names.hosts),
                            self.type_table,
                            self.lowered.source_map.type_span(ty),
                            self.diagnostics,
                            self.cancel,
                        );
                    }
                    let resolved = self.aggregates.normalize_type(&resolved);
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
                    self.infer_expr_with_coercion(*initializer, env, annotation.as_ref());
                let local_ty = annotation.unwrap_or_else(|| initializer_ty.clone());
                super::applications::validate_imported_interface_type(
                    &local_ty,
                    self.aggregates,
                    self.lowered.source.module_identity(),
                    self.lowered.source_map.stmt_span(stmt_id),
                    self.diagnostics,
                    self.cancel,
                );
                super::applications::validate(
                    &local_ty,
                    &env.generic_bounds,
                    (self.aggregates, &self.declarations.hosts),
                    self.type_table,
                    self.lowered.source_map.stmt_span(stmt_id),
                    self.diagnostics,
                    self.cancel,
                );
                let Ok(completes) = super::completion::expr_can_complete(
                    &self.lowered.module,
                    self.names,
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
                // Write permission does not erase the known target type needed by
                // contextual inference and tooling after an invalid assignment.
                let expected_ty = self.type_table.place_type(*target);
                let value_ty = self.infer_expr_with_coercion(*value, env, expected_ty.as_ref());
                let Ok(completes) = super::completion::expr_can_complete(
                    &self.lowered.module,
                    self.names,
                    *value,
                    self.cancel,
                ) else {
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
                    self.infer_expr_with_coercion(expr, env, Some(&expected))
                });
                if let Some(returns) = self.closure_returns.last_mut() {
                    returns.push(found.clone());
                }
                if let Some(expr) = expr {
                    let Ok(completes) = super::completion::expr_can_complete(
                        &self.lowered.module,
                        self.names,
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
                let mut body_env = env.clone();
                match condition {
                    crate::hir::Condition::Expr(expr) => {
                        if self.check_condition_type(*expr, "while", env).is_err() {
                            return;
                        }
                    }
                    crate::hir::Condition::Binding {
                        pattern,
                        initializer,
                    } => {
                        let ty = self.infer_expr_type(*initializer, env);
                        self.check_pattern(*pattern, &ty, &mut body_env);
                    }
                }
                self.loop_depth += 1;
                self.loop_results.push(LoopResult::Statement);
                let _ = self.infer_block_types(*body, &mut body_env);
                self.loop_results.pop();
                self.loop_depth -= 1;
            }
            StmtKind::Loop { body } => {
                self.loop_depth += 1;
                self.loop_results.push(LoopResult::Statement);
                let _ = self.infer_block_types(*body, env);
                self.loop_results.pop();
                self.loop_depth -= 1;
            }
            StmtKind::For {
                pattern,
                iterable,
                body,
            } => {
                let iterable_ty = self.infer_expr_type(*iterable, env);
                let Some(element_ty) = self.infer_iteration(*iterable, &iterable_ty, env) else {
                    if !iterable_ty.is_unresolved() {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::InvalidForIterable {
                                type_name: display_type_id(&iterable_ty),
                            })
                            .with_span(self.lowered.source_map.expr_span(*iterable)),
                        );
                    }
                    return;
                };
                let mut body_env = env.clone();
                self.check_pattern(*pattern, &element_ty, &mut body_env);
                if !self
                    .names
                    .pattern_is_irrefutable(&self.lowered.module, *pattern)
                {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: "irrefutable for binding".into(),
                            found: "refutable pattern".into(),
                        })
                        .with_span(self.lowered.source_map.pattern_span(*pattern)),
                    );
                }
                self.loop_depth += 1;
                self.loop_results.push(LoopResult::Statement);
                let _ = self.infer_block_types(*body, &mut body_env);
                self.loop_results.pop();
                self.loop_depth -= 1;
            }
            StmtKind::Break => {
                if self.loop_depth == 0 {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::BreakOutsideLoop)
                            .with_span(self.lowered.source_map.stmt_span(stmt_id)),
                    );
                } else {
                    self.record_loop_break_type(
                        TypeId::Builtin(BuiltinType::Unit),
                        false,
                        self.lowered.source_map.stmt_span(stmt_id),
                    );
                }
            }
            StmtKind::BreakValue(value) => {
                let context = match self.loop_results.last() {
                    Some(LoopResult::Expression(value)) => {
                        value.expected.as_ref().or(value.found.as_ref()).cloned()
                    }
                    _ => None,
                };
                let ty = self.infer_expr_type_expected(*value, env, context.as_ref());
                if self.loop_depth == 0 {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::BreakOutsideLoop)
                            .with_span(self.lowered.source_map.stmt_span(stmt_id)),
                    );
                } else {
                    self.record_loop_break_type(
                        ty,
                        true,
                        self.lowered.source_map.stmt_span(stmt_id),
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

    fn record_loop_break_type(&mut self, ty: TypeId, has_value: bool, span: kagari_common::Span) {
        match self.loop_results.last().cloned() {
            Some(LoopResult::Statement) => {
                if has_value {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::BreakValueOutsideLoopExpression)
                            .with_span(span),
                    );
                }
            }
            Some(LoopResult::Expression(value)) => {
                if let Some(expected) = value.expected.as_ref().or(value.found.as_ref())
                    && ty.conflicts_with(expected)
                {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::BreakValueTypeMismatch {
                            expected: display_type_id(expected),
                            found: display_type_id(&ty),
                        })
                        .with_span(span),
                    );
                }
                if let Some(LoopResult::Expression(value)) = self.loop_results.last_mut() {
                    if let Some(previous) = &mut value.found {
                        previous.recover_from(&ty);
                    } else {
                        value.found = Some(ty);
                    }
                }
            }
            None => {}
        }
    }

    fn resolve_assignment_target_type(
        &mut self,
        place_id: PlaceId,
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        if let Some(ty) = self.infer_host_path_write(place_id, env) {
            self.type_table.insert_place(place_id, ty.clone());
            return Some(ty);
        }
        let ty = match &self.lowered.module.place(place_id).kind {
            PlaceKind::Expr(expr) => {
                self.infer_expr_type(*expr, env);
                None
            }
            PlaceKind::Name(_) => {
                let ty = self.resolve_readable_place_type(place_id, env);
                let writable = self.place_root_resolution(place_id).is_some_and(|resolved| {
                    matches!(resolved, ResolvedName::Local(id) if env.local_writeability.get(&id).is_some_and(|writeability| writeability.is_var()))
                });
                ty.filter(|_| writable)
            }
            PlaceKind::Field { base, name } => {
                let base_ty = self.resolve_readable_place_type(*base, env)?;
                let field = self.resolve_field(&base_ty, name)?;
                let id = field.id.clone();
                let ty = field.ty.clone();
                let writable = field.writeability.is_var();
                self.type_table.insert_place_field(place_id, id);
                self.type_table.insert_place(place_id, ty.clone());
                writable.then_some(ty)
            }
            PlaceKind::Index { base, index } => {
                let base_ty = self.resolve_readable_place_type(*base, env);
                self.infer_expr_type(*index, env);
                let base_ty = base_ty?;
                let ty = self.resolve_index_type(*index, &base_ty);
                let fact = ty.clone().or_else(|| match &base_ty {
                    TypeId::Array(element) => Some((**element).clone()),
                    _ => None,
                });
                if let Some(fact) = fact {
                    self.type_table.insert_place(place_id, fact);
                }
                if matches!(base_ty, TypeId::Tuple(_)) {
                    self.resolve_assignment_target_type(*base, env)?;
                }
                ty
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
        // Host-target probing and assignment diagnostics may revisit a place.
        // Its checked facts already include any index diagnostics and targets.
        if let Some(ty) = self.type_table.place_type(place_id) {
            return Some(ty);
        }
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
                        | ResolvedName::StandardTrait(_)
                        | ResolvedName::StandardVariant(_)
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
                let base_ty = self.resolve_readable_place_type(*base, env);
                let index_ty = self.infer_expr_type(*index, env);
                let base_ty = base_ty?;
                let mut requested = crate::builtin::traits::StandardTrait::Index.nominal();
                requested.arguments.push(index_ty.clone());
                if !matches!(base_ty, TypeId::Array(_) | TypeId::Tuple(_))
                    && let Some((interface, result)) =
                        self.select_operator(&base_ty, requested, env)
                {
                    self.type_table.insert_place_index(place_id, interface);
                    Some(result)
                } else {
                    self.checked_index_type(*index, &base_ty, &index_ty, *index)
                }
            }
        };

        if let Some(ty) = ty.clone() {
            self.type_table.insert_place(place_id, ty);
        }

        ty
    }

    fn assignment_target_error_reason(&self, place_id: PlaceId, env: &BodyTypeEnv) -> String {
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
                    ResolvedName::StandardVariant(_) | ResolvedName::StandardModule(_) => {
                        "standard module item is not assignable".to_string()
                    }
                    ResolvedName::StandardFunction(_) | ResolvedName::RuntimeHelper(_) => {
                        "standard function item is not assignable".to_string()
                    }
                    ResolvedName::Struct(_) => "struct type is not assignable".to_string(),
                    ResolvedName::Enum(_) => "enum type is not assignable".to_string(),
                    ResolvedName::Trait(_) | ResolvedName::StandardTrait(_) => {
                        "trait type is not assignable".to_string()
                    }
                })
                .unwrap_or_else(|| "unresolved assignment target".to_string()),
            PlaceKind::Field { base, name } => {
                let Some(base_ty) = self.type_table.place_type(*base) else {
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
                let Some(base_ty) = self.type_table.place_type(*base) else {
                    return self.assignment_target_error_reason(*base, env);
                };
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
        const MAX_INFERENCE_DEPTH: usize = 64;
        if self.inference_depth >= MAX_INFERENCE_DEPTH {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::CompileLimitExceeded {
                    resource: "expression inference depth",
                    limit: MAX_INFERENCE_DEPTH,
                })
                .with_span(self.lowered.source_map.expr_span(expr_id)),
            );
            return TypeId::Unknown;
        }
        self.inference_depth += 1;
        let result = self.infer_expr_type_expected_inner(expr_id, env, expected);
        self.inference_depth -= 1;
        result
    }

    fn infer_expr_type_expected_inner(
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

        if let Some(ty) = self.infer_host_path_read(expr_id, env) {
            env.exprs.insert(expr_id, ty.clone());
            self.type_table.insert_expr(expr_id, ty.clone());
            return ty;
        }
        if let Some(ty) = self.infer_standard_constructor(expr_id, env, expected) {
            self.type_table.insert_expr(expr_id, ty.clone());
            env.exprs.insert(expr_id, ty.clone());
            return ty;
        }
        let expr = self.lowered.module.expr(expr_id);
        if let Some(ty) = self.infer_associated_const(expr_id, env) {
            env.exprs.insert(expr_id, ty.clone());
            self.type_table.insert_expr(expr_id, ty.clone());
            return ty;
        }
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
                    | ResolvedName::StandardTrait(_)
                    | ResolvedName::StandardVariant(_)
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
            ExprKind::InterpolatedString(parts) => {
                for part in parts {
                    self.infer_expr_type(*part, env);
                }
                TypeId::Builtin(BuiltinType::String)
            }
            ExprKind::FormatPart { expr, debug } => {
                let ty = self.infer_expr_type(*expr, env);
                let protocol = if *debug {
                    crate::builtin::traits::StandardTrait::Debug
                } else {
                    crate::builtin::traits::StandardTrait::Display
                };
                let completes = crate::typeck::completion::expr_can_complete(
                    &self.lowered.module,
                    self.names,
                    *expr,
                    self.cancel,
                )
                .unwrap_or(false);
                if completes
                    && self
                        .record_operator(expr_id, *expr, &ty, protocol.nominal(), env)
                        .is_none()
                    && !ty.is_unresolved()
                {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidCallTarget {
                            type_name: format!(
                                "interpolation requires {} for {}",
                                protocol.name(),
                                display_type_id(&ty)
                            ),
                        })
                        .with_span(self.lowered.source_map.expr_span(*expr)),
                    );
                }
                TypeId::Builtin(BuiltinType::String)
            }
            ExprKind::Propagate { expr } => self.infer_propagation(expr_id, *expr, env, expected),
            ExprKind::Prefix { op, expr } => self.infer_prefix_operator(expr_id, op, expr, env),
            ExprKind::Range { start, end, .. } => {
                let integer = TypeId::Builtin(BuiltinType::I32);
                let start_ty = self.infer_expr_type_expected(*start, env, Some(&integer));
                let end_ty = self.infer_expr_type_expected(*end, env, Some(&integer));
                for (operand, ty) in [(*start, start_ty), (*end, end_ty)] {
                    if ty.conflicts_with(&integer) {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::UnaryOperandTypeMismatch {
                                operator: "..",
                                expected: display_type_id(&integer),
                                found: display_type_id(&ty),
                            })
                            .with_span(self.lowered.source_map.expr_span(operand)),
                        );
                    }
                }
                TypeId::Array(Box::new(integer))
            }
            ExprKind::Binary { lhs, op, rhs } => {
                self.infer_binary_operator(expr_id, lhs, op, rhs, env)
            }
            ExprKind::Call { callee, args } => {
                if let Some(ty) = self.infer_conversion_call(expr_id, *callee, args, env, expected)
                {
                    ty
                } else if let Some(ty) =
                    self.infer_enum_constructor(expr_id, *callee, args, env, expected)
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
                    self.infer_inherent_method_call_type(expr_id, *callee, args, env, expected)
                {
                    method_ty
                } else if let Some(method_ty) =
                    self.infer_trait_method_call_type(expr_id, *callee, args, env, expected)
                {
                    method_ty
                } else {
                    self.infer_function_call_type(expr_id, *callee, args, env, expected)
                }
            }
            ExprKind::Closure { params, body } => {
                for capture in self.names.closure_captures(expr_id) {
                    let captured = match capture {
                        ResolvedName::Local(id) => env.locals.get(id),
                        ResolvedName::Param(id) => env.params.get(id),
                        _ => None,
                    };
                    if let Some(ty) = captured
                        && ty.contains_host_value()
                    {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::InvalidClosureCapture {
                                type_name: display_type_id(ty),
                            })
                            .with_span(self.lowered.source_map.expr_span(expr_id)),
                        );
                    }
                }
                let expected_params = match expected {
                    Some(TypeId::Function { params, .. }) => Some(params.as_slice()),
                    _ => None,
                };
                let expected_result = match expected {
                    Some(TypeId::Function { result, .. }) => Some(result.as_ref()),
                    _ => None,
                };
                let mut closure_env = env.clone();
                let mut param_types = Vec::with_capacity(params.len());
                for (index, param) in params.iter().enumerate() {
                    let ty = if let Some(annotation) = param.ty {
                        resolve_type_in(
                            &self.lowered.module,
                            annotation,
                            TypeContext {
                                declarations: self.declarations,
                                generics: &env.generics,
                                self_type: None,
                                implementation: None,
                            },
                            self.type_table,
                            self.cancel,
                        )
                    } else if let Some(ty) = expected_params.and_then(|types| types.get(index)) {
                        ty.clone()
                    } else {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::ExpectedType)
                                .with_span(self.lowered.source_map.local_span(param.local)),
                        );
                        TypeId::Unknown
                    };
                    self.type_table.insert_local(param.local, ty.clone());
                    closure_env.locals.insert(param.local, ty.clone());
                    param_types.push(ty);
                }
                let old_return = std::mem::replace(
                    &mut self.expected_return,
                    expected_result.cloned().unwrap_or(TypeId::Unknown),
                );
                let old_loop_depth = std::mem::replace(&mut self.loop_depth, 0);
                let old_loop_results = std::mem::take(&mut self.loop_results);
                let old_name = std::mem::replace(&mut self.function_name, "closure");
                self.closure_returns.push(Vec::new());
                let body_result =
                    self.infer_expr_with_coercion(*body, &mut closure_env, expected_result);
                let returns = self.closure_returns.pop().expect("closure return context");
                let completes = super::completion::expr_can_complete(
                    &self.lowered.module,
                    self.names,
                    *body,
                    self.cancel,
                )
                .unwrap_or(false);
                let mut result = if completes {
                    body_result
                } else {
                    expected_result
                        .cloned()
                        .or_else(|| returns.first().cloned())
                        .unwrap_or(TypeId::Unknown)
                };
                for returned in returns {
                    if result.conflicts_with(&returned) {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::ReturnTypeMismatch {
                                function_name: "closure".into(),
                                expected: display_type_id(&result),
                                found: display_type_id(&returned),
                            })
                            .with_span(self.lowered.source_map.expr_span(*body)),
                        );
                    } else {
                        result.recover_from(&returned);
                    }
                }
                self.function_name = old_name;
                self.expected_return = old_return;
                self.loop_depth = old_loop_depth;
                self.loop_results = old_loop_results;
                TypeId::Function {
                    params: param_types,
                    result: Box::new(result),
                }
            }
            ExprKind::Field { receiver, name } => {
                let receiver_ty = self.infer_expr_type(*receiver, env);
                let Ok(completes) = super::completion::expr_can_complete(
                    &self.lowered.module,
                    self.names,
                    *receiver,
                    self.cancel,
                ) else {
                    return TypeId::Unknown;
                };
                if name.is_empty() {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::ExpectedFieldName)
                            .with_span(self.lowered.source_map.expr_span(expr_id)),
                    );
                    TypeId::Error
                } else if !completes {
                    TypeId::Unknown
                } else {
                    self.checked_member_type(&receiver_ty, name, expr_id, false)
                }
            }
            ExprKind::Index { receiver, index } => {
                self.infer_index_operator(expr_id, receiver, index, env)
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let mut then_env = env.clone();
                let condition_completes = match condition {
                    crate::hir::Condition::Expr(expr) => {
                        let Ok(completes) = self.check_condition_type(*expr, "if", env) else {
                            return TypeId::Unknown;
                        };
                        completes
                    }
                    crate::hir::Condition::Binding {
                        pattern,
                        initializer,
                    } => {
                        let ty = self.infer_expr_type(*initializer, env);
                        self.check_pattern(*pattern, &ty, &mut then_env);
                        let Ok(completes) = super::completion::expr_can_complete(
                            &self.lowered.module,
                            self.names,
                            *initializer,
                            self.cancel,
                        ) else {
                            return TypeId::Unknown;
                        };
                        completes
                    }
                };
                let mut then_ty =
                    self.infer_block_types_expected(*then_branch, &mut then_env, expected);
                match else_branch {
                    Some(else_expr) => {
                        let Ok(then_completes) = super::completion::block_can_complete(
                            &self.lowered.module,
                            self.names,
                            *then_branch,
                            self.cancel,
                        ) else {
                            return TypeId::Unknown;
                        };
                        let else_context = expected
                            .or((condition_completes && then_completes).then_some(&then_ty));
                        let else_ty = self.infer_expr_with_coercion(*else_expr, env, else_context);
                        let Ok(else_completes) = super::completion::expr_can_complete(
                            &self.lowered.module,
                            self.names,
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
                    self.names,
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
                        .names
                        .pattern_is_irrefutable(&self.lowered.module, arm.pattern)
                        || arm.guard.is_some();
                    let Ok(completes) = super::completion::expr_can_complete(
                        &self.lowered.module,
                        self.names,
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
                if elements.is_empty() {
                    TypeId::Builtin(BuiltinType::Unit)
                } else {
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
                        types.push(self.infer_expr_with_coercion(*expr, env, member));
                    }
                    TypeId::Tuple(types)
                }
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
                        self.infer_expr_with_coercion(*expr, env, member.or(element_ty.as_ref()));
                    if !reachable {
                        continue;
                    }
                    let Ok(completes) = super::completion::expr_can_complete(
                        &self.lowered.module,
                        self.names,
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
            ExprKind::Loop { body } => {
                self.loop_depth += 1;
                self.loop_results
                    .push(LoopResult::Expression(Box::new(LoopValue {
                        expected: expected.cloned(),
                        found: None,
                    })));
                let _ = self.infer_block_types(*body, env);
                self.loop_depth -= 1;
                match self.loop_results.pop() {
                    Some(LoopResult::Expression(value)) => value.found.unwrap_or(TypeId::Unknown),
                    _ => TypeId::Unknown,
                }
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
            (self.aggregates, &self.declarations.hosts),
            self.type_table,
            self.lowered.source_map.expr_span(expr_id),
            self.diagnostics,
            self.cancel,
        );
        env.exprs.insert(expr_id, ty.clone());
        self.type_table.insert_expr(expr_id, ty.clone());
        ty
    }

    fn infer_expr_with_coercion(
        &mut self,
        expr_id: ExprId,
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> TypeId {
        let source = self.infer_expr_type_expected(expr_id, env, expected);
        self.apply_interface_coercion(expr_id, source, expected, env)
    }

    fn apply_interface_coercion(
        &mut self,
        expr_id: ExprId,
        source: TypeId,
        expected: Option<&TypeId>,
        env: &BodyTypeEnv,
    ) -> TypeId {
        use crate::aggregates::ImplementationSearchError;
        let Some(target @ TypeId::Trait(interface)) = expected else {
            return source;
        };
        if source == *target
            || !source.is_resolved_in(
                &env.generics
                    .iter()
                    .filter_map(|param| self.declarations.generic_type(param.id))
                    .collect::<Vec<_>>(),
            )
        {
            return source;
        }
        if let TypeId::Trait(child) = &source
            && self
                .aggregates
                .trait_closure(child, &source, self.cancel)
                .is_ok_and(|parents| parents.iter().any(|parent| parent.satisfies(interface)))
        {
            self.type_table.insert_interface_coercion(
                expr_id,
                super::ResolvedInterfaceCoercion {
                    implementation: super::ResolvedInterfaceImplementation::Upcast,
                    concrete_type: source,
                    interface_type: interface.clone(),
                },
            );
            return target.clone();
        }
        if matches!(&source, TypeId::Host(_))
            && self.declarations.hosts.implements(interface, &source)
        {
            self.type_table.insert_interface_coercion(
                expr_id,
                super::ResolvedInterfaceCoercion {
                    implementation: super::ResolvedInterfaceImplementation::Host,
                    concrete_type: source,
                    interface_type: interface.clone(),
                },
            );
            return target.clone();
        }
        match self.aggregates.concrete_interface_implementation(
            interface,
            &source,
            &env.generic_bounds,
            4096,
            64,
            self.cancel,
        ) {
            Ok(Some((implementation, arguments))) => {
                self.type_table.insert_interface_coercion(
                    expr_id,
                    super::ResolvedInterfaceCoercion {
                        implementation: super::ResolvedInterfaceImplementation::Script {
                            declaration: implementation,
                            arguments,
                        },
                        concrete_type: source,
                        interface_type: interface.clone(),
                    },
                );
                target.clone()
            }
            Ok(None) | Err(ImplementationSearchError::Cancelled) => source,
            Err(ImplementationSearchError::LimitExceeded) => {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::CompileLimitExceeded {
                        resource: "interface implementation search",
                        limit: 4096,
                    })
                    .with_span(self.lowered.source_map.expr_span(expr_id)),
                );
                source
            }
        }
    }

    fn infer_match_arm_type(
        &mut self,
        arm: &MatchArm,
        scrutinee_ty: &TypeId,
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> TypeId {
        let mut arm_env = env.clone();
        self.check_pattern(arm.pattern, scrutinee_ty, &mut arm_env);
        if let Some(guard) = arm.guard
            && self
                .check_condition_type(guard, "match guard", &mut arm_env)
                .is_err()
        {
            return TypeId::Unknown;
        }
        self.infer_expr_with_coercion(arm.expr, &mut arm_env, expected)
    }

    fn check_pattern(
        &mut self,
        pattern: crate::hir::PatternId,
        expected: &TypeId,
        env: &mut BodyTypeEnv,
    ) {
        let span = self.lowered.source_map.pattern_span(pattern);
        if self.check_standard_pattern(pattern, expected, env) {
            return;
        }
        match &self.lowered.module.pattern(pattern).kind {
            PatternKind::Wildcard => {}
            PatternKind::Or(alternatives) => {
                let original = env.clone();
                let mut canonical = None;
                for (index, alternative) in alternatives.iter().copied().enumerate() {
                    let mut branch = original.clone();
                    self.check_pattern(alternative, expected, &mut branch);
                    let (binding_ids, duplicate) = self.pattern_binding_ids(alternative);
                    let names = binding_ids
                        .into_iter()
                        .filter_map(|(name, local)| {
                            branch.locals.get(&local).cloned().map(|ty| (name, ty))
                        })
                        .collect::<BTreeMap<_, _>>();
                    if duplicate {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                                expected: "each name bound once per alternative".into(),
                                found: format!("bindings {names:?}"),
                            })
                            .with_span(self.lowered.source_map.pattern_span(alternative)),
                        );
                    }
                    if let Some(first) = &canonical
                        && (first != &names)
                    {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                                expected: "the same bindings in every alternative".into(),
                                found: format!("bindings {names:?}"),
                            })
                            .with_span(self.lowered.source_map.pattern_span(alternative)),
                        );
                    }
                    if index == 0 {
                        *env = branch;
                        canonical = Some(names);
                    }
                }
            }
            PatternKind::Range { start, end, .. } => {
                let integer = TypeId::Builtin(BuiltinType::I32);
                if expected.conflicts_with(&integer) {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(expected),
                            found: "i32 range pattern".into(),
                        })
                        .with_span(span),
                    );
                }
                let start = self.resolve_pattern_bound(start, span);
                let end = self.resolve_pattern_bound(end, span);
                if let (Some(start), Some(end)) = (start, end) {
                    if !matches!(start, super::ScalarValue::I32(_))
                        || !matches!(end, super::ScalarValue::I32(_))
                    {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                                expected: "i32 bounds".into(),
                                found: "non-integer range bound".into(),
                            })
                            .with_span(span),
                        );
                    } else {
                        self.type_table.insert_pattern_range(pattern, start, end);
                    }
                }
            }
            PatternKind::Name { local, .. } => {
                env.locals.insert(*local, expected.clone());
                self.type_table.insert_local(*local, expected.clone());
            }
            PatternKind::Literal(literal) => match super::ScalarValue::parse(literal) {
                Ok(value) => {
                    if value.ty().conflicts_with(expected) {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                                expected: display_type_id(expected),
                                found: display_type_id(&value.ty()),
                            })
                            .with_span(span),
                        );
                    }
                    self.type_table.insert_pattern_scalar(pattern, value);
                }
                Err(reason) => self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidLiteral {
                        reason: reason.to_owned(),
                    })
                    .with_span(span),
                ),
            },
            PatternKind::Tuple(elements) => {
                let TypeId::Tuple(types) = expected else {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(expected),
                            found: "tuple pattern".into(),
                        })
                        .with_span(span),
                    );
                    return;
                };
                if elements.len() != types.len() {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(expected),
                            found: format!("tuple of {} elements", elements.len()),
                        })
                        .with_span(span),
                    );
                    return;
                }
                let elements = elements.clone();
                let types = types.clone();
                for (element, ty) in elements.into_iter().zip(types.iter()) {
                    self.check_pattern(element, ty, env);
                }
            }
            PatternKind::Struct { path, fields } => {
                let path = path.clone();
                let fields = fields.clone();
                let TypeId::Struct(owner) = expected else {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(expected),
                            found: format!("struct pattern `{path}`"),
                        })
                        .with_span(span),
                    );
                    return;
                };
                if self.resolve_struct_id(&path).as_ref() != Some(&owner.declaration) {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(expected),
                            found: format!("struct pattern `{path}`"),
                        })
                        .with_span(span),
                    );
                    return;
                }
                let mut identities = Vec::new();
                let mut seen = HashSet::new();
                for field in fields {
                    if !seen.insert(field.name.clone()) {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                                expected: "distinct struct fields".into(),
                                found: format!("duplicate field `{}`", field.name),
                            })
                            .with_span(self.lowered.source_map.pattern_span(field.pattern)),
                        );
                        continue;
                    }
                    let Some(signature) = self.resolve_field(expected, &field.name) else {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                                expected: display_type_id(expected),
                                found: format!("unknown field `{}`", field.name),
                            })
                            .with_span(self.lowered.source_map.pattern_span(field.pattern)),
                        );
                        continue;
                    };
                    identities.push(signature.id);
                    self.check_pattern(field.pattern, &signature.ty, env);
                }
                self.type_table.insert_pattern_fields(pattern, identities);
            }
            PatternKind::EnumVariant { path, fields } => {
                let path = path.clone();
                let fields = fields.clone();
                let TypeId::Enum(owner) = expected else {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(expected),
                            found: format!("enum variant `{path}`"),
                        })
                        .with_span(span),
                    );
                    return;
                };
                let Some((owner_path, variant_name)) = path.rsplit_once("::") else {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(expected),
                            found: format!("enum variant `{path}`"),
                        })
                        .with_span(span),
                    );
                    return;
                };
                if self.resolve_enum_id(owner_path).as_ref() != Some(&owner.declaration) {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(expected),
                            found: format!("enum variant `{path}`"),
                        })
                        .with_span(span),
                    );
                    return;
                }
                let Some(enumeration) = self.aggregates.enumeration(&owner.declaration) else {
                    return;
                };
                let Some(variant) = enumeration
                    .variants
                    .iter()
                    .find(|variant| variant.name == variant_name)
                    .cloned()
                else {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(expected),
                            found: format!("unknown enum variant `{path}`"),
                        })
                        .with_span(span),
                    );
                    return;
                };
                if variant.payload.len() != fields.len() {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: format!("{} payload fields", variant.payload.len()),
                            found: format!("{} payload fields", fields.len()),
                        })
                        .with_span(span),
                    );
                    return;
                }
                let substitution = enumeration
                    .generic_params
                    .iter()
                    .cloned()
                    .zip(owner.arguments.iter().cloned())
                    .collect();
                self.type_table.insert_pattern_variant(pattern, variant.id);
                for (field, ty) in fields.into_iter().zip(variant.payload) {
                    self.check_pattern(field, &ty.instantiate(&substitution), env);
                }
            }
        }
    }

    fn pattern_binding_ids(
        &self,
        pattern: crate::hir::PatternId,
    ) -> (HashMap<String, crate::hir::LocalId>, bool) {
        let mut names = HashMap::new();
        let mut duplicate = false;
        let mut work = vec![pattern];
        while let Some(pattern) = work.pop() {
            match &self.lowered.module.pattern(pattern).kind {
                PatternKind::Name { name, local } => {
                    duplicate |= names.insert(name.clone(), *local).is_some();
                }
                PatternKind::Or(alternatives) => work.extend(alternatives.first().copied()),
                PatternKind::Tuple(alternatives)
                | PatternKind::EnumVariant {
                    fields: alternatives,
                    ..
                } => work.extend(alternatives.iter().copied()),
                PatternKind::Struct { fields, .. } => {
                    work.extend(fields.iter().map(|field| field.pattern));
                }
                PatternKind::Wildcard | PatternKind::Literal(_) | PatternKind::Range { .. } => {}
            }
        }
        (names, duplicate)
    }

    fn resolve_pattern_bound(
        &mut self,
        bound: &PatternBound,
        span: kagari_common::Span,
    ) -> Option<super::ScalarValue> {
        let value = match bound {
            PatternBound::Literal(literal) => super::ScalarValue::parse(literal).ok(),
            PatternBound::Path(path) => self
                .declarations
                .names
                .lookup(path)
                .and_then(|entry| entry.target())
                .and_then(|target| match target {
                    ResolvedName::Const(id) => self.const_values?.get(&id).cloned(),
                    _ => None,
                }),
        };
        if value.is_none() {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                    expected: "scalar constant range bound".into(),
                    found: format!("{bound:?}"),
                })
                .with_span(span),
            );
        }
        value
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
                call_expr,
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
        Some(self.infer_standard_intrinsic_type(call_expr, intrinsic, callee, None, args, env))
    }

    fn check_standard_parameter(
        &mut self,
        spec: &surface::StandardFunctionSpec,
        parameter: &crate::builtin::declarations::ApiType,
        actual: &TypeId,
        env: &BodyTypeEnv,
        site: ExprId,
    ) {
        let mut arguments = spec
            .type_params
            .iter()
            .map(|name| (*name, TypeId::Unknown))
            .collect();
        parameter.infer(actual, &mut arguments);
        for constraint in spec.constraints {
            if let Some(actual) = arguments.get(constraint.param) {
                self.check_standard_constraint(actual, constraint.constraint, env, site);
            }
        }
    }

    fn infer_standard_intrinsic_type(
        &mut self,
        call_expr: ExprId,
        intrinsic: StandardIntrinsic,
        callee: ExprId,
        receiver_ty: Option<TypeId>,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> TypeId {
        use crate::builtin::declarations::Arguments;
        let Some(spec) = surface::standard_function_by_intrinsic(intrinsic) else {
            return TypeId::Error;
        };
        let api = spec.api;
        let name = standard_intrinsic_name(intrinsic);
        let offset = usize::from(receiver_ty.is_some());
        self.check_builtin_arity(
            name,
            api.params.len().saturating_sub(offset),
            args.len(),
            callee,
        );
        let mut bindings: Arguments = spec
            .type_params
            .iter()
            .map(|name| (*name, TypeId::Unknown))
            .collect();
        if let Some(receiver) = receiver_ty {
            self.check_standard_parameter(spec, &api.params[0].ty, &receiver, env, callee);
            api.params[0].ty.infer(&receiver, &mut bindings);
            let expected = api.params[0].ty.instantiate(&bindings);
            if expected.conflicts_with(&receiver) {
                self.emit_arg_mismatch(name, api.params[0].name, &expected, &receiver, callee);
            }
        }
        for (index, argument) in args.iter().enumerate() {
            if self.cancel.check().is_err() {
                return TypeId::Unknown;
            }
            let parameter = api.params.get(index + offset);
            let expected = parameter.map(|p| p.ty.instantiate(&bindings));
            let actual = self.infer_expr_with_coercion(*argument, env, expected.as_ref());
            if !super::completion::expr_can_complete(
                &self.lowered.module,
                self.names,
                *argument,
                self.cancel,
            )
            .unwrap_or(false)
            {
                continue;
            }
            if let Some(parameter) = parameter {
                self.check_standard_parameter(spec, &parameter.ty, &actual, env, *argument);
                parameter.ty.infer(&actual, &mut bindings);
                let expected = parameter.ty.instantiate(&bindings);
                if expected.conflicts_with(&actual) {
                    self.emit_arg_mismatch(name, parameter.name, &expected, &actual, *argument);
                }
            }
        }
        self.type_table.insert_type_arguments(
            call_expr,
            spec.type_params
                .iter()
                .map(|name| bindings[*name].clone())
                .collect(),
        );
        api.result.instantiate(&bindings)
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
                let Ok(base_ty) = self.infer_reflection_receiver(*base, env) else {
                    return Some(TypeId::Unknown);
                };
                let Some(field_name) =
                    self.checked_reflection_field_name(*field_name_expr, env, "get_field")
                else {
                    return Some(TypeId::Error);
                };
                Some(base_ty.map_or(TypeId::Unknown, |base_ty| {
                    self.checked_member_type(&base_ty, &field_name, *field_name_expr, false)
                }))
            }
            BuiltinFunction::SetField => {
                let [base, field_name_expr, value] = args else {
                    return Some(TypeId::Error);
                };
                let Ok(base_ty) = self.infer_reflection_receiver(*base, env) else {
                    return Some(TypeId::Unknown);
                };
                self.check_const_write(*base);
                let field_name =
                    self.checked_reflection_field_name(*field_name_expr, env, "set_field");
                let expected = field_name
                    .as_ref()
                    .zip(base_ty.as_ref())
                    .map(|(name, base_ty)| {
                        self.checked_member_type(base_ty, name, *field_name_expr, true)
                    });
                self.check_reflection_assignment_value(*value, expected.as_ref(), env);
                if field_name.is_none() {
                    return Some(TypeId::Error);
                }
                Some(base_ty.unwrap_or(TypeId::Unknown))
            }
            BuiltinFunction::SetIndex => {
                let [base, index, value] = args else {
                    return Some(TypeId::Error);
                };
                let Ok(base_ty) = self.infer_reflection_receiver(*base, env) else {
                    return Some(TypeId::Unknown);
                };
                self.check_const_write(*base);
                let index_ty = self.infer_expr_type(*index, env);
                let expected = base_ty.as_ref().and_then(|base_ty| {
                    self.checked_index_type(*index, base_ty, &index_ty, *index)
                });
                self.check_reflection_assignment_value(*value, expected.as_ref(), env);
                Some(base_ty.unwrap_or(TypeId::Unknown))
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
    fn infer_reflection_receiver(
        &mut self,
        receiver: ExprId,
        env: &mut BodyTypeEnv,
    ) -> Result<Option<TypeId>, kagari_common::cancellation::Cancelled> {
        let ty = self.infer_expr_type(receiver, env);
        let completes = super::completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            receiver,
            self.cancel,
        )?;
        Ok(completes.then_some(ty))
    }

    fn check_reflection_assignment_value(
        &mut self,
        value: ExprId,
        expected: Option<&TypeId>,
        env: &mut BodyTypeEnv,
    ) {
        let found = self.infer_expr_with_coercion(value, env, expected);
        let Ok(completes) = super::completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            value,
            self.cancel,
        ) else {
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

    fn infer_inherent_method_call_type(
        &mut self,
        call_expr: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> Option<TypeId> {
        let ExprKind::Field { receiver, name } = &self.lowered.module.expr(callee).kind else {
            return None;
        };
        let receiver = *receiver;
        let name = name.clone();
        let receiver_ty = self.infer_expr_type(receiver, env);
        if receiver_ty.is_unresolved() {
            return None;
        }
        let candidates = self
            .aggregates
            .inherent_methods()
            .filter(|method| {
                method.function.name == name
                    && method.visibility.allows(
                        &method.declaration.module,
                        self.lowered.source.module_identity(),
                    )
            })
            .filter_map(|method| {
                let function = method.function.clone();
                let mut substitution = crate::types::TypeSubstitution::default();
                let generics = function.generic_params.as_slice();
                if super::inference::infer(
                    &method.owner,
                    &receiver_ty,
                    generics,
                    &mut substitution,
                    self.cancel,
                )
                .is_err()
                {
                    return None;
                }
                let target = if method.id.file == self.lowered.source.id()
                    && method.id.revision == self.lowered.source.revision()
                {
                    CallTarget::Function(method.id.function)
                } else {
                    CallTarget::SourceFunction(method.id)
                };
                Some((function, substitution, target))
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return None;
        }
        if candidates.len() != 1 {
            self.infer_call_args(args, env);
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::AmbiguousMethod { name })
                    .with_span(self.lowered.source_map.expr_span(callee)),
            );
            return Some(TypeId::Error);
        }
        let (function, mut substitution, target) =
            candidates.into_iter().next().expect("one method");
        if matches!(target, CallTarget::SourceFunction(_)) && !function.generic_params.is_empty() {
            self.infer_call_args(args, env);
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::PublicGenericFunction {
                    name: function.name.clone(),
                })
                .with_span(self.lowered.source_map.expr_span(callee)),
            );
            return Some(TypeId::Error);
        }
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
            return Some(TypeId::Unknown);
        }
        self.type_table
            .insert_call(call_expr, target, Some(receiver));
        let arg_tys = self.infer_generic_args(
            args,
            function
                .params
                .iter()
                .skip(1)
                .map(|parameter| parameter.ty.clone()),
            &function.generic_params,
            &mut substitution,
            env,
        );
        let suppress_missing = arg_tys.iter().any(|(_, ty)| ty.is_unresolved());
        let type_arguments = self.finish_inferred_arguments(
            &mut substitution,
            &function.generic_params,
            &function.name,
            callee,
            suppress_missing,
        );
        self.type_table
            .insert_type_arguments(call_expr, type_arguments);
        self.check_generic_call_bounds(
            &function.generic_params,
            &function.bounds,
            &substitution,
            env,
            callee,
        );
        let mut all_args = Vec::with_capacity(arg_tys.len() + 1);
        all_args.push((receiver, receiver_ty));
        all_args.extend(arg_tys);
        self.check_function_arguments(&function, &substitution, callee, &all_args);
        Some(
            self.aggregates
                .normalize_type(&function.return_type.instantiate(&substitution)),
        )
    }

    fn infer_associated_const(&mut self, expr: ExprId, env: &BodyTypeEnv) -> Option<TypeId> {
        let ExprKind::Name {
            name,
            explicit_type,
        } = &self.lowered.module.expr(expr).kind
        else {
            return None;
        };
        let context = TypeContext {
            declarations: self.declarations,
            generics: &env.generics,
            self_type: None,
            implementation: None,
        };
        let qualified = explicit_type.and_then(|id| match &self.lowered.module.type_ref(id).kind {
            crate::hir::TypeKind::Projection {
                receiver,
                trait_ref,
                member,
                arguments,
            } if arguments.is_empty() => Some((*receiver, *trait_ref, member.clone())),
            _ => None,
        });
        let (receiver, member, requested) = if let Some((receiver, trait_ref, member)) = qualified {
            let receiver = if matches!(&self.lowered.module.type_ref(receiver).kind, crate::hir::TypeKind::Named(name) if name == "Self")
            {
                env.self_type.clone().unwrap_or(TypeId::Error)
            } else {
                resolve_type_in(
                    &self.lowered.module,
                    receiver,
                    context,
                    self.type_table,
                    self.cancel,
                )
            };
            let interface = resolve_type_in(
                &self.lowered.module,
                trait_ref,
                context,
                self.type_table,
                self.cancel,
            );
            (receiver, member, Some(interface))
        } else {
            let (owner, member) = name.rsplit_once("::")?;
            let receiver = if let Some(ty) = explicit_type {
                resolve_type_in(
                    &self.lowered.module,
                    *ty,
                    context,
                    self.type_table,
                    self.cancel,
                )
            } else if owner == "Self" {
                env.self_type.clone().unwrap_or(TypeId::Error)
            } else {
                super::ty::resolve_named_type(owner, context).ty
            };
            (receiver, member.to_owned(), None)
        };
        if receiver.is_unresolved() {
            return None;
        }
        let mut candidates = Vec::new();
        for interface in self.trait_bounds_for(&receiver, env) {
            if requested.as_ref().is_some_and(|requested| !matches!(requested, TypeId::Trait(required) if interface.satisfies(required))) { continue; }
            let id = crate::types::associated_const_id(&interface.declaration, &member);
            if let Some(signature) = self
                .aggregates
                .trait_(&interface.declaration)
                .and_then(|contract| contract.associated_consts.get(&id))
                && !candidates
                    .iter()
                    .any(|(candidate, _, _)| candidate == &interface)
            {
                candidates.push((interface, id, signature.ty.clone()));
            }
        }
        if candidates.len() == 1 {
            let (interface, member, ty) = candidates.remove(0);
            self.type_table.insert_associated_const(
                expr,
                super::ResolvedAssociatedConst {
                    receiver,
                    interface,
                    member,
                },
            );
            return Some(ty);
        }
        if candidates.len() > 1 || requested.is_some() {
            self.diagnostics.push(Diagnostic::error(DiagnosticKind::InvalidAssociatedConst { name: member,
                reason: "requires a unique constant from a satisfied trait; use a qualified path to disambiguate".into(),
            }).with_span(self.lowered.source_map.expr_span(expr)));
            return Some(TypeId::Error);
        }
        None
    }

    fn trait_bounds_for(&self, ty: &TypeId, env: &BodyTypeEnv) -> Vec<crate::types::NominalType> {
        if let TypeId::Trait(interface) = ty {
            return self
                .aggregates
                .trait_closure(interface, ty, self.cancel)
                .unwrap_or_default();
        }
        if !matches!(
            ty,
            TypeId::Generic(_) | TypeId::SelfType(_) | TypeId::Projection { .. }
        ) {
            let mut implemented = Vec::new();
            for implementation in self.aggregates.implementations() {
                let mut substitution = crate::types::TypeSubstitution::default();
                if super::inference::infer(
                    &implementation.for_type,
                    ty,
                    &implementation.generic_params,
                    &mut substitution,
                    self.cancel,
                )
                .is_ok()
                {
                    let applied = implementation.trait_type.instantiate(&substitution);
                    if matches!(
                        self.aggregates.concrete_interface_implementation(
                            &applied,
                            ty,
                            &env.generic_bounds,
                            100_000,
                            64,
                            self.cancel
                        ),
                        Ok(Some(_))
                    ) && !implemented.contains(&applied)
                    {
                        implemented.push(applied);
                    }
                }
            }
            for kind in crate::builtin::traits::StandardTrait::ALL {
                let interface = kind.intrinsic_view(ty);
                if crate::builtin::traits::intrinsic_holds(
                    kind,
                    ty,
                    Some(self.aggregates),
                    &env.generic_bounds,
                ) && !implemented.contains(&interface)
                {
                    implemented.push(interface);
                }
            }
            if !implemented.is_empty() {
                self.add_iterator_view(ty, &mut implemented);
                return implemented;
            }
        }
        let mut bounds = env
            .generic_bounds
            .get(ty)
            .into_iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        if let TypeId::Projection {
            receiver,
            interface,
            member,
            arguments,
        } = ty
            && let Some(contract) = self.aggregates.trait_(&interface.declaration)
        {
            let mut substitution: crate::types::TypeSubstitution = contract
                .generic_params
                .iter()
                .cloned()
                .zip(interface.arguments.iter().cloned())
                .collect();
            substitution.insert_receiver(interface.declaration.clone(), (**receiver).clone());
            if let Some(inputs) = contract.associated_type_parameters.get(member) {
                substitution.extend(
                    inputs
                        .parameters
                        .iter()
                        .cloned()
                        .zip(arguments.iter().cloned()),
                );
            }
            bounds.extend(
                contract
                    .associated_types
                    .get(member)
                    .into_iter()
                    .flatten()
                    .map(|bound| match bound {
                        super::ConstraintTarget::Standard(value) => {
                            super::ConstraintTarget::Standard(*value)
                        }
                        super::ConstraintTarget::Trait(value) => {
                            super::ConstraintTarget::Trait(value.instantiate(&substitution))
                        }
                    }),
            );
        }
        let direct = bounds
            .into_iter()
            .filter_map(|bound| match bound {
                super::ConstraintTarget::Trait(ty) => Some(ty),
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut expanded = Vec::new();
        for interface in direct {
            for parent in self
                .aggregates
                .trait_closure(&interface, ty, self.cancel)
                .unwrap_or_default()
            {
                if !expanded.contains(&parent) {
                    expanded.push(parent);
                }
            }
        }
        self.add_iterator_view(ty, &mut expanded);
        expanded
    }

    fn infer_trait_method_call_type(
        &mut self,
        call_expr: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> Option<TypeId> {
        let expr = self.lowered.module.expr(callee);
        let ExprKind::Field { receiver, name } = &expr.kind else {
            return None;
        };
        let receiver_ty = self.infer_expr_type(*receiver, env);
        let mut trait_types = self.trait_bounds_for(&receiver_ty, env);
        // Arrays support every builtin integer index type. Method selection must
        // apply Index to the actual argument, just like bracket expressions.
        if matches!(receiver_ty, TypeId::Array(_)) && name == "index" && args.len() == 1 {
            let index_ty = self.infer_expr_type(args[0], env);
            let protocol = crate::builtin::traits::StandardTrait::Index;
            let mut requested = protocol.nominal();
            requested.arguments.push(index_ty);
            if let Some((interface, _)) = self.select_operator(&receiver_ty, requested, env) {
                trait_types.retain(|candidate| candidate.declaration != interface.declaration);
                trait_types.push(interface);
            }
        }
        let mut candidates = Vec::new();
        for interface in trait_types {
            if let Some(contract) = self.aggregates.trait_(&interface.declaration) {
                for method in &contract.methods {
                    if method.name == *name
                        && method
                            .params
                            .first()
                            .is_some_and(|parameter| parameter.name == "self")
                        && !candidates.contains(&(method.id.clone(), interface.clone()))
                    {
                        candidates.push((method.id.clone(), interface.clone()));
                    }
                }
            }
        }
        if candidates.is_empty() {
            return None;
        }
        // Applied standard protocols can select a unique RHS implementation.
        // Unrelated same-named traits retain ordinary ambiguity diagnostics.
        if candidates.len() > 1
            && args.len() == 1
            && let Some(protocol) =
                crate::builtin::traits::StandardTrait::from_id(&candidates[0].1.declaration)
            && (protocol.binary_operator()
                || protocol == crate::builtin::traits::StandardTrait::Index)
            && candidates
                .iter()
                .all(|(_, interface)| interface.declaration == protocol.contract().id)
        {
            let argument = self.infer_expr_type(args[0], env);
            let applicable: Vec<_> = candidates
                .iter()
                .filter(|(_, interface)| {
                    interface
                        .arguments
                        .first()
                        .is_some_and(|input| !input.conflicts_with(&argument))
                })
                .cloned()
                .collect();
            if !applicable.is_empty() {
                candidates = applicable;
            }
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
            .trait_method(&candidates[0].0)
            .expect("catalog method")
            .clone();
        let interface = &candidates[0].1;
        let trait_contract = self
            .aggregates
            .trait_(&interface.declaration)
            .expect("catalog trait");
        let mut substitution: crate::types::TypeSubstitution = trait_contract
            .generic_params
            .iter()
            .cloned()
            .zip(interface.arguments.iter().cloned())
            .collect();
        let self_owner = &method.owner;
        let self_ty = receiver_ty;
        substitution.insert_receiver(self_owner.clone(), self_ty.clone());
        let method_generics = &method.generic_params[trait_contract.generic_params.len()..];
        let return_pattern = method
            .return_type
            .with_self(self_owner, &self_ty)
            .instantiate(&substitution)
            .with_associated_types(interface);
        if let Some(expected) = expected
            && super::inference::infer(
                &return_pattern,
                expected,
                method_generics,
                &mut substitution,
                self.cancel,
            )
            .is_err()
        {
            return Some(TypeId::Unknown);
        }
        self.type_table.insert_call(
            call_expr,
            CallTarget::TraitMethod {
                method: method.id.clone(),
                interface: interface.clone(),
            },
            Some(*receiver),
        );
        let params = method
            .params
            .iter()
            .filter(|param| param.name != "self")
            .collect::<Vec<_>>();
        let param_types = params
            .iter()
            .map(|param| {
                param
                    .ty
                    .with_self(self_owner, &self_ty)
                    .instantiate(&substitution)
                    .with_associated_types(interface)
            })
            .collect::<Vec<_>>();
        let arg_tys = self.infer_generic_args(
            args,
            param_types.iter().cloned(),
            method_generics,
            &mut substitution,
            env,
        );
        let mut suppress_missing = arg_tys.iter().any(|(_, ty)| ty.is_unresolved());
        for (argument, _) in &arg_tys {
            let Ok(completes) = super::completion::expr_can_complete(
                &self.lowered.module,
                self.names,
                *argument,
                self.cancel,
            ) else {
                return Some(TypeId::Unknown);
            };
            suppress_missing |= !completes;
        }
        let type_arguments = self.finish_inferred_arguments(
            &mut substitution,
            method_generics,
            &method.name,
            callee,
            suppress_missing,
        );
        self.type_table
            .insert_type_arguments(call_expr, type_arguments);
        let method_bounds = method
            .bounds
            .iter()
            .map(|(ty, constraints)| {
                (
                    ty.with_self(self_owner, &self_ty)
                        .instantiate(&substitution)
                        .with_associated_types(interface),
                    constraints
                        .iter()
                        .map(|constraint| match constraint {
                            super::ConstraintTarget::Standard(standard) => {
                                super::ConstraintTarget::Standard(*standard)
                            }
                            super::ConstraintTarget::Trait(bound) => {
                                super::ConstraintTarget::Trait(bound.instantiate(&substitution))
                            }
                        })
                        .collect(),
                )
            })
            .collect();
        self.check_generic_call_bounds(method_generics, &method_bounds, &substitution, env, callee);
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
                param_types[index].instantiate(&substitution),
                index,
                &arg_tys,
            );
        }

        Some(
            self.aggregates
                .normalize_type(&return_pattern.instantiate(&substitution)),
        )
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
                implementation: None,
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
        let mut substitution = crate::types::TypeSubstitution::default();
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
        let Ok(completes) = super::completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            expression,
            self.cancel,
        ) else {
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
            associated_types: Default::default(),
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
                imported
                    .signature
                    .params
                    .iter()
                    .map(|p| self.aggregates.normalize_type(&p.ty))
                    .collect::<Vec<_>>()
                    .into_iter(),
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
            return self
                .aggregates
                .normalize_type(&imported.signature.return_type);
        }
        let Some(ResolvedName::Function(id)) = self.names.expr_resolution(callee) else {
            let callee_ty = self.infer_expr_type(callee, env);
            if let TypeId::Function { params, result } = &callee_ty {
                let arguments = self.infer_typed_args(args, params.iter().cloned(), env);
                self.type_table
                    .insert_call(call_expr, CallTarget::Value, Some(callee));
                self.check_builtin_arity("closure", params.len(), args.len(), callee);
                for (index, ty) in params.iter().enumerate() {
                    self.check_arg_type(
                        "closure",
                        &format!("arg{index}"),
                        ty.clone(),
                        index,
                        &arguments,
                    );
                }
                return result.as_ref().clone();
            }
            self.infer_call_args(args, env);
            let Ok(completes) = super::completion::expr_can_complete(
                &self.lowered.module,
                self.names,
                callee,
                self.cancel,
            ) else {
                return TypeId::Unknown;
            };
            if !completes {
                self.type_table
                    .insert_call(call_expr, CallTarget::TerminatingCallee, Some(callee));
                return TypeId::Unknown;
            }
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
        let mut substitution = crate::types::TypeSubstitution::default();
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
            let Ok(completes) = super::completion::expr_can_complete(
                &self.lowered.module,
                self.names,
                *argument,
                self.cancel,
            ) else {
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
        self.check_generic_call_bounds(
            &function.generic_params,
            &function.bounds,
            &substitution,
            env,
            callee,
        );
        self.check_function_arguments(function, &substitution, callee, &arg_tys);
        self.aggregates
            .normalize_type(&function.return_type.instantiate(&substitution))
    }

    fn check_generic_call_bounds(
        &mut self,
        _parameters: &[crate::types::GenericParameterType],
        bounds: &super::GenericBounds,
        substitution: &crate::types::TypeSubstitution,
        env: &BodyTypeEnv,
        callee: ExprId,
    ) {
        for (target, constraints) in bounds {
            let actual_owned = self
                .aggregates
                .normalize_type(&target.instantiate(substitution));
            let actual = &actual_owned;
            for constraint in constraints.iter().cloned() {
                match constraint {
                    super::ConstraintTarget::Standard(constraint) => {
                        self.check_standard_constraint(actual, constraint, env, callee)
                    }
                    super::ConstraintTarget::Trait(trait_type) => {
                        let trait_type = trait_type.instantiate(substitution);
                        let satisfied = self.aggregates.intrinsic_implementation(
                            &trait_type,
                            actual,
                            &env.generic_bounds,
                        ) || match actual {
                            TypeId::Generic(_)
                            | TypeId::SelfType(_)
                            | TypeId::Projection { .. } => self
                                .trait_bounds_for(actual, env)
                                .iter()
                                .any(|bound| bound.satisfies(&trait_type)),
                            _ => match self.aggregates.implementation_count(&trait_type, actual)
                                + usize::from(
                                    self.declarations.hosts.implements(&trait_type, actual),
                                ) {
                                0 => {
                                    crate::builtin::traits::StandardTrait::from_id(
                                        &trait_type.declaration,
                                    )
                                    .is_none()
                                        && self.type_table.implements(&trait_type, actual)
                                }
                                1 => true,
                                _ => false,
                            },
                        };
                        if !satisfied && !actual.is_unresolved() {
                            let trait_name = self
                                .aggregates
                                .trait_(&trait_type.declaration)
                                .map(|contract| contract.declaration.name.clone())
                                .or_else(|| {
                                    trait_type
                                        .declaration
                                        .path
                                        .last()
                                        .map(|segment| segment.name.clone())
                                })
                                .unwrap_or_default();
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
                self.aggregates
                    .normalize_type(&param.ty.instantiate(substitution)),
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

    fn resolve_host_declared_field(
        &self,
        owner: &TypeId,
        name: &str,
    ) -> Option<kagari_common::host_interface::HostFieldDeclaration> {
        let TypeId::Host(id) = owner else {
            return None;
        };
        self.names
            .hosts
            .nominal_type(id)
            .and_then(|id| self.names.hosts.type_declaration(id))
            .and_then(|owner| owner.fields.iter().find(|field| field.name == name))
            .cloned()
    }

    fn infer_host_path_read(&mut self, expr_id: ExprId, env: &mut BodyTypeEnv) -> Option<TypeId> {
        use kagari_common::host_interface::HostPathSegmentDeclaration as Segment;
        let mut root = expr_id;
        let mut steps = Vec::new();
        loop {
            match &self.lowered.module.expr(root).kind {
                ExprKind::Field { receiver, name } => {
                    steps.push(HostPathNode::Member {
                        node: root,
                        name: name.clone(),
                    });
                    root = *receiver;
                }
                ExprKind::Index { receiver, index } => {
                    steps.push(HostPathNode::Index {
                        node: root,
                        argument: *index,
                    });
                    root = *receiver;
                }
                _ => break,
            }
        }
        if steps.is_empty() {
            return None;
        }
        steps.reverse();
        let mut root_ty = self.infer_expr_type(root, env);
        let mut prefix = 0;
        while !matches!(root_ty, TypeId::Host(_)) && prefix + 1 < steps.len() {
            root = steps[prefix].id();
            root_ty = self.infer_expr_type(root, env);
            prefix += 1;
        }
        steps.drain(..prefix);
        let TypeId::Host(owner) = root_ty else {
            return None;
        };
        let source = steps
            .iter()
            .map(|step| match step {
                HostPathNode::Member { name, .. } => {
                    crate::host::HostSourcePathStep::Member(name.clone())
                }
                HostPathNode::Index { argument, .. } => {
                    crate::host::HostSourcePathStep::Index(self.infer_expr_type(*argument, env))
                }
            })
            .collect::<Vec<_>>();
        match self.names.hosts.source_path(&owner, &source) {
            Ok((declaration, contract)) => {
                let mut dynamic_arguments = Vec::new();
                for ((step, declared), result) in steps
                    .iter()
                    .zip(&declaration.segments)
                    .zip(&contract.segments)
                {
                    if let (HostPathNode::Member { node, .. }, Segment::Field(id)) =
                        (step, declared)
                    {
                        self.type_table.insert_expr_field(*node, id.clone());
                    }
                    if let (HostPathNode::Index { argument, .. }, Segment::Index(index)) =
                        (step, declared)
                    {
                        dynamic_arguments.push((index.slot, *argument));
                    }
                    let ty = crate::host::signature_type(&result.result);
                    self.type_table.insert_expr(step.id(), ty.clone());
                    env.exprs.insert(step.id(), ty);
                }
                let result = crate::host::signature_type(&contract.result);
                self.type_table.insert_host_path(
                    expr_id,
                    super::ResolvedHostPath {
                        root,
                        dynamic_arguments,
                        declaration,
                        contract,
                    },
                );
                Some(result)
            }
            Err(reason) => {
                let mut current = TypeId::Host(owner);
                let mut recovered_all_members = true;
                for step in &steps {
                    match step {
                        HostPathNode::Member { node, name } => {
                            let Some(field) = self.resolve_host_declared_field(&current, name)
                            else {
                                recovered_all_members = false;
                                break;
                            };
                            self.type_table.insert_expr_field(*node, field.id.clone());
                            current = crate::host::signature_type(&field.ty);
                        }
                        HostPathNode::Index { .. } => {
                            recovered_all_members = false;
                            if let TypeId::Array(element) = current {
                                current = *element;
                            } else {
                                break;
                            }
                        }
                    }
                    self.type_table.insert_expr(step.id(), current.clone());
                    env.exprs.insert(step.id(), current.clone());
                }
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidHostPath {
                        reason: reason.into(),
                    })
                    .with_span(self.lowered.source_map.expr_span(expr_id)),
                );
                Some(if recovered_all_members {
                    current
                } else {
                    TypeId::Error
                })
            }
        }
    }

    fn infer_host_path_write(
        &mut self,
        place_id: PlaceId,
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        use kagari_common::host_interface::{HostPathSegmentDeclaration as Segment, PathAccess};
        let mut root = place_id;
        let mut steps = Vec::new();
        loop {
            match &self.lowered.module.place(root).kind {
                PlaceKind::Field { base, name } => {
                    steps.push(HostPathNode::Member {
                        node: root,
                        name: name.clone(),
                    });
                    root = *base;
                }
                PlaceKind::Index { base, index } => {
                    steps.push(HostPathNode::Index {
                        node: root,
                        argument: *index,
                    });
                    root = *base;
                }
                _ => break,
            }
        }
        if steps.is_empty() {
            return None;
        }
        steps.reverse();
        let mut root_ty = self.resolve_readable_place_type(root, env)?;
        let mut prefix = 0;
        while !matches!(root_ty, TypeId::Host(_)) && prefix + 1 < steps.len() {
            root = steps[prefix].id();
            root_ty = self.resolve_readable_place_type(root, env)?;
            prefix += 1;
        }
        steps.drain(..prefix);
        let TypeId::Host(owner) = root_ty else {
            return None;
        };
        let source = steps
            .iter()
            .map(|step| match step {
                HostPathNode::Member { name, .. } => {
                    crate::host::HostSourcePathStep::Member(name.clone())
                }
                HostPathNode::Index { argument, .. } => {
                    crate::host::HostSourcePathStep::Index(self.infer_expr_type(*argument, env))
                }
            })
            .collect::<Vec<_>>();
        match self.names.hosts.source_path(&owner, &source) {
            Ok((declaration, contract)) => {
                let mut dynamic_arguments = Vec::new();
                for ((step, declared), result) in steps
                    .iter()
                    .zip(&declaration.segments)
                    .zip(&contract.segments)
                {
                    if let (HostPathNode::Member { node, .. }, Segment::Field(id)) =
                        (step, declared)
                    {
                        self.type_table.insert_place_field(*node, id.clone());
                    }
                    if let (HostPathNode::Index { argument, .. }, Segment::Index(index)) =
                        (step, declared)
                    {
                        dynamic_arguments.push((index.slot, *argument));
                    }
                    self.type_table
                        .insert_place(step.id(), crate::host::signature_type(&result.result));
                }
                let result = crate::host::signature_type(&contract.result);
                if declaration.access != PathAccess::ReadWrite {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidHostPath {
                            reason: "host path is read-only".into(),
                        })
                        .with_span(self.lowered.source_map.place_span(place_id)),
                    );
                } else {
                    self.type_table.insert_host_place_path(
                        place_id,
                        super::ResolvedHostPlacePath {
                            root,
                            dynamic_arguments,
                            declaration,
                            contract,
                        },
                    );
                }
                Some(result)
            }
            Err(reason) => {
                let mut current = TypeId::Host(owner);
                let mut recovered_all_members = true;
                for step in &steps {
                    match step {
                        HostPathNode::Member { node, name } => {
                            let Some(field) = self.resolve_host_declared_field(&current, name)
                            else {
                                recovered_all_members = false;
                                break;
                            };
                            self.type_table.insert_place_field(*node, field.id.clone());
                            current = crate::host::signature_type(&field.ty);
                        }
                        HostPathNode::Index { .. } => {
                            recovered_all_members = false;
                            if let TypeId::Array(element) = current {
                                current = *element;
                            } else {
                                break;
                            }
                        }
                    }
                    self.type_table.insert_place(step.id(), current.clone());
                }
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidHostPath {
                        reason: reason.into(),
                    })
                    .with_span(self.lowered.source_map.place_span(place_id)),
                );
                Some(if recovered_all_members {
                    current
                } else {
                    TypeId::Error
                })
            }
        }
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
            .find(|field| {
                field.name == field_name
                    && field
                        .visibility
                        .allows(&field.owner.module, self.lowered.source.module_identity())
            })?
            .clone();
        field.ty = self
            .aggregates
            .normalize_type(&field.ty.instantiate(&substitution));
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

    fn resolve_enum_id(&self, path: &str) -> Option<kagari_common::identity::DefinitionId> {
        if let Some(binding) = self.declarations.names.lookup(path) {
            match binding.target()? {
                target @ ResolvedName::Enum(_) => {
                    return self.declarations.definition(target).cloned();
                }
                ResolvedName::SourceImport(_) => {}
                _ => return None,
            }
        }
        let TypeId::Enum(id) = &self.declarations.imported_types().get(path)?.ty else {
            return None;
        };
        Some(id.declaration.clone())
    }

    fn select_operator(
        &self,
        ty: &TypeId,
        requested: crate::types::NominalType,
        env: &BodyTypeEnv,
    ) -> Option<(crate::types::NominalType, TypeId)> {
        let interface = if crate::builtin::traits::intrinsic_applies(
            &requested,
            ty,
            Some(self.aggregates),
            &env.generic_bounds,
        ) {
            let mut interface = requested;
            if let Some(kind) =
                crate::builtin::traits::StandardTrait::from_id(&interface.declaration)
                && kind.iteration()
                && let Some(outputs) = crate::builtin::traits::iteration_outputs(
                    kind,
                    ty,
                    Some(self.aggregates),
                    &env.generic_bounds,
                )
            {
                interface.associated_types.extend(outputs);
            }
            if let Some(output) = crate::builtin::traits::intrinsic_output(&interface, ty) {
                interface.associated_types.insert(
                    crate::types::associated_type_id(&interface.declaration, "Output"),
                    output,
                );
            }
            interface
        } else {
            self.trait_bounds_for(ty, env).into_iter().find(|bound| {
                bound.declaration == requested.declaration && bound.arguments == requested.arguments
            })?
        };
        let contract = self.aggregates.trait_(&interface.declaration)?;
        let method = contract.methods.first()?;
        let mut substitution: crate::types::TypeSubstitution = contract
            .generic_params
            .iter()
            .cloned()
            .zip(interface.arguments.iter().cloned())
            .collect();
        substitution.insert_receiver(contract.id.clone(), ty.clone());
        let result = self.aggregates.normalize_type(
            &method
                .return_type
                .with_self(&contract.id, ty)
                .instantiate(&substitution)
                .with_associated_types(&interface),
        );
        Some((interface, result))
    }

    /// Operator syntax and explicit method calls retain the same trait identity.
    fn record_operator(
        &mut self,
        site: ExprId,
        receiver: ExprId,
        ty: &TypeId,
        requested: crate::types::NominalType,
        env: &BodyTypeEnv,
    ) -> Option<TypeId> {
        let (interface, result) = self.select_operator(ty, requested, env)?;
        let method = self
            .aggregates
            .trait_(&interface.declaration)?
            .methods
            .first()?
            .id
            .clone();
        self.type_table.insert_call(
            site,
            CallTarget::TraitMethod { method, interface },
            Some(receiver),
        );
        Some(result)
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
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
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
            BinaryOp::IdentityEq | BinaryOp::IdentityNotEq => {
                if lhs_ty.conflicts_with(&rhs_ty)
                    || [&lhs_ty, &rhs_ty].into_iter().any(|ty| {
                        !matches!(
                            ty,
                            TypeId::Unknown
                                | TypeId::Error
                                | TypeId::Struct(_)
                                | TypeId::Array(_)
                                | TypeId::Map { .. }
                                | TypeId::Set(_)
                        )
                    })
                {
                    self.emit_binary_operand_type_mismatch(
                        op,
                        "matching identity-bearing object",
                        &lhs_ty,
                        &rhs_ty,
                        rhs_expr,
                    );
                }
                TypeId::Builtin(BuiltinType::Bool)
            }
            BinaryOp::Eq | BinaryOp::NotEq => {
                if lhs_ty.conflicts_with(&rhs_ty)
                    || [&lhs_ty, &rhs_ty].into_iter().any(|ty| {
                        !matches!(ty, TypeId::Unknown | TypeId::Error)
                            && !crate::builtin::traits::intrinsic_holds(
                                crate::builtin::traits::StandardTrait::PartialEq,
                                ty,
                                Some(self.aggregates),
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
                let supports_ordering = |ty: &TypeId| {
                    matches!(ty, TypeId::Unknown | TypeId::Error)
                        || self
                            .select_operator(
                                ty,
                                crate::builtin::traits::StandardTrait::PartialOrd.nominal(),
                                env,
                            )
                            .is_some()
                        || super::constraints::type_satisfies_standard_constraint(
                            ty,
                            StandardTypeConstraint::OrderedNumber,
                            &env.generic_bounds,
                        )
                };
                if lhs_ty.conflicts_with(&rhs_ty)
                    || !supports_ordering(&lhs_ty)
                    || !supports_ordering(&rhs_ty)
                {
                    self.emit_binary_operand_type_mismatch(
                        op,
                        "matching PartialOrd",
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
            BinaryOp::Rem => "%",
            BinaryOp::Eq => "==",
            BinaryOp::NotEq => "!=",
            BinaryOp::IdentityEq => "===",
            BinaryOp::IdentityNotEq => "!==",
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
        let completes = super::completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            expr_id,
            self.cancel,
        )?;
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

        let mut substitution = crate::types::TypeSubstitution::default();
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
            let parameter = struct_def.fields.iter().find(|member| {
                member.name == field.name
                    && member
                        .visibility
                        .allows(&member.owner.module, self.lowered.source.module_identity())
            });
            let expected = parameter.map(|member| {
                member
                    .ty
                    .argument_context(&substitution, &struct_def.generic_params)
            });
            let actual = self.infer_expr_with_coercion(field.value, env, expected.as_ref());
            let Ok(field_completes) = super::completion::expr_can_complete(
                &self.lowered.module,
                self.names,
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
                        .find(|field| {
                            field.name == init.name
                                && field.visibility.allows(
                                    &field.owner.module,
                                    self.lowered.source.module_identity(),
                                )
                        })
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

            let Some(expected) = self.aggregates.field(field).map(|field| {
                self.aggregates
                    .normalize_type(&field.ty.instantiate(&substitution))
            }) else {
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
            associated_types: Default::default(),
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
        // An invalid index does not erase an array's known element contract.
        // Keep the diagnostic above, but let downstream member queries recover.
        // A tuple still needs a valid constant index to select a member.
        result.or_else(|| match receiver {
            TypeId::Array(element) => Some((**element).clone()),
            _ => None,
        })
    }

    fn resolve_index_type(&self, index_expr: ExprId, receiver: &TypeId) -> Option<TypeId> {
        if !super::completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            index_expr,
            self.cancel,
        )
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
        let Ok(completes) = super::completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            *arg_expr,
            self.cancel,
        ) else {
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

    fn check_standard_constraint(
        &mut self,
        ty: &TypeId,
        constraint: StandardTypeConstraint,
        env: &BodyTypeEnv,
        span_expr: ExprId,
    ) {
        if matches!(
            constraint,
            StandardTypeConstraint::HashKey | StandardTypeConstraint::Comparable
        ) {
            use crate::builtin::traits::{StandardTrait, intrinsic_holds};
            let protocols: &[StandardTrait] = if constraint == StandardTypeConstraint::HashKey {
                &[StandardTrait::Eq, StandardTrait::Hash]
            } else {
                &[StandardTrait::PartialEq]
            };
            if !ty.is_unresolved()
                && protocols
                    .iter()
                    .any(|p| !intrinsic_holds(*p, ty, Some(self.aggregates), &env.generic_bounds))
            {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::StandardConstraintNotSatisfied {
                        type_name: ty.display_name(),
                        constraint: surface::standard_constraint_name(constraint).into(),
                        reason: super::constraints::standard_constraint_reason(constraint).into(),
                    })
                    .with_span(self.lowered.source_map.expr_span(span_expr)),
                );
            }
        } else {
            super::check::validate_standard_constraint_type(
                ty,
                constraint,
                &env.generic_bounds,
                self.lowered.source_map.expr_span(span_expr),
                self.diagnostics,
            );
        }
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
            let ty = self.infer_expr_with_coercion(*argument, env, expected.as_ref());
            let Ok(completes) = super::completion::expr_can_complete(
                &self.lowered.module,
                self.names,
                *argument,
                self.cancel,
            ) else {
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

fn standard_intrinsic_name(intrinsic: StandardIntrinsic) -> &'static str {
    use StandardIntrinsic::*;

    if let Some(spec) = surface::standard_function_by_intrinsic(intrinsic) {
        return spec.api.qualified_name;
    }
    match intrinsic {
        ValueEq => "std::cmp::PartialEq::eq",
        ValueHash => "std::hash::Hash::hash",
        ValueDebug => "std::fmt::Debug::debug",
        ValueDisplay => "std::fmt::Display::display",
        ValuePartialCmp | ValueCmp | KeyLookupBegin | KeyCandidates | KeyMapGet | KeyMapInsert
        | KeyMapRemove | KeySetContains | KeySetInsert | KeySetRemove => "internal key operation",
        _ => unreachable!("public intrinsic has a declaration"),
    }
}
