use crate::source::{
    lower::{MirLoweringError, state::FunctionLowerer},
    types,
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::DefinitionPath;
use kagari_contract::{
    callable::interface::InterfaceCallContract,
    numeric::{NumericConversion, NumericOperation},
    operations::{BinaryOp, UnaryOp},
    representation::semantic_representation,
    standard::RuntimePrimitive,
};
use kagari_hir::{
    aggregates::traits::MethodDefault,
    hir::{expr::ops::BinaryOp as HirBinaryOp, ids::ExprId},
    language::semantics as traits,
    native::NativeBinding,
    typeck::table::CallTarget as HirCallTarget,
    types::{
        NominalType, TypeId, TypeSubstitution,
        semantic::{lower_nominal_type, lower_type},
    },
};
use kagari_mir::instruction::{
    CallTarget, Constant, Instruction, MirValue, SourceFunctionContract, Terminator, ValueBuffer,
};
use kagari_types::{
    declaration::conversion::ConversionAdapter, integer::IntegerOp, language::Protocol,
    scalar::BuiltinType,
};

impl FunctionLowerer<'_, '_> {
    pub(super) fn check_integer_range(&mut self, value: MirValue, target: &TypeId) {
        let representation = value.ty;

        let range = match target {
            TypeId::Builtin(BuiltinType::I8) => Some((i64::from(i8::MIN), i64::from(i8::MAX))),
            TypeId::Builtin(BuiltinType::I16) => Some((i64::from(i16::MIN), i64::from(i16::MAX))),
            TypeId::Builtin(BuiltinType::U8) => Some((0, i64::from(u8::MAX))),
            TypeId::Builtin(BuiltinType::U16) => Some((0, i64::from(u16::MAX))),
            TypeId::Builtin(BuiltinType::U32) => Some((0, i64::from(u32::MAX))),
            _ => None,
        };
        if let Some((minimum, maximum)) = range {
            let message =
                self.lower_constant(Constant::Str("integer overflow".into()), ValueType::Str);
            for (limit, comparison) in [(minimum, BinaryOp::Ge), (maximum, BinaryOp::Le)] {
                let constant = if representation == ValueType::I32 {
                    Constant::I32(limit as i32)
                } else {
                    Constant::I64(limit)
                };
                let limit = self.lower_constant(constant, representation);
                let valid = self.alloc_temp(ValueType::Bool);
                self.emit(Instruction::Binary {
                    dst: valid,
                    op: comparison,
                    lhs: value,
                    rhs: limit,
                });
                self.emit_intrinsic(RuntimePrimitive::Assert, &[valid, message], ValueType::Unit);
            }
        }
    }

    pub(super) fn lower_selected_operator(
        &mut self,
        site: ExprId,
        args: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let call = self
            .analyzed
            .typed
            .type_table
            .call_resolution(site)
            .ok_or(MirLoweringError::MissingBinding("operator contract"))?;
        let HirCallTarget::TraitMethod { method, interface } = call.target else {
            return Err(MirLoweringError::MissingBinding("operator method"));
        };
        let receiver = call
            .receiver
            .ok_or(MirLoweringError::MissingBinding("operator receiver"))?;
        let receiver = self
            .analyzed
            .typed
            .type_table
            .expr_type(receiver)
            .ok_or(MirLoweringError::MissingExprType(receiver))?;
        self.lower_applied_method(interface, receiver, &method, &call.type_arguments, args)
    }

    pub(crate) fn lower_applied_operator(
        &mut self,
        interface: NominalType,
        receiver: TypeId,
        method: &DefinitionPath,
        args: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        self.lower_applied_method(interface, receiver, method, &[], args)
    }

    pub(crate) fn lower_applied_method(
        &mut self,
        interface: NominalType,
        receiver: TypeId,
        method: &DefinitionPath,
        method_arguments: &[TypeId],
        args: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let method_arguments = self.planner.arguments(
            method_arguments,
            &self.instance.substitution,
            self.function.debug.source_span,
        )?;
        let ty = self
            .planner
            .arguments(
                &[receiver],
                &self.instance.substitution,
                self.function.debug.source_span,
            )?
            .remove(0);
        let TypeId::Trait(interface) = self
            .planner
            .arguments(
                &[TypeId::Trait(interface)],
                &self.instance.substitution,
                self.function.debug.source_span,
            )?
            .remove(0)
        else {
            unreachable!()
        };

        if let Some((required, target)) =
            traits::conversion_requirement(&interface, &ty, Some(self.planner.catalog))
        {
            let method = self
                .planner
                .catalog
                .trait_(&interface.declaration)
                .and_then(|contract| contract.conversion_adapter.as_ref())
                .and_then(|adapter| match adapter {
                    ConversionAdapter::Reverse { method, .. } => Some(method.clone()),
                    _ => None,
                })
                .ok_or(MirLoweringError::MissingBinding(
                    "registered forward conversion member",
                ))?;
            return self.lower_applied_method(required, target, &method, &method_arguments, args);
        }

        if Protocol::from_id(&interface.declaration) == Some(Protocol::Fn)
            && let TypeId::Function { params, result } = &ty
        {
            let mut values = ValueBuffer::new();
            let mut representations = Vec::new();
            for (position, param) in params.iter().enumerate() {
                let representation = self.value_type(param)?;
                let dst = self.alloc_temp(representation);
                let index = self.lower_constant(Constant::I32(position as i32), ValueType::I32);
                self.emit(Instruction::ReadAggregateIndex {
                    dst,
                    base: args[1],
                    index,
                });
                values.push(dst);
                representations.push(representation);
            }
            let return_type = self.value_type(result)?;
            let dst = self.alloc_temp(return_type);
            self.emit(Instruction::Call {
                dst: Some(dst),
                callee: CallTarget::Closure {
                    value: args[0],
                    params: representations,
                    return_type,
                },
                args: values,
            });
            return Ok(dst);
        }

        let interface = if let TypeId::Trait(child) = &ty {
            self.planner
                .catalog
                .interface_closure(child, &ty, &self.planner.options.cancel)
                .ok()
                .and_then(|parents| {
                    parents
                        .into_iter()
                        .find(|parent| parent.satisfies(&interface))
                })
                .unwrap_or(interface)
        } else {
            interface
        };
        let native_default = self
            .planner
            .catalog
            .trait_method(method)
            .and_then(|signature| match signature.default.clone() {
                Some(MethodDefault::Native(binding)) => Some(binding),
                _ => None,
            });
        if matches!(&ty, TypeId::Generic(_))
            || (matches!(&ty, TypeId::Trait(child)
            if (native_default.is_none()
                || matches!(native_default, Some(NativeBinding::Default(_))))
            && self
                .planner
                .catalog
                .interface_closure(child, &ty, &self.planner.options.cancel)
                .is_ok_and(|parents| parents.contains(&interface))))
        {
            let contract = self
                .planner
                .catalog
                .trait_(&interface.declaration)
                .ok_or(MirLoweringError::MissingBinding("dynamic protocol"))?;
            let signature = self
                .planner
                .catalog
                .trait_method(method)
                .ok_or(MirLoweringError::MissingBinding("dynamic protocol method"))?;
            let local_parameters = &signature.generic_params[contract.generic_params.len()..];
            if local_parameters.len() != method_arguments.len() {
                return Err(MirLoweringError::MissingBinding("dynamic method arguments"));
            }
            let mut substitution: TypeSubstitution = contract
                .generic_params
                .iter()
                .cloned()
                .zip(interface.arguments.iter().cloned())
                .chain(
                    local_parameters
                        .iter()
                        .cloned()
                        .zip(method_arguments.iter().cloned()),
                )
                .collect();
            substitution.insert_receiver(contract.id.clone(), ty.clone());
            let result = self.planner.catalog.normalize_type(
                &signature
                    .return_type
                    .instantiate(&substitution)
                    .with_associated_types(&interface),
            );
            let dst = self.alloc_temp(self.value_type(&result)?);
            self.function
                .semantic
                .registers
                .insert(dst.temp.index(), lower_type(&result));
            let operations = self.planner.method_operations(
                method,
                &interface,
                &ty,
                &method_arguments,
                self.function.debug.source_span,
            )?;
            let normalizations =
                self.planner
                    .method_normalizations(method, &interface, &ty, &method_arguments)?;
            self.emit(Instruction::Call {
                dst: Some(dst),
                callee: CallTarget::InterfaceMethod(Box::new(InterfaceCallContract {
                    normalizations,
                    receiver: matches!(&ty, TypeId::Generic(_)).then(|| lower_type(&ty)),
                    operations,
                    arguments: method_arguments.iter().map(lower_type).collect(),
                    interface: lower_nominal_type(&interface),
                    method_slot: signature.slot as u32,
                })),
                args: args.iter().copied().collect(),
            });
            return Ok(dst);
        }
        if let TypeId::Builtin(input) = &ty {
            let op = match Protocol::from_id(&interface.declaration) {
                Some(Protocol::BitAnd) => Some(IntegerOp::BitAnd),
                Some(Protocol::BitOr) => Some(IntegerOp::BitOr),
                Some(Protocol::BitXor) => Some(IntegerOp::BitXor),
                Some(Protocol::Shl) => Some(IntegerOp::Shl),
                Some(Protocol::Shr) => Some(IntegerOp::Shr),
                Some(Protocol::Not) if input.integer_layout().is_some() => Some(IntegerOp::BitNot),
                _ => None,
            };
            if let Some(op) = op {
                let rhs = match interface.arguments.first() {
                    Some(TypeId::Builtin(rhs)) => Some(*rhs),
                    None => None,
                    _ => return Err(MirLoweringError::MissingBinding("numeric rhs type")),
                };
                let operation = NumericOperation {
                    op,
                    input: *input,
                    rhs,
                };
                let (_, _, result) = operation
                    .contract()
                    .ok_or(MirLoweringError::MissingBinding("numeric contract"))?;
                let dst = self.alloc_temp(semantic_representation(&result));
                self.emit(Instruction::Numeric {
                    dst,
                    operation,
                    lhs: args[0],
                    rhs: args.get(1).copied(),
                });
                return Ok(dst);
            }
        }

        if native_default.is_some()
            && self
                .planner
                .catalog
                .implementation_method(method, &interface, &ty)
                .is_none()
        {
            return self.lower_native_default(&ty, &interface, method, &method_arguments, args);
        }
        if Protocol::from_id(&interface.declaration) == Some(Protocol::From)
            && interface.arguments.as_slice() == [ty.clone()]
        {
            return Ok(args[0]);
        }
        if let (TypeId::Builtin(target), [TypeId::Builtin(source)]) =
            (&ty, interface.arguments.as_slice())
            && Protocol::from_id(&interface.declaration) == Some(Protocol::From)
            && traits::intrinsic_applies(
                &interface,
                &ty,
                Some(self.planner.catalog),
                &Default::default(),
            )
        {
            let conversion = NumericConversion {
                source: *source,
                target: *target,
            };
            let (_, output) = conversion
                .contract()
                .ok_or(MirLoweringError::MissingBinding(
                    "numeric conversion contract",
                ))?;
            let dst = self.alloc_temp(semantic_representation(&output));
            self.emit(Instruction::Convert {
                dst,
                src: args[0],
                conversion,
            });
            return Ok(dst);
        }

        let contract = self
            .planner
            .catalog
            .trait_(&interface.declaration)
            .ok_or(MirLoweringError::MissingBinding("operator trait"))?;
        let signature = self
            .planner
            .catalog
            .trait_method(method)
            .ok_or(MirLoweringError::MissingBinding("operator signature"))?;
        let mut substitution: TypeSubstitution = contract
            .generic_params
            .iter()
            .cloned()
            .zip(interface.arguments.iter().cloned())
            .collect();
        substitution.insert_receiver(contract.id.clone(), ty.clone());
        substitution.extend(
            signature
                .generic_params
                .iter()
                .skip(contract.generic_params.len())
                .cloned()
                .zip(method_arguments.iter().cloned()),
        );
        let result = self.planner.catalog.normalize_type(
            &signature
                .return_type
                .with_self(&contract.id, &ty)
                .instantiate(&substitution)
                .with_associated_types(&interface),
        );
        let result_ty = self.value_type(&result)?;
        let callee = if let Some((declaration, mut arguments)) = self
            .planner
            .catalog
            .implementation_method(method, &interface, &ty)
        {
            arguments.extend(method_arguments);
            if self.planner.native_function(&declaration).is_some() {
                return self.lower_native_implementation(
                    &declaration,
                    &arguments,
                    &ty,
                    &result,
                    args,
                );
            }
            if declaration.module == *self.planner.owner().lowered.source.module_identity() {
                CallTarget::Function(self.planner.enqueue_declaration(
                    &declaration,
                    arguments,
                    self.function.debug.source_span,
                )?)
            } else {
                CallTarget::SourceFunction(Box::new(SourceFunctionContract {
                    declaration,
                    arguments: arguments.iter().map(lower_type).collect(),
                    params: args.iter().map(|a| a.ty).collect(),
                    return_type: result_ty,
                }))
            }
        } else {
            if Protocol::from_id(&interface.declaration) == Some(Protocol::Iterable)
                && result == ty
                && traits::iterator_requirement(&interface, &ty).is_some_and(|required| {
                    traits::intrinsic_applies(
                        &required,
                        &ty,
                        Some(self.planner.catalog),
                        &Default::default(),
                    )
                })
            {
                return Ok(args[0]);
            }
            if let Some(protocol) = Protocol::from_id(&interface.declaration)
                && protocol.binary_operator()
            {
                let op = match protocol {
                    Protocol::Add => BinaryOp::Add,
                    Protocol::Sub => BinaryOp::Sub,
                    Protocol::Mul => BinaryOp::Mul,
                    Protocol::Div => BinaryOp::Div,
                    Protocol::Rem => BinaryOp::Rem,
                    _ => unreachable!(),
                };
                let dst = self.alloc_temp(result_ty);
                if protocol == Protocol::Rem
                    && let TypeId::Builtin(input @ (BuiltinType::I8 | BuiltinType::I16)) = ty
                {
                    let operation = types::lower_numeric_operation(HirBinaryOp::Rem, input, input)
                        .expect("integer remainder");
                    self.emit(Instruction::Numeric {
                        dst,
                        operation,
                        lhs: args[0],
                        rhs: Some(args[1]),
                    });
                } else {
                    self.emit(Instruction::Binary {
                        dst,
                        op,
                        lhs: args[0],
                        rhs: args[1],
                    });
                    self.check_integer_range(dst, &result);
                }
                return Ok(dst);
            }
            if let Some(protocol) = Protocol::from_id(&interface.declaration)
                && matches!(protocol, Protocol::Neg | Protocol::Not)
            {
                let dst = self.alloc_temp(result_ty);
                self.emit(Instruction::Unary {
                    dst,
                    op: if protocol == Protocol::Neg {
                        UnaryOp::Neg
                    } else {
                        UnaryOp::Not
                    },
                    operand: args[0],
                });
                self.check_integer_range(dst, &result);
                return Ok(dst);
            }
            if Protocol::from_id(&interface.declaration) == Some(Protocol::Index) {
                let dst = self.alloc_temp(result_ty);
                self.emit(Instruction::ReadAggregateIndex {
                    dst,
                    base: args[0],
                    index: args[1],
                });
                return Ok(dst);
            }
            let intrinsic = match Protocol::from_id(&interface.declaration) {
                Some(Protocol::PartialOrd) => RuntimePrimitive::ValuePartialCmp,
                Some(Protocol::Ord) => RuntimePrimitive::ValueCmp,
                _ => return Err(MirLoweringError::MissingBinding("builtin operator")),
            };
            CallTarget::RuntimePrimitive(intrinsic)
        };
        let dst = self.alloc_temp(result_ty);
        self.emit(Instruction::Call {
            dst: Some(dst),
            callee,
            args: args.iter().copied().collect(),
        });
        Ok(dst)
    }

    pub(super) fn lower_ordering_operator(
        &mut self,
        site: ExprId,
        op: HirBinaryOp,
        args: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        // Primitive numeric comparisons retain their direct instruction path.
        if matches!(
            args[0].ty,
            ValueType::I32 | ValueType::I64 | ValueType::U64 | ValueType::F32 | ValueType::F64
        ) {
            let dst = self.alloc_temp(ValueType::Bool);
            let op = match op {
                HirBinaryOp::Lt => BinaryOp::Lt,
                HirBinaryOp::Le => BinaryOp::Le,
                HirBinaryOp::Gt => BinaryOp::Gt,
                _ => BinaryOp::Ge,
            };
            self.emit(Instruction::Binary {
                dst,
                op,
                lhs: args[0],
                rhs: args[1],
            });
            return Ok(dst);
        }

        let value = self.lower_selected_operator(site, args)?;
        let optional = traits::ordering_type(true);
        let ordering = traits::ordering_type(false);
        let some = self.test_enum_variant(&optional, value, 0)?;
        let body = self.new_block();
        let absent = self.new_block();
        let join = self.new_block();
        let dst = self.alloc_temp(ValueType::Bool);
        self.set_terminator(Terminator::Branch {
            cond: some,
            then_block: body,
            else_block: absent,
        });
        self.switch_to_block(body);
        let order = self.read_enum_field(&optional, value, 0, 0)?;
        let tag = if matches!(op, HirBinaryOp::Lt | HirBinaryOp::Ge) {
            0
        } else {
            2
        };
        let test = self.test_enum_variant(&ordering, order, tag)?;
        if matches!(op, HirBinaryOp::Le | HirBinaryOp::Ge) {
            self.emit(Instruction::Unary {
                dst,
                op: UnaryOp::Not,
                operand: test,
            });
        } else {
            self.emit(Instruction::Move { dst, src: test });
        }
        self.set_terminator(Terminator::Jump(join));
        self.switch_to_block(absent);
        let no = self.lower_constant(Constant::Bool(false), ValueType::Bool);
        self.emit(Instruction::Move { dst, src: no });
        self.set_terminator(Terminator::Jump(join));
        self.switch_to_block(join);
        Ok(dst)
    }
}
