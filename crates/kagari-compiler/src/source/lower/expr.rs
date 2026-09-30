use crate::source::{lower::support::lower_scalar, types::raise_type};
mod aggregates;
mod calls;
mod native_defaults;
mod native_results;
mod native_sources;
mod patterns;
use crate::source::lower::{instances::CallableInstance, state::LoopScope};
use hir::{BinaryOp as HirBinaryOp, Condition, ExprKind};
use kagari_abi::{
    numeric::NumericConversion,
    operations::{StandardEnumOp, UnaryOp},
    standard::traits::StandardTrait,
};
use kagari_common::collection::CollectionAccess;
use kagari_hir::{
    native::NativeTypeKind,
    resolver::ResolvedName,
    typeck::{CallTarget as TypeckCallTarget, ResolvedInterfaceImplementation},
    types::TypeId,
    types::abi::{lower_nominal_type, lower_type},
};
mod adapters;
mod branches;
mod collections;
mod equality;
mod iterators;
mod list_queries;
mod list_windows;
mod native_calls;
mod native_contracts;
mod native_keys;
mod operators;
mod set_queries;
mod standard;
mod terminals;

use kagari_abi::{representation::ValueType, standard::StandardIntrinsic};
use kagari_hir::hir;
use std::ops::ControlFlow;

use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_mir::instruction::{
    CallTarget, Constant, Instruction, MirValue, Terminator, ValueBuffer,
};

