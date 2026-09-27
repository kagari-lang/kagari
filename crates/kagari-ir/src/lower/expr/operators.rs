use super::*;
use kagari_hir::{builtin::traits::StandardTrait, types::TypeId};

impl FunctionLowerer<'_, '_> {
    pub(super) fn check_integer_range(&mut self, value: IrValue, target: &TypeId) {
        let representation = value.ty;
        use kagari_hir::types::BuiltinType;
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
                self.emit_intrinsic(
                    StandardIntrinsic::DebugAssert,
                    &[valid, message],
                    ValueType::Unit,
                );
            }
        }
    }

    pub(super) fn lower_selected_operator(
        &mut self,
        site: hir::ExprId,
        args: &[IrValue],
    ) -> Result<IrValue, IrLoweringError> {
        let call = self
            .analyzed
            .typed
            .type_table
            .call_resolution(site)
            .ok_or(IrLoweringError::MissingBinding("operator contract"))?;
        let kagari_hir::typeck::CallTarget::TraitMethod { method, interface } = call.target else {
            return Err(IrLoweringError::MissingBinding("operator method"));
        };
        let receiver = call
            .receiver
            .ok_or(IrLoweringError::MissingBinding("operator receiver"))?;
        let receiver = self
            .analyzed
            .typed
            .type_table
            .expr_type(receiver)
            .ok_or(IrLoweringError::MissingExprType(receiver))?;
        self.lower_applied_method(interface, receiver, &method, &call.type_arguments, args)
    }

    pub(crate) fn lower_applied_operator(
        &mut self,
        interface: kagari_hir::types::NominalType,
        receiver: TypeId,
        method: &kagari_common::identity::DefinitionId,
        args: &[IrValue],
    ) -> Result<IrValue, IrLoweringError> {
        self.lower_applied_method(interface, receiver, method, &[], args)
    }

    pub(crate) fn lower_applied_method(
        &mut self,
        interface: kagari_hir::types::NominalType,
        receiver: TypeId,
        method: &kagari_common::identity::DefinitionId,
        method_arguments: &[TypeId],
        args: &[IrValue],
    ) -> Result<IrValue, IrLoweringError> {
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

        if let TypeId::Builtin(input) = &ty {
            use kagari_common::integer::IntegerOp;
            let op = match StandardTrait::from_id(&interface.declaration) {
                Some(StandardTrait::BitAnd) => Some(IntegerOp::BitAnd),
                Some(StandardTrait::BitOr) => Some(IntegerOp::BitOr),
                Some(StandardTrait::BitXor) => Some(IntegerOp::BitXor),
                Some(StandardTrait::Shl) => Some(IntegerOp::Shl),
                Some(StandardTrait::Shr) => Some(IntegerOp::Shr),
                Some(StandardTrait::Not) if input.integer_layout().is_some() => {
                    Some(IntegerOp::BitNot)
                }
                _ => None,
            };
            if let Some(op) = op {
                let rhs = match interface.arguments.first() {
                    Some(TypeId::Builtin(rhs)) => Some(*rhs),
                    None => None,
                    _ => return Err(IrLoweringError::MissingBinding("numeric rhs type")),
                };
                let operation = crate::module::numeric::NumericOperation {
                    op,
                    input: *input,
                    rhs,
                };
                let (_, _, result) = operation
                    .contract()
                    .ok_or(IrLoweringError::MissingBinding("numeric contract"))?;
                let dst = self.alloc_temp(result.representation());
                self.emit(Instruction::Numeric {
                    dst,
                    operation,
                    lhs: args[0],
                    rhs: args.get(1).copied(),
                });
                return Ok(dst);
            }
        }

        if let Some(operation) = kagari_hir::builtin::declarations::iterator_method(method)
            && self
                .planner
                .catalog
                .implementation_method(method, &interface, &ty)
                .is_none()
        {
            use kagari_hir::builtin::declarations::IteratorMethod::*;
            if matches!(operation, Sum | Product) {
                let protocol = if operation == Sum {
                    StandardTrait::Sum
                } else {
                    StandardTrait::Product
                };
                let target = &method_arguments[0];
                let mut contract = protocol.nominal();
                contract.arguments.push(self.iterator_item(&ty)?);
                return self.lower_applied_method(
                    contract,
                    target.clone(),
                    &protocol.contract().methods[0].id,
                    std::slice::from_ref(&ty),
                    args,
                );
            }
            if matches!(
                operation,
                Find | Any
                    | All
                    | Count
                    | Fold
                    | ForEach
                    | Partition
                    | GroupBy
                    | FindMap
                    | Position
                    | Nth
                    | Last
                    | Reduce
                    | MinBy
                    | MaxBy
                    | Min
                    | Max
                    | MinByKey
                    | MaxByKey
            ) {
                return self.lower_iterator_terminal(operation, &ty, &method_arguments, args);
            }
            if operation != kagari_hir::builtin::declarations::IteratorMethod::Collect {
                return self.lower_iterator_adapter(operation, &ty, &method_arguments, args);
            }
            let target = method_arguments
                .first()
                .ok_or(IrLoweringError::MissingBinding("collect destination"))?;
            let item = self.iterator_item(&ty)?;
            let mut contract = StandardTrait::FromIterator.nominal();
            contract.arguments.push(item);
            return self.lower_applied_method(
                contract,
                target.clone(),
                &StandardTrait::FromIterator.contract().methods[0].id,
                std::slice::from_ref(&ty),
                args,
            );
        }
        if StandardTrait::from_id(&interface.declaration) == Some(StandardTrait::FromIterator)
            && kagari_hir::builtin::traits::intrinsic_applies(
                &interface,
                &ty,
                Some(self.planner.catalog),
                &Default::default(),
            )
        {
            if kagari_hir::builtin::traits::lifted_collection_requirement(&interface, &ty).is_some()
            {
                return self.lower_fallible_collect(&ty, &method_arguments[0], args[0]);
            }
            return self.lower_collect(&ty, &method_arguments[0], args[0]);
        }
        if let Some(protocol) = StandardTrait::from_id(&interface.declaration)
            && protocol.aggregation()
            && kagari_hir::builtin::traits::intrinsic_applies(
                &interface,
                &ty,
                Some(self.planner.catalog),
                &Default::default(),
            )
        {
            return self.lower_numeric_aggregate(protocol, &ty, &method_arguments[0], args[0]);
        }

        if let Some((required, target)) =
            kagari_hir::builtin::traits::conversion_requirement(&interface, &ty)
        {
            let kind = StandardTrait::from_id(&required.declaration).expect("forward conversion");
            return self.lower_applied_operator(
                required,
                target,
                &kind.contract().methods[0].id,
                args,
            );
        }
        if StandardTrait::from_id(&interface.declaration) == Some(StandardTrait::From)
            && interface.arguments.as_slice() == [ty.clone()]
        {
            return Ok(args[0]);
        }
        if let (
            Some(protocol @ (StandardTrait::From | StandardTrait::TryFrom)),
            TypeId::Builtin(target),
            [TypeId::Builtin(source)],
        ) = (
            StandardTrait::from_id(&interface.declaration),
            &ty,
            interface.arguments.as_slice(),
        ) && kagari_hir::builtin::traits::intrinsic_applies(
            &interface,
            &ty,
            Some(self.planner.catalog),
            &Default::default(),
        ) {
            let conversion = crate::module::numeric::NumericConversion {
                source: *source,
                target: *target,
                checked: protocol == StandardTrait::TryFrom,
            };
            let (_, output) = conversion
                .contract()
                .ok_or(IrLoweringError::MissingBinding(
                    "numeric conversion contract",
                ))?;
            let dst = self.alloc_temp(output.representation());
            self.emit(Instruction::Convert {
                dst,
                src: args[0],
                conversion,
            });
            return Ok(dst);
        }
        use crate::module::{abi::AbiType, instruction::IterOp};
        if StandardTrait::from_id(&interface.declaration) == Some(StandardTrait::RangeBounds)
            && matches!(ty, TypeId::Range(_, _))
        {
            let upper = method.path.last().is_some_and(|p| p.name == "end_bound");
            let bound = AbiType::StandardEnum {
                kind: kagari_hir::builtin::surface::StandardEnum::Bound,
                args: interface
                    .arguments
                    .iter()
                    .map(AbiType::from_checked_type)
                    .collect(),
            };
            let dst = self.alloc_temp(ValueType::HeapObject);
            self.emit(Instruction::RangeBound {
                dst,
                value: args[0],
                range: AbiType::from_checked_type(&ty),
                bound,
                upper,
            });
            return Ok(dst);
        }
        let iter_op = match StandardTrait::from_id(&interface.declaration) {
            Some(StandardTrait::Iterable)
                if matches!(
                    ty,
                    TypeId::Range(_, _)
                        | TypeId::Array(_, _)
                        | TypeId::Set(_, _)
                        | TypeId::Map { .. }
                        | TypeId::Builtin(kagari_hir::types::BuiltinType::String)
                ) =>
            {
                Some(IterOp::New)
            }
            Some(StandardTrait::Iterator) if matches!(ty, TypeId::Iter(_)) => Some(IterOp::Next),
            _ => None,
        };
        if let Some(op) = iter_op {
            let dst = self.alloc_temp(ValueType::HeapObject);
            self.emit(Instruction::Iter {
                dst,
                value: Some(args[0]),
                ty: AbiType::from_checked_type(&ty),
                op,
            });
            return Ok(dst);
        }
        let contract = self
            .planner
            .catalog
            .trait_(&interface.declaration)
            .ok_or(IrLoweringError::MissingBinding("operator trait"))?;
        let signature = self
            .planner
            .catalog
            .trait_method(method)
            .ok_or(IrLoweringError::MissingBinding("operator signature"))?;
        let mut substitution: kagari_hir::types::TypeSubstitution = contract
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
            if declaration.module == *self.planner.owner().lowered.source.module_identity() {
                CallTarget::Function(self.planner.enqueue_declaration(
                    &declaration,
                    arguments,
                    self.function.debug.source_span,
                )?)
            } else {
                CallTarget::SourceFunction(Box::new(
                    crate::module::instruction::SourceFunctionContract {
                        declaration,
                        arguments,
                        params: args.iter().map(|a| a.ty).collect(),
                        return_type: result_ty,
                    },
                ))
            }
        } else {
            if StandardTrait::from_id(&interface.declaration) == Some(StandardTrait::Iterable) {
                return Ok(args[0]);
            }
            if let Some(protocol) = StandardTrait::from_id(&interface.declaration)
                && protocol.binary_operator()
            {
                let op = match protocol {
                    StandardTrait::Add => BinaryOp::Add,
                    StandardTrait::Sub => BinaryOp::Sub,
                    StandardTrait::Mul => BinaryOp::Mul,
                    StandardTrait::Div => BinaryOp::Div,
                    StandardTrait::Rem => BinaryOp::Rem,
                    _ => unreachable!(),
                };
                let dst = self.alloc_temp(result_ty);
                if protocol == StandardTrait::Rem
                    && let TypeId::Builtin(
                        input @ (kagari_hir::types::BuiltinType::I8
                        | kagari_hir::types::BuiltinType::I16),
                    ) = ty
                {
                    let operation = crate::module::numeric::NumericOperation::binary(
                        hir::BinaryOp::Rem,
                        input,
                        input,
                    )
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
            if let Some(protocol) = StandardTrait::from_id(&interface.declaration)
                && matches!(protocol, StandardTrait::Neg | StandardTrait::Not)
            {
                let dst = self.alloc_temp(result_ty);
                self.emit(Instruction::Unary {
                    dst,
                    op: if protocol == StandardTrait::Neg {
                        crate::module::instruction::UnaryOp::Neg
                    } else {
                        crate::module::instruction::UnaryOp::Not
                    },
                    operand: args[0],
                });
                self.check_integer_range(dst, &result);
                return Ok(dst);
            }
            if StandardTrait::from_id(&interface.declaration) == Some(StandardTrait::Index) {
                let dst = self.alloc_temp(result_ty);
                self.emit(Instruction::ReadAggregateIndex {
                    dst,
                    base: args[0],
                    index: args[1],
                });
                return Ok(dst);
            }
            let intrinsic = match StandardTrait::from_id(&interface.declaration) {
                Some(StandardTrait::PartialOrd) => StandardIntrinsic::ValuePartialCmp,
                Some(StandardTrait::Ord) => StandardIntrinsic::ValueCmp,
                _ => return Err(IrLoweringError::MissingBinding("builtin operator")),
            };
            CallTarget::StandardIntrinsic(intrinsic)
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
        site: hir::ExprId,
        op: hir::BinaryOp,
        args: &[IrValue],
    ) -> Result<IrValue, IrLoweringError> {
        // Primitive numeric comparisons retain their direct instruction path.
        if matches!(
            args[0].ty,
            ValueType::I32 | ValueType::I64 | ValueType::U64 | ValueType::F32 | ValueType::F64
        ) {
            let dst = self.alloc_temp(ValueType::Bool);
            let op = match op {
                hir::BinaryOp::Lt => BinaryOp::Lt,
                hir::BinaryOp::Le => BinaryOp::Le,
                hir::BinaryOp::Gt => BinaryOp::Gt,
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
        use crate::module::instruction::StandardEnumOp;
        let value = self.lower_selected_operator(site, args)?;
        let optional = kagari_hir::builtin::traits::ordering_type(true);
        let ordering = kagari_hir::builtin::traits::ordering_type(false);
        let some = self.standard_enum_op(&optional, StandardEnumOp::Test(0), Some(value))?;
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
        let order = self.standard_enum_op(&optional, StandardEnumOp::Read(0), Some(value))?;
        let tag = if matches!(op, hir::BinaryOp::Lt | hir::BinaryOp::Ge) {
            0
        } else {
            2
        };
        let test = self.standard_enum_op(&ordering, StandardEnumOp::Test(tag), Some(order))?;
        if matches!(op, hir::BinaryOp::Le | hir::BinaryOp::Ge) {
            self.emit(Instruction::Unary {
                dst,
                op: crate::module::instruction::UnaryOp::Not,
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
