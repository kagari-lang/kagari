//! Body traversal and inference orchestration; specialized operations live in child modules.

use crate::{
    aggregates::AggregateCatalog,
    builtin::array_bridge,
    declarations::Declarations,
    hir::{
        expr::{ExprKind, literal::LiteralKind},
        ids::{BlockId, ConstId, ExprId},
    },
    imports::functions::ImportedFunctions,
    lower::LoweredModule,
    resolver::resolved::{ResolvedName, ResolvedNames},
    typeck::{
        BodyTypeEnv, FunctionTypeIndex, TopLevelTypeIndex, TypeIndexes, applications, completion,
        scalar::ScalarValue,
        solver::Solver,
        table::TypeTable,
        ty::{TypeContext, display_type_id, resolve_type_in},
    },
    types,
    types::TypeId,
};
use kagari_common::cancellation::CancellationToken;
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::{language::Protocol, range::RangeKind, scalar::BuiltinType};
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet};

mod asynchronous;
mod calls;
mod closures;
mod constructors;
mod host_access;
mod methods;
mod patterns;
mod places;
mod statements;
mod trait_calls;

mod iteration;
mod joins;
mod local_functions;
mod numeric;
mod operators;
mod solving;
mod standard;

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

/// Per-function/constant semantic traversal state borrowing prepared catalogs.
///
/// The checker reads lowered nodes and resolved names, updates the body environment,
/// and writes diagnostics plus `TypeTable` facts. `Solver` holds temporary inference
/// variables; solving/recheck state must be sealed before publishing the table.
/// Loop and closure stacks keep expected/result types separate from outer returns.
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
    /// Mutable semantic output; node keys belong to `lowered`.
    type_table: &'a mut TypeTable,
    function_name: &'a str,
    /// Result contract of the currently checked callable context.
    expected_return: TypeId,
    async_body: bool,
    loop_depth: usize,
    /// Nested loop result expectations and observed `break` value types.
    loop_results: Vec<LoopResult>,
    /// Expression-inference recursion depth, limited before nested inference proceeds.
    inference_depth: usize,
    /// Nested closure return observations, separate from the enclosing function.
    closure_returns: Vec<Vec<TypeId>>,
    /// Body-local variables and constraints, never a reusable signature object.
    solver: Solver,
    /// Whether the current traversal is accumulating/solving body constraints.
    solving: bool,
    body_inference: bool,
    /// Call-site type arguments retained across inference/recheck passes.
    explicit_arguments: HashMap<ExprId, Vec<TypeId>>,
    used_explicit_arguments: HashSet<ExprId>,
    /// Deferred propagation output/residual defaults awaiting inference resolution.
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
            async_body: false,
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

    pub(crate) fn set_async(&mut self, is_async: bool) {
        self.async_body = is_async;
    }

    pub(crate) fn infer_block_types(&mut self, block_id: BlockId, env: &mut BodyTypeEnv) -> TypeId {
        self.infer_block_types_expected(block_id, env, None)
    }

    /// Checks statements and the tail under an optional expected type, then accounts for whether the block can complete.
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

    /// Applies source context and the depth bound, computes an expression type, and records its semantic fact.
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
        if !result.reference_position_valid() {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidReference {
                    reason: "views require a nominal target and cannot be nested in owning values",
                })
                .with_span(self.lowered.source_map.expr_span(expr_id)),
            );
        }
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
        if let Some(ty) = self.infer_local_function_value(expr_id, env, expected) {
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
                    ResolvedName::Static(id) => self.top_level_index.statics.get(&id).cloned(),
                    ResolvedName::Source(source) => {
                        if let Some(item) = self.top_level_index.imported_constants.get(&source) {
                            self.type_table.insert_scalar(expr_id, item.value.clone());
                            Some(item.value.ty())
                        } else {
                            self.top_level_index
                                .imported_statics
                                .get(&source)
                                .map(|item| {
                                    self.aggregates
                                        .normalize_type(self.declarations.definitions(), &item.ty)
                                })
                        }
                    }
                    ResolvedName::Function(_)
                    | ResolvedName::RuntimeHelper(_)
                    | ResolvedName::Namespace(_)
                    | ResolvedName::Module(_)
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
                        .aggregates
                        .language_trait(protocol)
                        .and_then(|requested| {
                            self.record_operator(expr_id, *expr, &ty, requested, env)
                        })
                        .is_none()
                    && !ty.is_unresolved()
                {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidCallTarget {
                            type_name: format!(
                                "interpolation requires {} for {}",
                                protocol.name(),
                                display_type_id(self.declarations.definitions(), &ty)
                            ),
                        })
                        .with_span(self.lowered.source_map.expr_span(*expr)),
                    );
                }
                TypeId::Builtin(BuiltinType::String)
            }
            ExprKind::Await { expr } => self.infer_await(expr_id, *expr, env, expected),
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
                            from: display_type_id(self.declarations.definitions(), &input),
                            to: display_type_id(self.declarations.definitions(), &output),
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
                                    expected: display_type_id(
                                        self.declarations.definitions(),
                                        expected,
                                    ),
                                    found: display_type_id(
                                        self.declarations.definitions(),
                                        &actual,
                                    ),
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
                if matches!(
                    self.lowered.module.expr(*callee).kind,
                    ExprKind::Name { .. }
                ) && let Some(ty) =
                    self.infer_trait_method_call_type(expr_id, *callee, args, env, expected)
                {
                    ty
                } else if let Some(ty) =
                    self.infer_enum_constructor(expr_id, *callee, args, env, expected)
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
            ExprKind::Closure { .. } | ExprKind::AsyncBlock { .. } => {
                self.infer_closure(expr_id, env, expected)
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
            } => self.infer_if_type(condition, *then_branch, *else_branch, env, expected),
            ExprKind::Match { scrutinee, arms } => {
                self.infer_match_type(*scrutinee, arms, env, expected)
            }
            ExprKind::StructInit {
                path,
                fields,
                base,
                explicit_type,
            } => {
                let explicit = explicit_type.map(|ty| self.resolve_constructor_type(ty, env));
                self.infer_struct_init_type(
                    path,
                    fields,
                    *base,
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
                let member = expected.and_then(|ty| array_bridge::element_context(ty));
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
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::StandardConstraintNotSatisfied {
                            type_name: element.display_name(self.declarations.definitions()),
                            constraint: "array repetition without shared mutable objects".into(),
                            reason: "initialize each element separately to create distinct objects"
                                .into(),
                        })
                        .with_span(self.lowered.source_map.expr_span(*value)),
                    );
                }
                let length_type = TypeId::Builtin(BuiltinType::U64);
                let actual = self.infer_expr_with_coercion(*count, env, Some(&length_type));
                if actual.conflicts_with(&length_type) {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::UnaryOperandTypeMismatch {
                            operator: "array repeat count",
                            expected: display_type_id(
                                self.declarations.definitions(),
                                &length_type,
                            ),
                            found: display_type_id(self.declarations.definitions(), &actual),
                        })
                        .with_span(self.lowered.source_map.expr_span(*count)),
                    );
                }
                TypeId::Array(Box::new(element))
            }
            ExprKind::Array {
                elements,
                element_type,
            } => {
                let intrinsic = element_type.map(TypeId::Builtin);
                let member = intrinsic
                    .as_ref()
                    .or_else(|| expected.and_then(|ty| array_bridge::element_context(ty)));
                let mut element_ty = intrinsic.clone();
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
                                    expected: display_type_id(
                                        self.declarations.definitions(),
                                        element_ty,
                                    ),
                                    found: display_type_id(self.declarations.definitions(), &ty),
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
                        .filter(|ty| !matches!(ty, TypeId::Unknown))
                        .cloned()
                        .unwrap_or_else(|| self.inference_variable(expr_id, 1))
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
                    Some(LoopResult::Expression(value)) => {
                        value.found.unwrap_or(TypeId::Builtin(BuiltinType::Never))
                    }
                    _ => TypeId::Unknown,
                }
            }
            ExprKind::Block(block) => self.infer_block_types_expected(*block, env, expected),
        };

        applications::validate(
            applications::ApplicationContext {
                definitions: self.declarations.definitions(),
                catalog: self.aggregates,
                receiver: None,
            },
            &ty,
            &env.generic_bounds,
            self.lowered.source_map.expr_span(expr_id),
            self.diagnostics,
            self.cancel,
        );
        env.exprs.insert(expr_id, ty.clone());
        self.type_table.insert_expr(expr_id, ty.clone());
        ty
    }
}
