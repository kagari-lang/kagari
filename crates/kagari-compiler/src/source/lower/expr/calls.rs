use crate::source::lower::expr::native_contracts::NativeApplication;
use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::{
    callable::NativeCall,
    representation::ValueType,
    standard::{StandardIntrinsic, traits::StandardTrait},
};
use kagari_common::host_interface;
use kagari_hir::{
    builtin::{BuiltinFunction, traits},
    declarations::DeclarationId,
    hir,
    resolver::ResolvedName,
    typeck::{CallTarget as TypeckCallTarget, ScalarValue},
    types::abi::{lower_nominal_type, lower_type},
    types::{NominalType, TypeId},
};
use kagari_mir::instruction::{
    CallTarget, Instruction, InterfaceCallContract, MirValue, RuntimeHelper,
    SourceFunctionContract, ValueBuffer,
};
use smallvec::SmallVec;
use std::{ops::ControlFlow, slice};

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_call(
        &mut self,
        expr: hir::ExprId,
        args: &[hir::ExprId],
    ) -> Result<MirValue, MirLoweringError> {
        let call = self
            .analyzed
            .typed
            .type_table
            .call_resolution(expr)
            .ok_or(MirLoweringError::MissingBinding("checked call target"))?;
        let span = self.analyzed.lowered.source_map.expr_span(expr);
        if let Some(receiver_type) = self
            .analyzed
            .typed
            .type_table
            .protocol_receiver(expr)
            .cloned()
        {
            let TypeckCallTarget::TraitMethod { method, interface } = call.target else {
                return Err(MirLoweringError::MissingBinding("conversion target"));
            };
            let mut values = Vec::new();
            if let Some(receiver) = call.receiver {
                let value = self.lower_expr(receiver)?;
                if self.current_block_terminated() {
                    return Ok(value);
                }
                values.push(value);
            }
            match self.lower_values(args)? {
                ControlFlow::Continue(args) => values.extend(args),
                ControlFlow::Break(value) => return Ok(value),
            }
            return self.lower_applied_method(
                interface,
                receiver_type,
                &method,
                &call.type_arguments,
                &values,
            );
        }

        if let TypeckCallTarget::TraitMethod { ref interface, .. } = call.target
            && StandardTrait::from_id(&interface.declaration).is_some_and(|kind| {
                kind.operator()
                    || kind.collection()
                    || kind.iteration()
                    || kind == StandardTrait::RangeBounds
            })
        {
            let receiver = call
                .receiver
                .ok_or(MirLoweringError::MissingBinding("operator receiver"))?;
            let value = self.lower_expr(receiver)?;
            if self.current_block_terminated() {
                return Ok(value);
            }
            let mut values = vec![value];
            match self.lower_values(args)? {
                ControlFlow::Continue(args) => values.extend(args),
                ControlFlow::Break(value) => return Ok(value),
            };
            if StandardTrait::from_id(&interface.declaration) == Some(StandardTrait::Fn)
                && matches!(&self.analyzed.lowered.module.expr(expr).kind, hir::ExprKind::Call { callee, .. } if *callee == receiver)
            {
                let receiver_type = self
                    .analyzed
                    .typed
                    .type_table
                    .expr_type(receiver)
                    .ok_or(MirLoweringError::MissingExprType(receiver))?;
                let receiver_type = self
                    .planner
                    .arguments(&[receiver_type], &self.instance.substitution, span)?
                    .remove(0);
                if let TypeId::Function { result, .. } = receiver_type {
                    // Specializing a callable bound does not allocate an argument tuple for closures.
                    return self.call_function_value(values[0], &result, &values[1..]);
                }
                let packed = if values.len() == 1 {
                    self.lower_unit()
                } else {
                    let dst = self.alloc_temp(ValueType::HeapObject);
                    self.emit(Instruction::MakeTuple {
                        dst,
                        elements: values[1..].iter().copied().collect(),
                    });
                    dst
                };
                values.truncate(1);
                values.push(packed);
            }
            return self.lower_selected_operator(expr, &values);
        }
        if let TypeckCallTarget::TraitMethod { ref interface, .. } = call.target
            && let Some(protocol) = StandardTrait::from_id(&interface.declaration)
            && protocol.equality_protocol()
        {
            let receiver = call
                .receiver
                .ok_or(MirLoweringError::MissingBinding("protocol receiver"))?;
            let ty = self
                .analyzed
                .typed
                .type_table
                .expr_type(receiver)
                .ok_or(MirLoweringError::MissingExprType(receiver))?;
            let ty = self
                .planner
                .arguments(&[ty], &self.instance.substitution, span)?
                .remove(0);
            let value = self.lower_expr(receiver)?;
            if self.current_block_terminated() {
                return Ok(value);
            }
            let mut values = vec![value];
            match self.lower_values(args)? {
                ControlFlow::Continue(args) => values.extend(args),
                ControlFlow::Break(value) => return Ok(value),
            }
            return self.lower_protocol(protocol, &ty, &values, 0);
        }
        let (target, impl_arguments, linked_trait_target) = if let TypeckCallTarget::TraitMethod {
            method,
            interface,
        } = call.target
        {
            let receiver = call
                .receiver
                .ok_or(MirLoweringError::MissingBinding("trait receiver"))?;
            let ty = self
                .analyzed
                .typed
                .type_table
                .expr_type(receiver)
                .ok_or(MirLoweringError::MissingExprType(receiver))?;
            let mut types = self
                .planner
                .arguments(&[ty], &self.instance.substitution, span)?;
            let ty = types.pop().expect("receiver type");
            let interface = NominalType {
                associated_types: interface
                    .associated_types
                    .iter()
                    .map(|(id, ty)| {
                        let mut types = self.planner.arguments(
                            slice::from_ref(ty),
                            &self.instance.substitution,
                            span,
                        )?;
                        Ok((id.clone(), types.pop().expect("associated type")))
                    })
                    .collect::<Result<_, MirLoweringError>>()?,
                declaration: interface.declaration,
                arguments: self.planner.arguments(
                    &interface.arguments,
                    &self.instance.substitution,
                    span,
                )?,
            };
            if matches!(&ty, kagari_hir::types::TypeId::Trait(child)
                    if self.analyzed.aggregates.trait_closure(child, &ty, &self.planner.options.cancel)
                        .is_ok_and(|parents| parents.contains(&interface)))
            {
                let trait_contract = self
                    .planner
                    .catalog
                    .trait_(&interface.declaration)
                    .ok_or(MirLoweringError::MissingBinding("trait contract"))?;
                let method_contract = self
                    .planner
                    .catalog
                    .trait_method(&method)
                    .ok_or(MirLoweringError::MissingBinding("trait method contract"))?;
                if method_contract.generic_params.len() != trait_contract.generic_params.len()
                    || !call.type_arguments.is_empty()
                {
                    return Err(MirLoweringError::UnsupportedExpr(
                        "interface method requires static specialization",
                    ));
                }
                (
                    TypeckCallTarget::TraitMethod {
                        method,
                        interface: interface.clone(),
                    },
                    Vec::new(),
                    Some(CallTarget::InterfaceMethod(Box::new(
                        InterfaceCallContract {
                            interface: lower_nominal_type(&interface),
                            method_slot: u32::try_from(method_contract.slot).map_err(|_| {
                                MirLoweringError::UnsupportedExpr("interface method slot overflow")
                            })?,
                        },
                    ))),
                )
            } else if let Some(host_method) = self
                .analyzed
                .names
                .hosts
                .trait_method_binding(&method, &interface, &ty)
            {
                (
                    TypeckCallTarget::HostFunction(host_method),
                    Vec::new(),
                    None,
                )
            } else if let Some((implementation, impl_arguments)) = self
                .analyzed
                .typed
                .type_table
                .implementation_method(&method, &interface, &ty)
            {
                (
                    TypeckCallTarget::Function(implementation),
                    impl_arguments,
                    None,
                )
            } else if self
                .planner
                .catalog
                .implementation_method(&method, &interface, &ty)
                .is_none()
                && let Some(protocol) = StandardTrait::from_id(&interface.declaration)
                && traits::intrinsic_holds(
                    protocol,
                    &ty,
                    Some(self.planner.catalog),
                    &Default::default(),
                )
            {
                let intrinsic = match protocol {
                    StandardTrait::PartialOrd => StandardIntrinsic::ValuePartialCmp,
                    StandardTrait::Ord => StandardIntrinsic::ValueCmp,
                    StandardTrait::PartialEq => StandardIntrinsic::ValueEq,
                    StandardTrait::Hash => StandardIntrinsic::ValueHash,
                    StandardTrait::Debug => StandardIntrinsic::ValueDebug,
                    StandardTrait::Display => StandardIntrinsic::ValueDisplay,
                    StandardTrait::Eq => unreachable!("marker trait has no methods"),
                    _ => unreachable!("operator protocol handled above"),
                };
                (
                    TypeckCallTarget::TraitMethod { method, interface },
                    Vec::new(),
                    Some(CallTarget::StandardIntrinsic(intrinsic)),
                )
            } else {
                let (implementation, impl_arguments) = self
                    .planner
                    .catalog
                    .implementation_method(&method, &interface, &ty)
                    .ok_or(MirLoweringError::UnsupportedExpr(
                        "interface dispatch requires linked implementation tables",
                    ))?;
                let trait_contract = self
                    .planner
                    .catalog
                    .trait_(&interface.declaration)
                    .ok_or(MirLoweringError::MissingBinding("trait contract"))?;
                let method_contract = self
                    .planner
                    .catalog
                    .trait_method(&method)
                    .ok_or(MirLoweringError::MissingBinding("trait method contract"))?;
                let method_params =
                    &method_contract.generic_params[trait_contract.generic_params.len()..];
                let method_arguments = self.planner.arguments(
                    &call.type_arguments,
                    &self.instance.substitution,
                    span,
                )?;
                if method_params.len() != method_arguments.len() {
                    return Err(MirLoweringError::MissingBinding(
                        "checked trait method type arguments",
                    ));
                }
                let substitution = trait_contract
                    .generic_params
                    .iter()
                    .cloned()
                    .zip(interface.arguments.iter().cloned())
                    .chain(
                        method_params
                            .iter()
                            .cloned()
                            .zip(method_arguments.iter().cloned()),
                    )
                    .collect();
                let params = method_contract
                    .params
                    .iter()
                    .map(|param| {
                        let ty = param
                            .ty
                            .with_self(&method_contract.owner, &ty)
                            .instantiate(&substitution);
                        self.planner.value_type(&ty, &Default::default(), span)
                    })
                    .collect::<Result<_, _>>()?;
                let return_type = self.planner.value_type(
                    &method_contract
                        .return_type
                        .with_self(&method_contract.owner, &ty)
                        .instantiate(&substitution),
                    &Default::default(),
                    span,
                )?;
                let arguments = impl_arguments
                    .into_iter()
                    .chain(method_arguments)
                    .collect::<Vec<_>>();
                let linked = if implementation.module
                    == *self.planner.owner().lowered.source.module_identity()
                {
                    CallTarget::Function(self.planner.enqueue_declaration(
                        &implementation,
                        arguments.clone(),
                        span,
                    )?)
                } else {
                    CallTarget::SourceFunction(Box::new(SourceFunctionContract {
                        declaration: implementation.clone(),
                        arguments: arguments.iter().map(lower_type).collect(),
                        params,
                        return_type,
                    }))
                };
                (
                    TypeckCallTarget::TraitMethod { method, interface },
                    Vec::new(),
                    Some(linked),
                )
            }
        } else {
            (call.target, Vec::new(), None)
        };
        let (callee, args) = match target {
            TypeckCallTarget::TerminatingCallee => {
                let callee = call.receiver.ok_or(MirLoweringError::MissingBinding(
                    "checked terminating callee",
                ))?;
                let value = self.lower_expr(callee)?;
                if !self.current_block_terminated() {
                    return Err(MirLoweringError::MissingBinding(
                        "callee termination contract",
                    ));
                }
                return Ok(value);
            }
            TypeckCallTarget::RuntimeHelper(helper) => {
                match self.lower_runtime_helper_call(helper, args)? {
                    ControlFlow::Continue(call) => call,
                    ControlFlow::Break(value) => return Ok(value),
                }
            }
            TypeckCallTarget::Value => {
                let receiver = call
                    .receiver
                    .ok_or(MirLoweringError::MissingBinding("closure callee"))?;
                let signature = call
                    .signature
                    .as_ref()
                    .ok_or(MirLoweringError::MissingBinding(
                        "checked closure application",
                    ))?;
                let param_types = signature
                    .params
                    .iter()
                    .map(|ty| self.value_type(ty))
                    .collect::<Result<Vec<_>, _>>()?;
                let return_type = self.value_type(&signature.return_type)?;
                let value = self.lower_expr(receiver)?;
                if self.current_block_terminated() {
                    return Ok(value);
                }
                let args = match self.lower_values(args)? {
                    ControlFlow::Continue(values) => values,
                    ControlFlow::Break(value) => return Ok(value),
                };
                (
                    CallTarget::Closure {
                        value,
                        params: param_types,
                        return_type,
                    },
                    args,
                )
            }
            target => {
                let mut lowered = ValueBuffer::new();
                if let Some(receiver) = call.receiver {
                    let value = self.lower_expr(receiver)?;
                    if self.current_block_terminated() {
                        return Ok(value);
                    }
                    lowered.push(value);
                }
                match self.lower_values(args)? {
                    ControlFlow::Continue(values) => lowered.extend(values),
                    ControlFlow::Break(value) => return Ok(value),
                }
                if self.engine_native_for_call(&target)? {
                    return self.lower_engine_call(
                        expr,
                        lowered,
                        NativeApplication {
                            target: &target,
                            signature: call.signature.as_ref().expect("checked native application"),
                            arguments: &impl_arguments
                                .iter()
                                .chain(&call.type_arguments)
                                .cloned()
                                .collect::<Vec<_>>(),
                        },
                    );
                }
                let target =
                    match target {
                        TypeckCallTarget::Function(id) => {
                            let arguments = self.planner.arguments(
                                &impl_arguments
                                    .iter()
                                    .chain(&call.type_arguments)
                                    .cloned()
                                    .collect::<Vec<_>>(),
                                &self.instance.substitution,
                                span,
                            )?;
                            let declaration = self
                                .analyzed
                                .declarations
                                .target(ResolvedName::Function(id))
                                .ok_or(MirLoweringError::MissingBinding("call declaration"))?;
                            let DeclarationId::Definition(declaration) = &declaration.id else {
                                return Err(MirLoweringError::MissingBinding("call identity"));
                            };
                            if declaration.module
                                == *self.planner.owner().lowered.source.module_identity()
                            {
                                CallTarget::Function(self.planner.enqueue(id, arguments, span)?)
                            } else {
                                let signature = call.signature.as_ref().ok_or(
                                    MirLoweringError::MissingBinding("checked source application"),
                                )?;
                                let params = signature
                                    .params
                                    .iter()
                                    .map(|ty| self.value_type(ty))
                                    .collect::<Result<_, _>>()?;
                                let return_type = self.value_type(&signature.return_type)?;
                                CallTarget::SourceFunction(Box::new(SourceFunctionContract {
                                    declaration: declaration.clone(),
                                    arguments: arguments.iter().map(lower_type).collect(),
                                    params,
                                    return_type,
                                }))
                            }
                        }
                        TypeckCallTarget::SourceFunction(id) => {
                            let imported = self.analyzed.imported_functions.target(&id).ok_or(
                                MirLoweringError::MissingBinding("source function contract"),
                            )?;
                            let signature =
                                call.signature
                                    .as_ref()
                                    .ok_or(MirLoweringError::MissingBinding(
                                        "checked imported application",
                                    ))?;
                            let params = signature
                                .params
                                .iter()
                                .map(|ty| self.value_type(ty))
                                .collect::<Result<_, _>>()?;
                            let return_type = self.value_type(&signature.return_type)?;
                            CallTarget::SourceFunction(Box::new(SourceFunctionContract {
                                declaration: imported.declaration.clone(),
                                arguments: Vec::new(),
                                params,
                                return_type,
                            }))
                        }
                        TypeckCallTarget::HostFunction(id) => {
                            CallTarget::Native(NativeCall::Host(Box::new(
                                self.analyzed
                                    .names
                                    .hosts
                                    .function(id)
                                    .ok_or(MirLoweringError::MissingBinding("host declaration"))?
                                    .clone(),
                            )))
                        }
                        TypeckCallTarget::TraitMethod { .. } => linked_trait_target.ok_or(
                            MirLoweringError::MissingBinding("imported implementation contract"),
                        )?,
                        TypeckCallTarget::TerminatingCallee
                        | TypeckCallTarget::RuntimeHelper(_)
                        | TypeckCallTarget::Value => {
                            unreachable!()
                        }
                    };
                (target, lowered)
            }
        };
        let dst = self.alloc_temp(self.expr_type(expr)?);
        self.emit(Instruction::Call {
            dst: Some(dst),
            callee,
            args,
        });
        Ok(dst)
    }

    pub(super) fn lower_runtime_helper_call(
        &mut self,
        helper: BuiltinFunction,
        args: &[hir::ExprId],
    ) -> Result<ControlFlow<MirValue, (CallTarget, ValueBuffer)>, MirLoweringError> {
        let (target, operands): (_, SmallVec<[hir::ExprId; 3]>) = match (helper, args) {
            (BuiltinFunction::TypeOf, [value]) => (
                CallTarget::RuntimeHelper(RuntimeHelper::ReflectTypeOf),
                smallvec::smallvec![*value],
            ),
            (BuiltinFunction::GetField, [base, field]) => (
                CallTarget::RuntimeHelper(RuntimeHelper::ReflectGetField(
                    self.checked_field_name(*field)?,
                )),
                smallvec::smallvec![*base],
            ),
            (BuiltinFunction::SetField, [base, field, value]) => (
                CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetField(
                    self.checked_field_name(*field)?,
                )),
                smallvec::smallvec![*base, *value],
            ),
            (BuiltinFunction::SetIndex, [base, index, value]) => (
                CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetIndex),
                smallvec::smallvec![*base, *index, *value],
            ),
            (BuiltinFunction::Print, [message]) => (
                CallTarget::Native(NativeCall::Host(Box::new(host_interface::standard_log()))),
                smallvec::smallvec![*message],
            ),
            _ => {
                return Err(MirLoweringError::MissingBinding(
                    "checked runtime helper arguments",
                ));
            }
        };
        Ok(self
            .lower_values(&operands)?
            .map_continue(|values| (target, values)))
    }

    pub(super) fn checked_field_name(&self, expr: hir::ExprId) -> Result<String, MirLoweringError> {
        match self.analyzed.typed.type_table.scalar_value(expr) {
            Some(ScalarValue::String(value)) => Ok(value.clone()),
            _ => Err(MirLoweringError::MissingBinding(
                "checked reflection field name",
            )),
        }
    }
}
