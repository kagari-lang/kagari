use crate::source::lower::{
    MirLoweringError,
    instances::CallableInstance,
    state::{FunctionLowerer, LoopScope},
    support::lower_scalar,
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{
    numeric::NumericConversion, operations::UnaryOp, representation::semantic_representation,
    standard::RuntimePrimitive,
};
use kagari_hir::{
    aggregates::implementations::{ImplementationSearchContext, ImplementationSearchLimits},
    hir::{
        expr::{Condition, ExprKind, ops::BinaryOp as HirBinaryOp},
        ids::{BlockId, ExprId},
    },
    resolver::resolved::ResolvedName,
    typeck::table::ResolvedInterfaceImplementation,
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_mir::instruction::{
    CallTarget, Constant, Instruction, MirValue, Terminator, ValueBuffer,
};
use kagari_types::{collection::CollectionAccess, language::Protocol, scalar::BuiltinType, ty::Ty};
use std::ops::ControlFlow;

mod aggregates;
mod calls;
mod patterns;
mod propagation;
mod shared;

mod equality;
mod native_calls;
mod native_contracts;
mod operators;
mod standard;

impl FunctionLowerer<'_, '_> {
    fn implementation_search_context(&self) -> ImplementationSearchContext<'_> {
        ImplementationSearchContext {
            assumptions: &self.implementation_bounds,
            limits: ImplementationSearchLimits {
                max_checks: self.planner.options.max_type_nodes,
                max_depth: self.planner.options.max_type_depth,
            },
            cancel: &self.planner.options.cancel,
        }
    }

    fn find_implementation_method(
        &self,
        method: &DefinitionId,
        interface: &NominalType,
        ty: &TypeId,
    ) -> Result<Option<(DefinitionId, Vec<TypeId>)>, MirLoweringError> {
        Ok(self.planner.catalog.implementation_method(
            &self.planner.definitions(),
            method,
            interface,
            ty,
            self.implementation_search_context(),
        )?)
    }

    fn lower_closure(&mut self, expr_id: ExprId) -> Result<MirValue, MirLoweringError> {
        let mut captures = ValueBuffer::new();
        for resolved in self.analyzed.names.closure_captures(expr_id) {
            let local = self.lookup_binding(resolved.clone())?;
            let ty = match resolved {
                ResolvedName::Local(id) => self
                    .analyzed
                    .typed
                    .type_table
                    .local_type(*id)
                    .ok_or(MirLoweringError::MissingLocalType(*id))?,
                ResolvedName::Param(id) => self
                    .analyzed
                    .typed
                    .functions
                    .iter()
                    .find(|function| function.id == self.instance.function)
                    .and_then(|function| function.params.iter().find(|param| param.id == *id))
                    .map(|param| param.ty.clone())
                    .ok_or(MirLoweringError::MissingBinding("captured parameter type"))?,
                _ => return Err(MirLoweringError::MissingBinding("closure capture")),
            };
            let physical = if matches!(resolved, ResolvedName::Local(id) if self.cell_locals.contains(id))
            {
                ValueType::HeapObject
            } else {
                self.value_type(&ty)?
            };
            let value = self.alloc_temp(physical);
            self.emit(Instruction::LoadLocal { dst: value, local });
            if physical == ValueType::Generic {
                let semantic = self.semantic_type(&ty)?;
                self.function
                    .semantic
                    .registers
                    .insert(value.temp.index(), semantic);
            }
            captures.push(value);
        }
        let span = self.analyzed.lowered.source_map.expr_span(expr_id);
        let function = self
            .planner
            .enqueue_closure(&self.instance, expr_id, span)?;
        let dst = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::MakeClosure {
            dst,
            function,
            captures,
        });
        Ok(dst)
    }

    fn lower_values(
        &mut self,
        expressions: &[ExprId],
    ) -> Result<ControlFlow<MirValue, ValueBuffer>, MirLoweringError> {
        let mut values = ValueBuffer::new();
        for expression in expressions {
            let value = self.lower_expr(*expression)?;
            if self.current_block_terminated() {
                return Ok(ControlFlow::Break(value));
            }
            values.push(value);
        }
        Ok(ControlFlow::Continue(values))
    }

    fn record_expr_layout(&mut self, expr_id: ExprId) -> Result<(), MirLoweringError> {
        let ty = self
            .analyzed
            .typed
            .type_table
            .expr_type(expr_id)
            .ok_or(MirLoweringError::MissingExprType(expr_id))?;
        self.planner.record_layout_root(
            &ty,
            &self.instance.substitution,
            self.analyzed.lowered.source_map.expr_span(expr_id),
        )?;
        Ok(())
    }

    pub(crate) fn lower_expr(&mut self, expr_id: ExprId) -> Result<MirValue, MirLoweringError> {
        self.planner.check()?;
        let mut value = self.lower_expr_value(expr_id)?;
        if !self.current_block_terminated() && self.expr_type(expr_id)? == ValueType::Never {
            self.set_terminator(Terminator::Unreachable);
        }
        if !self.current_block_terminated() {
            self.record_expr_layout(expr_id)?;
            if let Some((receiver, interface)) = self
                .analyzed
                .typed
                .type_table
                .callable_coercion(expr_id)
                .cloned()
            {
                let function = self.planner.enqueue_callable(
                    &self.instance,
                    CallableInstance {
                        receiver,
                        interface,
                        span: self.analyzed.lowered.source_map.expr_span(expr_id),
                    },
                )?;
                let dst = self.alloc_temp(ValueType::HeapObject);
                self.emit(Instruction::MakeClosure {
                    dst,
                    function,
                    captures: smallvec::smallvec![value],
                });
                value = dst;
            }

            if let Some(coercion) = self
                .analyzed
                .typed
                .type_table
                .interface_coercion(expr_id)
                .cloned()
            {
                let span = self.analyzed.lowered.source_map.expr_span(expr_id);

                if matches!(
                    coercion.implementation,
                    ResolvedInterfaceImplementation::Upcast
                ) {
                    let types = self.planner.arguments(
                        &[
                            coercion.concrete_type,
                            TypeId::Trait(coercion.interface_type),
                        ],
                        &self.instance.substitution,
                        span,
                    )?;
                    let [Ty::Trait(source), Ty::Trait(target)] = [
                        self.planner.encode_type(&types[0]),
                        self.planner.encode_type(&types[1]),
                    ] else {
                        return Err(MirLoweringError::MissingBinding("interface upcast types"));
                    };
                    let dst = self.alloc_temp(ValueType::HeapObject);
                    self.emit(Instruction::UpcastInterface {
                        dst,
                        value,
                        source,
                        target,
                    });
                    return Ok(dst);
                }
                let types = self.planner.arguments(
                    &[
                        coercion.concrete_type.clone(),
                        TypeId::Trait(coercion.interface_type.clone()),
                    ],
                    &self.instance.substitution,
                    span,
                )?;
                let TypeId::Trait(interface) = &types[1] else {
                    return Err(MirLoweringError::MissingBinding("interface demand type"));
                };
                // Boxing changes representation; retain the checked input type
                // before replacing the expression's register with the interface.
                self.function
                    .semantic
                    .registers
                    .insert(value.temp.index(), self.planner.encode_type(&types[0]));
                self.planner.require_parent_interfaces(
                    &types[0],
                    interface,
                    &Default::default(),
                    span,
                )?;
                let (implementation, arguments, constructed) = match coercion.implementation {
                    ResolvedInterfaceImplementation::Upcast => unreachable!("upcast handled above"),
                    ResolvedInterfaceImplementation::Intrinsic => {
                        let mut types = self
                            .planner
                            .arguments(
                                &[
                                    coercion.concrete_type,
                                    TypeId::Trait(coercion.interface_type),
                                ],
                                &self.instance.substitution,
                                span,
                            )?
                            .into_iter();
                        let receiver = types.next().expect("receiver argument");
                        let applied = types.next().expect("interface argument");
                        let TypeId::Trait(interface) = applied else {
                            return Err(MirLoweringError::MissingBinding(
                                "intrinsic interface type",
                            ));
                        };
                        (
                            self.planner
                                .intrinsic_interface_bridge(&receiver, &interface, span)?,
                            Vec::new(),
                            interface,
                        )
                    }
                    ResolvedInterfaceImplementation::Declared {
                        declaration,
                        arguments,
                    } => {
                        let arguments = self.planner.arguments(
                            &arguments,
                            &self.instance.substitution,
                            span,
                        )?;
                        self.planner
                            .record_interface(&declaration, &arguments, span)?;
                        let contract = self
                            .planner
                            .catalog
                            .implementation_signature(&declaration)
                            .ok_or(MirLoweringError::MissingBinding(
                                "interface construction contract",
                            ))?;
                        let substitution: TypeSubstitution = contract
                            .generic_params
                            .iter()
                            .cloned()
                            .zip(arguments.iter().cloned())
                            .collect();
                        let constructed = contract.trait_type.instantiate(&substitution);
                        (
                            declaration,
                            arguments
                                .iter()
                                .map(|ty| self.planner.encode_type(ty))
                                .collect(),
                            constructed,
                        )
                    }
                };
                let dst = self.alloc_temp(ValueType::HeapObject);
                self.emit(Instruction::MakeInterface {
                    dst,
                    value,
                    implementation,
                    arguments,
                });
                value = dst;
                // The table constructs its exact outputs. The value's bounded
                // iterator-result view is a separate checked adaptation.
                let source = self.planner.encode_nominal(&constructed);
                let Ty::Trait(target) = self.planner.encode_type(&TypeId::Trait(interface.clone()))
                else {
                    unreachable!("interface value type")
                };
                if source != target {
                    self.function
                        .semantic
                        .registers
                        .insert(value.temp.index(), Ty::Trait(source.clone()));
                    let dst = self.alloc_temp(ValueType::HeapObject);
                    self.emit(Instruction::UpcastInterface {
                        dst,
                        value,
                        source,
                        target,
                    });
                    value = dst;
                }
            }
        }
        if !self.current_block_terminated() {
            let ty = self
                .analyzed
                .typed
                .type_table
                .coerced_expr_type(&self.planner.definitions(), expr_id)
                .ok_or(MirLoweringError::MissingExprType(expr_id))?;
            if ty != TypeId::Unknown && ty != TypeId::Error {
                let span = self.analyzed.lowered.source_map.expr_span(expr_id);
                let ty = self
                    .planner
                    .arguments(&[ty], &self.instance.substitution, span)?
                    .remove(0);
                let abi = self.planner.encode_type(&ty);
                // A distinct move records access weakening without changing an alias's contract.
                if ty.collection_access() == Some(CollectionAccess::ReadOnly) {
                    let dst = self.alloc_temp(value.ty);
                    self.emit(Instruction::Move { dst, src: value });
                    value = dst;
                }
                self.function
                    .semantic
                    .registers
                    .insert(value.temp.index(), abi);
            }
        }
        Ok(value)
    }

    fn lower_expr_value(&mut self, expr_id: ExprId) -> Result<MirValue, MirLoweringError> {
        if let Some(fact) = self
            .analyzed
            .typed
            .type_table
            .associated_const(expr_id)
            .cloned()
        {
            let span = self.analyzed.lowered.source_map.expr_span(expr_id);
            let types = self.planner.arguments(
                &[fact.receiver, TypeId::Trait(fact.interface)],
                &self.instance.substitution,
                span,
            )?;
            let TypeId::Trait(interface) = &types[1] else {
                return Err(MirLoweringError::MissingBinding("constant trait"));
            };
            let (implementation, _) = self
                .planner
                .catalog
                .concrete_interface_implementation(
                    &self.planner.definitions(),
                    interface,
                    &types[0],
                    &self.implementation_bounds,
                    ImplementationSearchLimits {
                        max_checks: self.planner.options.max_type_nodes,
                        max_depth: self.planner.options.max_type_depth,
                    },
                    &self.planner.options.cancel,
                )?
                .ok_or(MirLoweringError::MissingBinding("constant implementation"))?;
            let declaration = self
                .planner
                .catalog
                .implementation_constant(&implementation, &fact.member)
                .ok_or(MirLoweringError::MissingBinding("constant definition"))?;
            let value =
                self.planner
                    .constant(declaration)
                    .ok_or(MirLoweringError::MissingBinding(
                        "checked associated constant",
                    ))?;
            return Ok(self.lower_constant(lower_scalar(value), self.expr_type(expr_id)?));
        }
        if let Some(target) = self
            .analyzed
            .typed
            .type_table
            .enum_constructor(expr_id)
            .cloned()
        {
            let variant = target
                .variant
                .as_ref()
                .and_then(|id| self.analyzed.aggregates.variant(id))
                .ok_or(MirLoweringError::MissingBinding("checked enum variant"))?;
            let enumeration = self
                .analyzed
                .aggregates
                .enumeration(&target.enumeration)
                .ok_or(MirLoweringError::MissingBinding("checked enum owner"))?;
            if variant.owner != enumeration.id {
                return Err(MirLoweringError::MissingBinding("checked variant owner"));
            }
            let arity = variant.payload.len();
            let variant = variant.slot;
            let args = match &self.analyzed.lowered.module.expr(expr_id).kind {
                ExprKind::Call { args, .. } => args.to_vec(),
                ExprKind::Name { .. } => Vec::new(),
                _ => {
                    return Err(MirLoweringError::MissingBinding(
                        "enum constructor expression",
                    ));
                }
            };
            let fields = match self.lower_values(&args)? {
                ControlFlow::Continue(fields) => fields,
                ControlFlow::Break(value) => return Ok(value),
            };
            if fields.len() != arity {
                return Err(MirLoweringError::MissingBinding(
                    "checked enum constructor arity",
                ));
            }
            let dst = self.alloc_temp(ValueType::HeapObject);
            self.emit(Instruction::MakeEnum {
                dst,
                enumeration: self.expr_nominal_instance(expr_id)?,
                variant,
                fields,
            });
            return Ok(dst);
        }
        if let Some(value) = self
            .analyzed
            .typed
            .type_table
            .scalar_value(expr_id)
            .cloned()
        {
            return Ok(self.lower_constant(lower_scalar(value), self.expr_type(expr_id)?));
        }
        let expr = self.analyzed.lowered.module.expr(expr_id).clone();
        match expr.kind {
            ExprKind::Missing => Err(MirLoweringError::UnresolvedExpr(expr_id)),
            ExprKind::Name { .. } => self.lower_name_expr(expr_id),
            ExprKind::Literal(_) => Err(MirLoweringError::MissingBinding("checked literal")),
            ExprKind::Await { expr } => self.lower_await(expr_id, expr),
            ExprKind::Propagate { expr } => self.lower_propagation(expr_id, expr),
            ExprKind::InterpolatedString(parts) => {
                let elements = match self.lower_values(&parts)? {
                    ControlFlow::Continue(values) => values,
                    ControlFlow::Break(value) => return Ok(value),
                };
                let array = self.alloc_temp(ValueType::HeapObject);
                self.emit(Instruction::MakeArray {
                    element: Ty::Builtin(BuiltinType::String),
                    dst: array,
                    elements,
                });
                let separator = self.alloc_temp(ValueType::Str);
                self.emit(Instruction::LoadConst {
                    dst: separator,
                    constant: Constant::Str(String::new()),
                });
                let dst = self.alloc_temp(ValueType::Str);
                self.emit(Instruction::Call {
                    dst: Some(dst),
                    callee: CallTarget::RuntimePrimitive(RuntimePrimitive::StringPartsJoin),
                    args: smallvec::smallvec![array, separator],
                });
                Ok(dst)
            }
            ExprKind::FormatPart { expr, .. } => {
                if self
                    .analyzed
                    .typed
                    .type_table
                    .call_resolution(expr_id)
                    .is_none()
                {
                    return self.lower_expr(expr);
                }
                self.lower_call(expr_id, &[])
            }
            ExprKind::Cast { expr, .. } => {
                let src = self.lower_expr(expr)?;
                if self.current_block_terminated() {
                    return Ok(src);
                }
                let source = self
                    .analyzed
                    .typed
                    .type_table
                    .expr_type(expr)
                    .ok_or(MirLoweringError::MissingExprType(expr))?;
                let target = self
                    .analyzed
                    .typed
                    .type_table
                    .expr_type(expr_id)
                    .ok_or(MirLoweringError::MissingExprType(expr_id))?;
                let (TypeId::Builtin(source), TypeId::Builtin(target)) = (source, target) else {
                    return Err(MirLoweringError::MissingBinding("concrete numeric cast"));
                };
                let conversion = NumericConversion { source, target };
                let (_, result) = conversion
                    .contract()
                    .ok_or(MirLoweringError::MissingBinding("numeric cast"))?;
                let dst = self.alloc_temp(semantic_representation(&result));
                self.emit(Instruction::Convert {
                    dst,
                    src,
                    conversion,
                });
                Ok(dst)
            }
            ExprKind::Prefix { op, expr } => {
                let operand = self.lower_expr(expr)?;
                if self.current_block_terminated() {
                    return Ok(operand);
                }
                if self
                    .analyzed
                    .typed
                    .type_table
                    .call_resolution(expr_id)
                    .is_some()
                {
                    return self.lower_selected_operator(expr_id, &[operand]);
                }
                let dst = self.alloc_temp(self.expr_type(expr_id)?);
                self.emit(Instruction::Unary {
                    dst,
                    op: FunctionLowerer::lower_unary_op(op),
                    operand,
                });
                Ok(dst)
            }
            ExprKind::Binary { lhs, op, rhs } => {
                if matches!(op, HirBinaryOp::AndAnd | HirBinaryOp::OrOr) {
                    return self.lower_short_circuit(expr_id, lhs, op, rhs);
                }
                let operand_ty = self
                    .analyzed
                    .typed
                    .type_table
                    .expr_type(lhs)
                    .ok_or(MirLoweringError::MissingExprType(lhs))?;
                let lhs = self.lower_expr(lhs)?;
                if self.current_block_terminated() {
                    return Ok(lhs);
                }
                let rhs = self.lower_expr(rhs)?;
                if self.current_block_terminated() {
                    return Ok(rhs);
                }
                if matches!(
                    op,
                    HirBinaryOp::Add
                        | HirBinaryOp::Sub
                        | HirBinaryOp::Mul
                        | HirBinaryOp::Div
                        | HirBinaryOp::Rem
                        | HirBinaryOp::BitAnd
                        | HirBinaryOp::BitOr
                        | HirBinaryOp::BitXor
                        | HirBinaryOp::Shl
                        | HirBinaryOp::Shr
                ) && self
                    .analyzed
                    .typed
                    .type_table
                    .call_resolution(expr_id)
                    .is_some()
                {
                    return self.lower_selected_operator(expr_id, &[lhs, rhs]);
                }
                if matches!(
                    op,
                    HirBinaryOp::Lt | HirBinaryOp::Le | HirBinaryOp::Gt | HirBinaryOp::Ge
                ) && self
                    .analyzed
                    .typed
                    .type_table
                    .call_resolution(expr_id)
                    .is_some()
                {
                    return self.lower_ordering_operator(expr_id, op, &[lhs, rhs]);
                }
                if matches!(op, HirBinaryOp::Eq | HirBinaryOp::NotEq) {
                    let ty = self
                        .planner
                        .arguments(
                            &[operand_ty],
                            &self.instance.substitution,
                            self.function.debug.source_span,
                        )?
                        .remove(0);
                    let equal = self.lower_protocol(Protocol::PartialEq, &ty, &[lhs, rhs], 0)?;
                    if op == HirBinaryOp::Eq {
                        return Ok(equal);
                    }
                    let dst = self.alloc_temp(ValueType::Bool);
                    self.emit(Instruction::Unary {
                        dst,
                        op: UnaryOp::Not,
                        operand: equal,
                    });
                    return Ok(dst);
                }
                let dst = self.alloc_temp(self.expr_type(expr_id)?);
                self.emit(Instruction::Binary {
                    dst,
                    op: FunctionLowerer::lower_binary_op(op),
                    lhs,
                    rhs,
                });
                Ok(dst)
            }
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => self.lower_range(expr_id, start, end, inclusive),
            ExprKind::Call { args, .. } => self.lower_call(expr_id, &args),
            ExprKind::Block(block) => {
                if let Some(temp) = self.lower_block(block)? {
                    Ok(temp)
                } else {
                    Ok(self.lower_unit())
                }
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.lower_if(expr_id, condition, then_branch, else_branch),
            ExprKind::Field { receiver, .. } => self.lower_field(expr_id, receiver),
            ExprKind::Index { receiver, index } => self.lower_index(expr_id, receiver, index),
            ExprKind::Match { scrutinee, arms } => self.lower_match(expr_id, scrutinee, arms),
            ExprKind::Loop { body } => self.lower_loop_expr(expr_id, body),
            ExprKind::StructInit { fields, .. } => self.lower_struct_init(expr_id, fields),
            ExprKind::Tuple(elements) if elements.is_empty() => Ok(self.lower_unit()),
            ExprKind::Tuple(elements) => self.lower_tuple(expr_id, elements),
            ExprKind::ArrayRepeat { value, count } => {
                let values = match self.lower_values(&[value, count])? {
                    ControlFlow::Continue(values) => values,
                    ControlFlow::Break(value) => return Ok(value),
                };
                let dst = self.alloc_temp(self.expr_type(expr_id)?);
                self.emit(Instruction::RepeatArray {
                    element: self.array_element_type(expr_id)?,
                    dst,
                    value: values[0],
                    count: values[1],
                });
                Ok(dst)
            }
            ExprKind::Array(elements) => self.lower_array(expr_id, elements),
            ExprKind::Closure { .. } => self.lower_closure(expr_id),
        }
    }

    fn lower_if(
        &mut self,
        expr_id: ExprId,
        condition: Condition,
        then_branch: BlockId,
        else_branch: Option<ExprId>,
    ) -> Result<MirValue, MirLoweringError> {
        let cond = self.lower_expr(condition.value())?;
        if self.current_block_terminated() {
            return Ok(cond);
        }
        let outer_scope = self.current_scope;
        let then_block = self.new_block();
        let else_block = self.new_block();
        let join_block = self.new_block();
        let result = self.alloc_temp(self.expr_type(expr_id)?);
        let mut bindings = Vec::new();
        match condition {
            Condition::Expr(_) => self.set_terminator(Terminator::Branch {
                cond,
                then_block,
                else_block,
            }),
            Condition::Binding {
                pattern,
                initializer,
            } => {
                let ty = self
                    .analyzed
                    .typed
                    .type_table
                    .expr_type(initializer)
                    .ok_or(MirLoweringError::MissingExprType(initializer))?;
                self.lower_pattern_decision(pattern, cond, &ty, Some(else_block), &mut bindings)?;
                self.set_terminator(Terminator::Jump(then_block));
            }
        }

        self.switch_to_block(then_block);
        self.publish_pattern_bindings(bindings)?;
        let then_value = self.lower_block(then_branch)?;
        if !self.current_block_terminated() {
            let then_value = if else_branch.is_some() {
                then_value.unwrap_or_else(|| self.lower_unit())
            } else {
                self.lower_unit()
            };
            self.emit(Instruction::Move {
                dst: result,
                src: then_value,
            });
            self.set_terminator(Terminator::Jump(join_block));
        }

        self.current_scope = outer_scope;
        self.switch_to_block(else_block);
        let else_value = match else_branch {
            Some(expr) => self.lower_expr(expr)?,
            None => self.lower_unit(),
        };
        if !self.current_block_terminated() {
            self.emit(Instruction::Move {
                dst: result,
                src: else_value,
            });
            self.set_terminator(Terminator::Jump(join_block));
        }

        self.switch_to_join(join_block);
        Ok(result)
    }

    fn lower_short_circuit(
        &mut self,
        expr_id: ExprId,
        lhs: ExprId,
        op: HirBinaryOp,
        rhs: ExprId,
    ) -> Result<MirValue, MirLoweringError> {
        let lhs = self.lower_expr(lhs)?;
        if self.current_block_terminated() {
            return Ok(lhs);
        }
        let rhs_block = self.new_block();
        let short_block = self.new_block();
        let join_block = self.new_block();
        let result = self.alloc_temp(self.expr_type(expr_id)?);

        match op {
            HirBinaryOp::AndAnd => {
                self.set_terminator(Terminator::Branch {
                    cond: lhs,
                    then_block: rhs_block,
                    else_block: short_block,
                });

                self.switch_to_block(short_block);
                let short_value = self.lower_constant(Constant::Bool(false), ValueType::Bool);
                self.emit(Instruction::Move {
                    dst: result,
                    src: short_value,
                });
                self.set_terminator(Terminator::Jump(join_block));
            }
            HirBinaryOp::OrOr => {
                self.set_terminator(Terminator::Branch {
                    cond: lhs,
                    then_block: short_block,
                    else_block: rhs_block,
                });

                self.switch_to_block(short_block);
                let short_value = self.lower_constant(Constant::Bool(true), ValueType::Bool);
                self.emit(Instruction::Move {
                    dst: result,
                    src: short_value,
                });
                self.set_terminator(Terminator::Jump(join_block));
            }
            _ => unreachable!("short-circuit lowering called for non-short-circuit op"),
        }

        self.switch_to_block(rhs_block);
        let rhs = self.lower_expr(rhs)?;
        if !self.current_block_terminated() {
            self.emit(Instruction::Move {
                dst: result,
                src: rhs,
            });
            self.set_terminator(Terminator::Jump(join_block));
        }

        self.switch_to_block(join_block);
        Ok(result)
    }

    fn lower_loop_expr(
        &mut self,
        expr: ExprId,
        body: BlockId,
    ) -> Result<MirValue, MirLoweringError> {
        let result = self.alloc_temp(self.expr_type(expr)?);
        let body_block = self.new_block();
        let exit_block = self.new_block();
        self.ensure_jump(body_block);
        self.loops.push(LoopScope {
            break_block: exit_block,
            continue_block: body_block,
            break_value: Some(result),
        });
        self.switch_to_block(body_block);
        let _ = self.lower_block(body)?;
        self.ensure_jump(body_block);
        self.loops.pop();
        self.switch_to_join(exit_block);
        Ok(result)
    }
}