impl FunctionLowerer<'_, '_> {
    fn lower_closure(&mut self, expr_id: hir::ExprId) -> Result<MirValue, MirLoweringError> {
        let mut captures = ValueBuffer::new();
        for resolved in self.analyzed.names.closure_captures(expr_id) {
            let local = self.lookup_binding(*resolved)?;
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
            let physical = if matches!(resolved, kagari_hir::resolver::ResolvedName::Local(id) if self.cell_locals.contains(id))
            {
                ValueType::HeapObject
            } else {
                self.value_type(&ty)?
            };
            let value = self.alloc_temp(physical);
            self.emit(Instruction::LoadLocal { dst: value, local });
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
        expressions: &[hir::ExprId],
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

    fn record_expr_layout(&mut self, expr_id: hir::ExprId) -> Result<(), MirLoweringError> {
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

    pub(crate) fn lower_expr(
        &mut self,
        expr_id: hir::ExprId,
    ) -> Result<MirValue, MirLoweringError> {
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
                    let [TypeId::Trait(source), TypeId::Trait(target)] = types.as_slice() else {
                        return Err(MirLoweringError::MissingBinding("interface upcast types"));
                    };
                    let dst = self.alloc_temp(ValueType::HeapObject);
                    self.emit(Instruction::UpcastInterface {
                        dst,
                        value,
                        source: lower_nominal_type(source),
                        target: lower_nominal_type(target),
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
                self.planner
                    .require_parent_interfaces(&types[0], interface, span)?;
                let (implementation, arguments) = match coercion.implementation {
                    ResolvedInterfaceImplementation::Upcast => unreachable!("upcast handled above"),
                    ResolvedInterfaceImplementation::Native => (
                        self.planner.native_interface(&types[0], interface, span)?,
                        Vec::new(),
                    ),
                    ResolvedInterfaceImplementation::Host => {
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
                            return Err(MirLoweringError::MissingBinding("host interface type"));
                        };
                        (
                            self.planner.host_interface(&receiver, &interface, span)?,
                            Vec::new(),
                        )
                    }
                    ResolvedInterfaceImplementation::Script {
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
                        if declaration.module == *self.analyzed.lowered.source.module_identity() {
                            let signature = self
                                .analyzed
                                .aggregates
                                .implementation_signature(&declaration)
                                .ok_or(MirLoweringError::MissingBinding(
                                    "interface implementation",
                                ))?;
                            let methods = self.planner.catalog.implementation_methods(signature);
                            for method in methods {
                                self.planner.enqueue_declaration(
                                    &method,
                                    arguments.clone(),
                                    span,
                                )?;
                            }
                        }
                        (declaration, arguments.iter().map(lower_type).collect())
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
            }
        }
        if !self.current_block_terminated() {
            let ty = self
                .analyzed
                .typed
                .type_table
                .coerced_expr_type(expr_id)
                .ok_or(MirLoweringError::MissingExprType(expr_id))?;
            if ty != TypeId::Unknown && ty != TypeId::Error {
                let span = self.analyzed.lowered.source_map.expr_span(expr_id);
                let ty = self
                    .planner
                    .arguments(&[ty], &self.instance.substitution, span)?
                    .remove(0);
                let abi = lower_type(&ty);
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

    fn lower_expr_value(&mut self, expr_id: hir::ExprId) -> Result<MirValue, MirLoweringError> {
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
                    interface,
                    &types[0],
                    &Default::default(),
                    100_000,
                    64,
                    &self.planner.options.cancel,
                )
                .ok()
                .flatten()
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
            let native = enumeration.native_type;
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
            if let Some(native) = native {
                let ty = self
                    .analyzed
                    .typed
                    .type_table
                    .expr_type(expr_id)
                    .ok_or(MirLoweringError::MissingExprType(expr_id))?;
                if !matches!(&ty, TypeId::StandardEnum { kind, .. } if native == NativeTypeKind::Enum(*kind))
                    || arity > 1
                {
                    return Err(MirLoweringError::MissingBinding(
                        "checked native enum representation",
                    ));
                }
                let slot = u32::try_from(variant)
                    .map_err(|_| MirLoweringError::MissingBinding("native enum slot"))?;
                return self.standard_enum_op(
                    &ty,
                    StandardEnumOp::Make(slot),
                    fields.first().copied(),
                );
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
            ExprKind::Propagate { expr } => {
                let value = self.lower_expr(expr)?;
                if self.current_block_terminated() {
                    return Ok(value);
                }
                let ty = self
                    .analyzed
                    .typed
                    .type_table
                    .expr_type(expr)
                    .ok_or(MirLoweringError::MissingExprType(expr))?;
                let cond = self.standard_enum_op(&ty, StandardEnumOp::Test(0), Some(value))?;
                let success = self.new_block();
                let failure = self.new_block();
                self.set_terminator(Terminator::Branch {
                    cond,
                    then_block: success,
                    else_block: failure,
                });
                self.switch_to_block(failure);
                let output = self.function.semantic.result.clone().ok_or(
                    MirLoweringError::MissingBinding("propagation return contract"),
                )?;
                let residual = if matches!(
                    &ty,
                    kagari_hir::types::TypeId::StandardEnum {
                        kind: kagari_abi::standard::surface::StandardEnum::Result,
                        ..
                    }
                ) {
                    let error = self.standard_enum_op(&ty, StandardEnumOp::Read(1), Some(value))?;
                    let conversion = self
                        .analyzed
                        .typed
                        .type_table
                        .call_resolution(expr_id)
                        .ok_or(MirLoweringError::MissingBinding("propagation conversion"))?;
                    let TypeckCallTarget::TraitMethod { method, interface } = conversion.target
                    else {
                        return Err(MirLoweringError::MissingBinding(
                            "propagation From contract",
                        ));
                    };
                    let target = self
                        .analyzed
                        .typed
                        .type_table
                        .protocol_receiver(expr_id)
                        .cloned()
                        .ok_or(MirLoweringError::MissingBinding("propagation error type"))?;
                    let error =
                        self.lower_applied_operator(interface, target, &method, &[error])?;
                    let dst = self.alloc_temp(ValueType::HeapObject);
                    self.emit(Instruction::MapResultError {
                        dst,
                        original: value,
                        error,
                        ty: output,
                    });
                    dst
                } else {
                    self.standard_enum_op(&raise_type(&output), StandardEnumOp::Make(1), None)?
                };
                self.set_terminator(Terminator::Return(Some(residual)));
                self.switch_to_block(success);
                self.standard_enum_op(&ty, StandardEnumOp::Read(0), Some(value))
            }
            ExprKind::InterpolatedString(parts) => {
                let elements = match self.lower_values(&parts)? {
                    ControlFlow::Continue(values) => values,
                    ControlFlow::Break(value) => return Ok(value),
                };
                let array = self.alloc_temp(ValueType::HeapObject);
                self.emit(Instruction::MakeArray {
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
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::ArrayJoin),
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
                let conversion = NumericConversion {
                    source,
                    target,
                    checked: false,
                };
                let (_, result) = conversion
                    .contract()
                    .ok_or(MirLoweringError::MissingBinding("numeric cast"))?;
                let dst = self.alloc_temp(result.representation());
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
                if matches!(op, hir::BinaryOp::AndAnd | hir::BinaryOp::OrOr) {
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
                    hir::BinaryOp::Add
                        | hir::BinaryOp::Sub
                        | hir::BinaryOp::Mul
                        | hir::BinaryOp::Div
                        | hir::BinaryOp::Rem
                        | hir::BinaryOp::BitAnd
                        | hir::BinaryOp::BitOr
                        | hir::BinaryOp::BitXor
                        | hir::BinaryOp::Shl
                        | hir::BinaryOp::Shr
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
                    hir::BinaryOp::Lt | hir::BinaryOp::Le | hir::BinaryOp::Gt | hir::BinaryOp::Ge
                ) && self
                    .analyzed
                    .typed
                    .type_table
                    .call_resolution(expr_id)
                    .is_some()
                {
                    return self.lower_ordering_operator(expr_id, op, &[lhs, rhs]);
                }
                if matches!(op, hir::BinaryOp::Eq | hir::BinaryOp::NotEq) {
                    let ty = self
                        .planner
                        .arguments(
                            &[operand_ty],
                            &self.instance.substitution,
                            self.function.debug.source_span,
                        )?
                        .remove(0);
                    let equal =
                        self.lower_protocol(StandardTrait::PartialEq, &ty, &[lhs, rhs], 0)?;
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
        expr_id: hir::ExprId,
        condition: hir::Condition,
        then_branch: hir::BlockId,
        else_branch: Option<hir::ExprId>,
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
                self.lower_pattern_decision(pattern, cond, &ty, else_block, &mut bindings)?;
                self.set_terminator(Terminator::Jump(then_block));
            }
        }

        self.switch_to_block(then_block);
        for (local, value) in bindings {
            self.emit(Instruction::StoreLocal { local, src: value });
            self.introduce_debug_local(local);
        }
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
        expr_id: hir::ExprId,
        lhs: hir::ExprId,
        op: hir::BinaryOp,
        rhs: hir::ExprId,
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
        expr: hir::ExprId,
        body: hir::BlockId,
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
