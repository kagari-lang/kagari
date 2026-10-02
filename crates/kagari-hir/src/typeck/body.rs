use crate::{
    aggregates::AggregateCatalog,
    declarations::Declarations,
    hir::{
        expr::{Condition, ExprKind, literal::LiteralKind},
        ids::{BlockId, ConstId, ExprId},
    },
    imports::functions::ImportedFunctions,
    language::semantics::ProtocolSemantics,
    lower::LoweredModule,
    resolver::resolved::{ResolvedName, ResolvedNames},
    typeck::{
        BodyTypeEnv, FunctionTypeIndex, TopLevelTypeIndex, TypeIndexes, applications, completion,
        scalar::ScalarValue,
        solver::Solver,
        table::TypeTable,
        ty::{TypeContext, display_type_id, resolve_type_in},
    },
    types::{self, TypeId},
};
mod calls;
mod constructors;
mod host_access;
mod methods;
mod patterns;
mod places;
mod statements;

use kagari_abi::{language::Protocol, scalar::BuiltinType};
use kagari_common::{
    cancellation::CancellationToken,
    collection::CollectionAccess,
    diagnostic::{Diagnostic, DiagnosticKind},
    range::RangeKind,
};
use std::{
    collections::{HashMap, HashSet},
    mem,
};
mod iteration;
mod numeric;
mod operators;
mod solving;
mod standard;

