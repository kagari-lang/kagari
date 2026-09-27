use crate::source::lower::MirLoweringError;
use crate::source::lower::state::FunctionLowerer;
use crate::source::types::lower_nominal_type;
use kagari_abi::operations::IterOp;
use kagari_abi::operations::StringIterKind;
use kagari_abi::representation::ValueType;
use kagari_abi::scalar::BuiltinType;
use kagari_abi::standard::StandardIntrinsic;
use kagari_abi::types::NominalAbiType;
use kagari_common::host_interface;
use kagari_hir::builtin::BuiltinFunction;
use kagari_hir::builtin::traits;
use kagari_hir::builtin::traits::StandardTrait;
use kagari_hir::declarations::DeclarationId;
use kagari_hir::hir;
use kagari_hir::resolver::ResolvedName;
use kagari_hir::typeck::CallTarget as TypeckCallTarget;
use kagari_hir::typeck::ScalarValue;
use kagari_hir::types::NominalType;
use kagari_hir::types::TypeId;
use kagari_mir::instruction::CallTarget;
use kagari_mir::instruction::Instruction;
use kagari_mir::instruction::InterfaceCallContract;
use kagari_mir::instruction::MirValue;
use kagari_mir::instruction::RuntimeHelper;
use kagari_mir::instruction::SourceFunctionContract;
use kagari_mir::instruction::ValueBuffer;
use smallvec::SmallVec;
use std::ops::ControlFlow;
use std::slice;

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
                        arguments,
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
                let TypeId::Function { params, result } = self
                    .analyzed
                    .typed
                    .type_table
                    .expr_type(receiver)
                    .ok_or(MirLoweringError::MissingExprType(receiver))?
                else {
                    return Err(MirLoweringError::MissingBinding("checked closure callee"));
                };
                let param_types = params
                    .iter()
                    .map(|ty| self.value_type(ty))
                    .collect::<Result<Vec<_>, _>>()?;
                let return_type = self.value_type(&result)?;
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
                let target = match target {
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
                            let typed = self
                                .analyzed
                                .typed
                                .functions
                                .iter()
                                .find(|function| function.id == id)
                                .ok_or(MirLoweringError::MissingTypedFunction(id))?;
                            let substitution = typed
                                .generic_params
                                .iter()
                                .cloned()
                                .zip(arguments.iter().cloned())
                                .collect();
                            let params = typed
                                .params
                                .iter()
                                .map(|param| {
                                    self.planner.value_type(&param.ty, &substitution, span)
                                })
                                .collect::<Result<_, _>>()?;
                            let return_type =
                                self.planner
                                    .value_type(&typed.return_type, &substitution, span)?;
                            CallTarget::SourceFunction(Box::new(SourceFunctionContract {
                                declaration: declaration.clone(),
                                arguments,
                                params,
                                return_type,
                            }))
                        }
                    }
                    TypeckCallTarget::SourceFunction(id) => {
                        let imported =
                            self.analyzed.imported_functions.target(id).ok_or(
                                MirLoweringError::MissingBinding("source function contract"),
                            )?;
                        let params = imported
                            .signature
                            .params
                            .iter()
                            .map(|param| {
                                self.planner
                                    .value_type(&param.ty, &Default::default(), span)
                            })
                            .collect::<Result<_, _>>()?;
                        let return_type = self.planner.value_type(
                            &imported.signature.return_type,
                            &Default::default(),
                            span,
                        )?;
                        CallTarget::SourceFunction(Box::new(SourceFunctionContract {
                            declaration: imported.declaration.clone(),
                            arguments: Vec::new(),
                            params,
                            return_type,
                        }))
                    }
                    TypeckCallTarget::StandardIntrinsic(intrinsic) => {
                        if matches!(
                            intrinsic,
                            StandardIntrinsic::ArrayRetain
                                | StandardIntrinsic::MapRetain
                                | StandardIntrinsic::SetRetain
                                | StandardIntrinsic::ArraySort
                                | StandardIntrinsic::ArraySortBy
                                | StandardIntrinsic::ArraySortByKey
                                | StandardIntrinsic::ArrayDedup
                        ) {
                            let base = call
                                .receiver
                                .or_else(|| args.first().copied())
                                .ok_or(MirLoweringError::MissingBinding("collection receiver"))?;
                            let receiver = self
                                .analyzed
                                .typed
                                .type_table
                                .expr_type(base)
                                .ok_or(MirLoweringError::MissingExprType(base))?;
                            let receiver = self
                                .planner
                                .arguments(&[receiver], &self.instance.substitution, span)?
                                .remove(0);
                            let callback_type = if intrinsic == StandardIntrinsic::ArraySortByKey {
                                let site = *args
                                    .last()
                                    .ok_or(MirLoweringError::MissingBinding("sort callback"))?;
                                let ty = self
                                    .analyzed
                                    .typed
                                    .type_table
                                    .coerced_expr_type(site)
                                    .ok_or(MirLoweringError::MissingExprType(site))?;
                                Some(
                                    self.planner
                                        .arguments(&[ty], &self.instance.substitution, span)?
                                        .remove(0),
                                )
                            } else {
                                None
                            };
                            return self.lower_prepared_collection(
                                intrinsic,
                                &receiver,
                                &lowered,
                                callback_type.as_ref(),
                            );
                        }

                        if matches!(
                            intrinsic,
                            StandardIntrinsic::MapGetOrInsertWith | StandardIntrinsic::MapUpdate
                        ) {
                            let base = call
                                .receiver
                                .or_else(|| args.first().copied())
                                .ok_or(MirLoweringError::MissingBinding("map receiver"))?;
                            let receiver = self
                                .analyzed
                                .typed
                                .type_table
                                .expr_type(base)
                                .ok_or(MirLoweringError::MissingExprType(base))?;
                            let receiver = self
                                .planner
                                .arguments(&[receiver], &self.instance.substitution, span)?
                                .remove(0);
                            return self.lower_map_update(intrinsic, &receiver, &lowered);
                        }

                        if intrinsic == StandardIntrinsic::StringParse {
                            let output = self
                                .analyzed
                                .typed
                                .type_table
                                .expr_type(expr)
                                .ok_or(MirLoweringError::MissingExprType(expr))?;
                            let output = self
                                .planner
                                .arguments(&[output], &self.instance.substitution, span)?
                                .remove(0);
                            let TypeId::StandardEnum { args: members, .. } = output else {
                                return Err(MirLoweringError::MissingBinding("parse result"));
                            };
                            return self.lower_applied_operator(
                                StandardTrait::FromStr.nominal(),
                                members[0].clone(),
                                &StandardTrait::FromStr.contract().methods[0].id,
                                &lowered,
                            );
                        }

                        let string_iteration = match intrinsic {
                            StandardIntrinsic::StringBytes => Some(StringIterKind::Bytes),
                            StandardIntrinsic::StringCharIndices => {
                                Some(StringIterKind::CharIndices)
                            }
                            StandardIntrinsic::StringSplit => Some(StringIterKind::Split),
                            StandardIntrinsic::StringSplitN => Some(StringIterKind::SplitN),
                            StandardIntrinsic::StringSplitWhitespace => {
                                Some(StringIterKind::Whitespace)
                            }
                            StandardIntrinsic::StringLines => Some(StringIterKind::Lines),
                            _ => None,
                        };
                        if let Some(kind) = string_iteration {
                            let source = self.alloc_temp(ValueType::HeapObject);
                            self.emit(Instruction::MakeTuple {
                                dst: source,
                                elements: lowered,
                            });
                            let dst = self.alloc_temp(ValueType::HeapObject);
                            self.emit(Instruction::Iter {
                                dst,
                                value: Some(source),
                                ty: kind.source_type(),
                                op: IterOp::String(kind),
                            });
                            return Ok(dst);
                        }

                        if matches!(
                            intrinsic,
                            StandardIntrinsic::MapKeys
                                | StandardIntrinsic::MapValues
                                | StandardIntrinsic::MapEntries
                        ) {
                            return self.lower_map_snapshot(expr, intrinsic, lowered[0]);
                        }
                        if matches!(
                            intrinsic,
                            StandardIntrinsic::ArrayCopyFrom | StandardIntrinsic::ArrayExtend
                        ) {
                            let source_expr = *args
                                .last()
                                .ok_or(MirLoweringError::MissingBinding("copy source"))?;
                            let source = self
                                .analyzed
                                .typed
                                .type_table
                                .interface_coercion(source_expr)
                                .map(|coercion| TypeId::Trait(coercion.interface_type.clone()))
                                .or_else(|| self.analyzed.typed.type_table.expr_type(source_expr))
                                .ok_or(MirLoweringError::MissingExprType(source_expr))?;
                            let source = self
                                .planner
                                .arguments(
                                    &[source],
                                    &self.instance.substitution,
                                    self.function.debug.source_span,
                                )?
                                .remove(0);
                            return self.lower_list_copy(
                                source,
                                lowered[0],
                                lowered[1],
                                intrinsic == StandardIntrinsic::ArrayExtend,
                            );
                        }

                        if intrinsic == StandardIntrinsic::ArrayListFromFn {
                            return self.lower_array_from_fn(expr, lowered[0], lowered[1]);
                        }
                        if matches!(
                            intrinsic,
                            StandardIntrinsic::ArrayCopyWithin
                                | StandardIntrinsic::ArrayRemoveRange
                        ) {
                            let input = args[usize::from(call.receiver.is_none())];
                            let source = self
                                .analyzed
                                .typed
                                .type_table
                                .expr_type(input)
                                .ok_or(MirLoweringError::MissingExprType(input))?;
                            let source = self
                                .planner
                                .arguments(&[source], &self.instance.substitution, span)?
                                .remove(0);
                            let mut interface = StandardTrait::RangeBounds.nominal();
                            interface
                                .arguments
                                .push(TypeId::Builtin(BuiltinType::USize));
                            let methods = &StandardTrait::RangeBounds.contract().methods;
                            let start = self.lower_applied_operator(
                                interface.clone(),
                                source.clone(),
                                &methods[0].id,
                                &[lowered[1]],
                            )?;
                            let end = self.lower_applied_operator(
                                interface,
                                source,
                                &methods[1].id,
                                &[lowered[1]],
                            )?;
                            if intrinsic == StandardIntrinsic::ArrayRemoveRange {
                                let base = call
                                    .receiver
                                    .or_else(|| args.first().copied())
                                    .ok_or(MirLoweringError::MissingBinding("array receiver"))?;
                                let ty = self
                                    .analyzed
                                    .typed
                                    .type_table
                                    .expr_type(base)
                                    .ok_or(MirLoweringError::MissingExprType(base))?;
                                let ty = self
                                    .planner
                                    .arguments(&[ty], &self.instance.substitution, span)?
                                    .remove(0);
                                let TypeId::Array(item, _) = &ty else {
                                    return Err(MirLoweringError::MissingBinding("array storage"));
                                };
                                self.emit_intrinsic(
                                    StandardIntrinsic::CollectionMutationBegin,
                                    &[lowered[0]],
                                    ValueType::Unit,
                                );
                                let prepared = self.emit_intrinsic(
                                    StandardIntrinsic::ArrayRemoveRangePrepare,
                                    &[lowered[0], start, end],
                                    ValueType::HeapObject,
                                );
                                let remaining = self.prepared_field(prepared, 0, &ty)?;
                                let removed = self.prepared_field(prepared, 1, &ty)?;
                                let result = self.readonly_array((**item).clone(), removed)?;
                                self.emit_intrinsic(
                                    StandardIntrinsic::CollectionMutationEnd,
                                    &[lowered[0]],
                                    ValueType::Unit,
                                );
                                self.emit_intrinsic(
                                    StandardIntrinsic::ArrayReplaceStorage,
                                    &[lowered[0], remaining],
                                    ValueType::Unit,
                                );
                                return Ok(result);
                            }
                            return Ok(self.emit_intrinsic(
                                StandardIntrinsic::ArrayCopyWithinBounds,
                                &[lowered[0], start, end, lowered[2]],
                                ValueType::Unit,
                            ));
                        }
                        if matches!(
                            intrinsic,
                            StandardIntrinsic::ArrayListFrom
                                | StandardIntrinsic::LinkedHashMapFrom
                                | StandardIntrinsic::LinkedHashSetFrom
                        ) {
                            return self.lower_collection_factory(expr, lowered[0]);
                        }
                        let base = call.receiver.or_else(|| args.first().copied());
                        if let Some(base) = base {
                            let ty = self
                                .analyzed
                                .typed
                                .type_table
                                .expr_type(base)
                                .ok_or(MirLoweringError::MissingExprType(base))?;
                            let ty = self
                                .planner
                                .arguments(&[ty], &self.instance.substitution, span)?
                                .remove(0);
                            if intrinsic == StandardIntrinsic::DebugAssertEq {
                                let equal = self.lower_protocol(
                                    StandardTrait::PartialEq,
                                    &ty,
                                    &lowered[..2],
                                    0,
                                )?;
                                return Ok(self.emit_intrinsic(
                                    StandardIntrinsic::DebugAssert,
                                    &[equal, lowered[2]],
                                    ValueType::Unit,
                                ));
                            }
                            let key = match &ty {
                                TypeId::Map { key, .. } | TypeId::Set(key, _) => Some(&**key),
                                _ => None,
                            };
                            if let Some(key) = key
                                && self.has_custom_protocol(key)
                                && matches!(
                                    intrinsic,
                                    StandardIntrinsic::MapGet
                                        | StandardIntrinsic::MapContainsKey
                                        | StandardIntrinsic::MapInsert
                                        | StandardIntrinsic::MapRemove
                                        | StandardIntrinsic::SetContains
                                        | StandardIntrinsic::SetInsert
                                        | StandardIntrinsic::SetRemove
                                )
                            {
                                return self.lower_key_operation(intrinsic, key, &lowered);
                            }
                        }
                        if matches!(
                            intrinsic,
                            StandardIntrinsic::OptionUnwrapOrElse
                                | StandardIntrinsic::OptionOrElse
                                | StandardIntrinsic::OptionMapOr
                                | StandardIntrinsic::OptionMapOrElse
                                | StandardIntrinsic::OptionFilter
                                | StandardIntrinsic::OptionIsSomeAnd
                                | StandardIntrinsic::OptionZip
                                | StandardIntrinsic::OptionFlatten
                                | StandardIntrinsic::OptionTranspose
                                | StandardIntrinsic::ResultUnwrapOrElse
                                | StandardIntrinsic::ResultOrElse
                                | StandardIntrinsic::ResultMapOr
                                | StandardIntrinsic::ResultMapOrElse
                                | StandardIntrinsic::ResultOk
                                | StandardIntrinsic::ResultErr
                                | StandardIntrinsic::ResultIsOkAnd
                                | StandardIntrinsic::ResultIsErrAnd
                                | StandardIntrinsic::ResultFlatten
                                | StandardIntrinsic::ResultTranspose
                                | StandardIntrinsic::OptionMap
                                | StandardIntrinsic::OptionAndThen
                                | StandardIntrinsic::OptionOkOr
                                | StandardIntrinsic::OptionOkOrElse
                                | StandardIntrinsic::ResultMap
                                | StandardIntrinsic::ResultMapErr
                                | StandardIntrinsic::ResultAndThen
                        ) {
                            let base = call
                                .receiver
                                .or_else(|| args.first().copied())
                                .ok_or(MirLoweringError::MissingBinding("standard receiver"))?;
                            let base_ty = self
                                .analyzed
                                .typed
                                .type_table
                                .expr_type(base)
                                .ok_or(MirLoweringError::MissingExprType(base))?;
                            return self
                                .lower_standard_combinator(expr, intrinsic, &base_ty, &lowered);
                        }
                        CallTarget::StandardIntrinsic(intrinsic)
                    }
                    TypeckCallTarget::HostFunction(id) => CallTarget::HostFunction(Box::new(
                        self.analyzed
                            .names
                            .hosts
                            .function(id)
                            .ok_or(MirLoweringError::MissingBinding("host declaration"))?
                            .clone(),
                    )),
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
                CallTarget::HostFunction(Box::new(host_interface::standard_log())),
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