use smallvec::SmallVec;

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
    aggregates: &'a AggregateCatalog,
    imported_functions: &'a ImportedFunctions,
    declarations: &'a Declarations,
    cancel: &'a CancellationToken,
    lowered: &'a LoweredModule,
    names: &'a ResolvedNames,
    function_index: &'a FunctionTypeIndex,
    top_level_index: &'a TopLevelTypeIndex,
    const_values: Option<&'a HashMap<ConstId, ScalarValue>>,
    diagnostics: &'a mut SmallVec<[Diagnostic; 4]>,
    type_table: &'a mut TypeTable,
    function_name: &'a str,
    expected_return: TypeId,
    loop_depth: usize,
    loop_results: Vec<LoopResult>,
    inference_depth: usize,
    closure_returns: Vec<Vec<TypeId>>,
    solver: Solver,
    solving: bool,
    body_inference: bool,
    explicit_arguments: HashMap<ExprId, Vec<TypeId>>,
    used_explicit_arguments: HashSet<ExprId>,
    propagation_defaults: Vec<(TypeId, TypeId)>,
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
            solver: Default::default(),
            solving: false,
            body_inference: false,
            explicit_arguments: Default::default(),
            used_explicit_arguments: Default::default(),
            propagation_defaults: Vec::new(),
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

        let result = block
            .tail_expr
            .map_or(TypeId::Builtin(BuiltinType::Unit), |expr| {
                self.infer_expr_with_coercion(expr, env, expected)
            });
        if completion::block_can_complete(
            &self.lowered.module,
            self.names,
            self.type_table,
            block_id,
            self.cancel,
        )
        .unwrap_or(true)
        {
            result
        } else {
            TypeId::Builtin(BuiltinType::Never)
        }
    }

    pub(crate) fn infer_expr_type(&mut self, expr_id: ExprId, env: &mut BodyTypeEnv) -> TypeId {
        self.infer_expr_type_expected(expr_id, env, None)
    }

    pub(super) fn infer_expr_type_expected(
        &mut self,
        expr_id: ExprId,
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> TypeId {
        const MAX_INFERENCE_DEPTH: usize = 32;
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
        let expected = self.expression_context(expr_id, expected);
        self.prepare_call_type_arguments(expr_id, env);
        let result = self.infer_expr_type_expected_inner(expr_id, env, expected.as_ref());
        self.type_table.insert_expr(expr_id, result.clone());
        let result = if completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            self.type_table,
            expr_id,
            self.cancel,
        )
        .unwrap_or(true)
        {
            result
        } else {
            TypeId::Builtin(BuiltinType::Never)
        };
        let result = self.constrain_expression(expr_id, result, expected.as_ref());
        self.check_call_type_arguments_used(expr_id);
        env.exprs.insert(expr_id, result.clone());
        self.type_table.insert_expr(expr_id, result.clone());
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
        let expr = self.lowered.module.expr(expr_id);
        if let Some(ty) = self.infer_associated_const(expr_id, env) {
            env.exprs.insert(expr_id, ty.clone());
            self.type_table.insert_expr(expr_id, ty.clone());
            return ty;
        }
        let ty = match &expr.kind {
            ExprKind::Missing => {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::ExpectedExpression)
                        .with_span(self.lowered.source_map.expr_span(expr_id)),
                );
                TypeId::Unknown
            }
            ExprKind::Name { explicit_type, .. }
                if explicit_type.is_some() || self.enum_member(expr_id).is_some() =>
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
                    | ResolvedName::HostFunction(_)
                    | ResolvedName::OpaqueType(_)
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
            ExprKind::Literal(literal)
                if matches!(literal.kind, LiteralKind::Number | LiteralKind::Float) =>
            {
                self.infer_numeric_literal(expr_id, literal, expected, false)
            }
            ExprKind::Literal(literal) => match ScalarValue::parse(literal) {
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
                    Protocol::Debug
                } else {
                    Protocol::Display
                };
                let completes = completion::expr_can_complete(
                    &self.lowered.module,
                    self.names,
                    self.type_table,
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
            ExprKind::Cast { expr, target } => {
                let input = self.infer_expr_type(*expr, env);
                let context = TypeContext {
                    declarations: self.declarations,
                    generics: &env.generics,
                    self_type: None,
                    implementation: None,
                };
                let output = resolve_type_in(
                    &self.lowered.module,
                    *target,
                    context,
                    self.type_table,
                    self.cancel,
                );
                let completes = completion::expr_can_complete(
                    &self.lowered.module,
                    self.names,
                    self.type_table,
                    *expr,
                    self.cancel,
                )
                .unwrap_or(false);
                if completes
                    && !matches!(
                        input,
                        TypeId::Inference(_) | TypeId::Unknown | TypeId::Error
                    )
                    && !matches!((&input, &output), (TypeId::Builtin(a), TypeId::Builtin(b)) if a.can_cast_to(*b))
                {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidNumericCast {
                            from: display_type_id(&input),
                            to: display_type_id(&output),
                        })
                        .with_span(self.lowered.source_map.expr_span(expr_id)),
                    );
                }
                output
            }
            ExprKind::Prefix { op, expr } => {
                self.infer_prefix_operator(expr_id, op, expr, env, expected)
            }
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => {
                let kind = RangeKind::from_parts(start.is_some(), end.is_some(), *inclusive)
                    .unwrap_or(RangeKind::Full);
                let mut element = match expected {
                    Some(TypeId::Range(item, _)) => Some((**item).clone()),
                    _ => None,
                };
                for operand in start.iter().chain(end) {
                    let actual = self.infer_expr_with_coercion(*operand, env, element.as_ref());
                    if let Some(expected) = &mut element {
                        if expected.conflicts_with(&actual) {
                            self.diagnostics.push(
                                Diagnostic::error(DiagnosticKind::UnaryOperandTypeMismatch {
                                    operator: "range",
                                    expected: display_type_id(expected),
                                    found: display_type_id(&actual),
                                })
                                .with_span(self.lowered.source_map.expr_span(*operand)),
                            );
                        }
                        expected.recover_from(&actual);
                    } else {
                        element = Some(actual);
                    }
                }
                TypeId::Range(
                    Box::new(element.unwrap_or(TypeId::Builtin(BuiltinType::Unit))),
                    kind,
                )
            }
            ExprKind::Binary { lhs, op, rhs } => {
                self.infer_binary_operator(expr_id, lhs, op, rhs, env, expected)
            }
            ExprKind::Call { callee, args, .. } => {
                if let Some(ty) = self.infer_enum_constructor(expr_id, *callee, args, env, expected)
                {
                    ty
                } else if let Some(ty) =
                    self.infer_host_call_type(expr_id, *callee, args, env, expected)
                {
                    ty
                } else if let Some(ty) =
                    self.infer_host_method_call(expr_id, *callee, args, env, expected)
                {
                    ty
                } else if let Some(method_ty) =
                    self.infer_inherent_method_call_type(expr_id, *callee, args, env, expected)
                {
                    method_ty
                } else if let Some(helper_ty) =
                    self.infer_runtime_helper_call_type(expr_id, *callee, args, env)
                {
                    helper_ty
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
                        self.prepare_annotation_holes(annotation);
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
                    } else if self.body_inference {
                        self.inference_variable(expr_id, 128 + index)
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
                let old_return = mem::replace(
                    &mut self.expected_return,
                    expected_result.cloned().unwrap_or(TypeId::Unknown),
                );
                let old_loop_depth = mem::replace(&mut self.loop_depth, 0);
                let old_loop_results = mem::take(&mut self.loop_results);
                let old_name = mem::replace(&mut self.function_name, "closure");
                self.closure_returns.push(Vec::new());
                let body_result =
                    self.infer_expr_with_coercion(*body, &mut closure_env, expected_result);
                let returns = self.closure_returns.pop().expect("closure return context");
                let completes = completion::expr_can_complete(
                    &self.lowered.module,
                    self.names,
                    self.type_table,
                    *body,
                    self.cancel,
                )
                .unwrap_or(false);
                let mut result = if completes {
                    body_result
                } else {
                    expected_result
                        .filter(|ty| !matches!(ty, TypeId::Unknown))
                        .cloned()
                        .or_else(|| returns.first().cloned())
                        .unwrap_or_else(|| self.inference_variable(expr_id, 2))
                };
                if !completes && returns.is_empty() {
                    self.solver.defer_never(&result);
                    if matches!(result, TypeId::Unknown) {
                        result = TypeId::Builtin(BuiltinType::Never);
                    }
                }
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
                let Ok(completes) = completion::expr_can_complete(
                    &self.lowered.module,
                    self.names,
                    self.type_table,
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
                    Condition::Expr(expr) => {
                        let Ok(completes) = self.check_condition_type(*expr, "if", env) else {
                            return TypeId::Unknown;
                        };
                        completes
                    }
                    Condition::Binding {
                        pattern,
                        initializer,
                    } => {
                        let ty = self.infer_expr_type(*initializer, env);
                        self.check_pattern(*pattern, &ty, &mut then_env);
                        let Ok(completes) = completion::expr_can_complete(
                            &self.lowered.module,
                            self.names,
                            self.type_table,
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
                        let Ok(then_completes) = completion::block_can_complete(
                            &self.lowered.module,
                            self.names,
                            self.type_table,
                            *then_branch,
                            self.cancel,
                        ) else {
                            return TypeId::Unknown;
                        };
                        let mut else_context = expected.cloned();
                        if condition_completes && then_completes {
                            if let Some(context) = &mut else_context {
                                context.recover_from(&then_ty);
                            } else {
                                else_context = Some(then_ty.clone());
                            }
                        }
                        let else_ty =
                            self.infer_expr_with_coercion(*else_expr, env, else_context.as_ref());
                        let Ok(else_completes) = completion::expr_can_complete(
                            &self.lowered.module,
                            self.names,
                            self.type_table,
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
                            } else if else_completes
                                && then_ty != else_ty
                                && then_ty.same_collection_family(&else_ty)
                            {
                                let view = then_ty.collection_view().expect("collection join");
                                if let Some(tail) =
                                    self.lowered.module.block(*then_branch).tail_expr
                                {
                                    self.apply_interface_coercion(
                                        tail,
                                        then_ty,
                                        Some(&view),
                                        &then_env,
                                    );
                                }
                                self.apply_interface_coercion(
                                    *else_expr,
                                    else_ty,
                                    Some(&view),
                                    env,
                                );
                                then_ty = view;
                            } else if else_completes
                                && (then_ty.can_weaken_to(&else_ty)
                                    || else_ty.can_weaken_to(&then_ty))
                            {
                                then_ty = then_ty.read_only_view().expect("collection branch join");
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
                                if !then_ty.is_unresolved()
                                    && let Some(tail) =
                                        self.lowered.module.block(*then_branch).tail_expr
                                {
                                    self.refine_standard_tail(tail, &then_ty, &mut then_env);
                                }
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
                let Ok(scrutinee_completes) = completion::expr_can_complete(
                    &self.lowered.module,
                    self.names,
                    self.type_table,
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
                    let Ok(completes) = completion::expr_can_complete(
                        &self.lowered.module,
                        self.names,
                        self.type_table,
                        arm.expr,
                        self.cancel,
                    ) else {
                        return TypeId::Unknown;
                    };
                    if !completes {
                        continue;
                    }
                    if let Some(result) = &mut result {
                        if found != *result && found.same_collection_family(result) {
                            let view = result.collection_view().expect("collection match join");
                            for previous in arms {
                                if let Some(ty) = self.type_table.expr_type(previous.expr) {
                                    self.apply_interface_coercion(
                                        previous.expr,
                                        ty,
                                        Some(&view),
                                        env,
                                    );
                                }
                                if previous.expr == arm.expr {
                                    break;
                                }
                            }
                            *result = view;
                        } else if found.can_weaken_to(result) || result.can_weaken_to(&found) {
                            *result = result.read_only_view().expect("collection arm join");
                        } else if found.conflicts_with(result) {
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
            ExprKind::ArrayRepeat { value, count } => {
                let member = match expected {
                    Some(TypeId::Array(element, _)) => Some(element.as_ref()),
                    Some(TypeId::Trait(interface))
                        if matches!(
                            Protocol::from_id(&interface.declaration),
                            Some(Protocol::List | Protocol::MutableList)
                        ) =>
                    {
                        interface.arguments.first()
                    }
                    _ => None,
                };
                let element = self.infer_expr_with_coercion(*value, env, member);
                if !self.solving
                    && !types::supports_array_repetition(&element, |instance| {
                        if self.cancel.check().is_err() {
                            return None;
                        }
                        let contract = self.aggregates.enumeration(&instance.declaration)?;
                        let substitution = contract
                            .generic_params
                            .iter()
                            .cloned()
                            .zip(instance.arguments.iter().cloned())
                            .collect();
                        Some(
                            contract
                                .variants
                                .iter()
                                .flat_map(|v| &v.payload)
                                .map(|ty| ty.instantiate(&substitution))
                                .collect(),
                        )
                    })
                {
                    self.diagnostics.push(Diagnostic::error(DiagnosticKind::StandardConstraintNotSatisfied {
                        type_name: element.display_name(),
                        constraint: "array repetition without shared mutable objects".into(),
                        reason: "use ArrayList::from_fn(count, |index| value) to initialize each element".into(),
                    }).with_span(self.lowered.source_map.expr_span(*value)));
                }
                let length_type = TypeId::Builtin(BuiltinType::USize);
                let actual = self.infer_expr_with_coercion(*count, env, Some(&length_type));
                if actual.conflicts_with(&length_type) {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::UnaryOperandTypeMismatch {
                            operator: "array repeat count",
                            expected: display_type_id(&length_type),
                            found: display_type_id(&actual),
                        })
                        .with_span(self.lowered.source_map.expr_span(*count)),
                    );
                }
                TypeId::Array(Box::new(element), CollectionAccess::Mutable)
            }
            ExprKind::Array(elements) => {
                let member = match expected {
                    Some(TypeId::Array(element, _)) => Some(element.as_ref()),
                    Some(TypeId::Trait(interface))
                        if matches!(
                            Protocol::from_id(&interface.declaration),
                            Some(Protocol::List | Protocol::MutableList)
                        ) =>
                    {
                        interface.arguments.first()
                    }
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
                    let Ok(completes) = completion::expr_can_complete(
                        &self.lowered.module,
                        self.names,
                        self.type_table,
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
                TypeId::Array(
                    Box::new(element_ty.unwrap_or_else(|| {
                        member
                            .cloned()
                            .unwrap_or_else(|| self.inference_variable(expr_id, 1))
                    })),
                    CollectionAccess::Mutable,
                )
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
                    Some(LoopResult::Expression(value)) => {
                        value.found.unwrap_or(TypeId::Builtin(BuiltinType::Never))
                    }
                    _ => TypeId::Unknown,
                }
            }
            ExprKind::Block(block) => self.infer_block_types_expected(*block, env, expected),
        };

        applications::validate(
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
}
