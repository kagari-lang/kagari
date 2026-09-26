use super::*;
use kagari_hir::{builtin::traits::StandardTrait, types::TypeId};

impl FunctionLowerer<'_, '_> {
    pub(super) fn has_custom_protocol(&self, ty: &TypeId) -> bool {
        let mut pending = vec![ty.clone()];
        let mut seen = std::collections::HashSet::new();
        while let Some(ty) = pending.pop() {
            if !seen.insert(ty.clone()) {
                continue;
            }
            if seen.len() > self.planner.options.max_type_nodes {
                return true;
            }
            match &ty {
                TypeId::Struct(_) | TypeId::Enum(_) => {
                    if self
                        .planner
                        .catalog
                        .implementation_method(
                            &StandardTrait::PartialEq.contract().methods[0].id,
                            &StandardTrait::PartialEq.nominal(),
                            &ty,
                        )
                        .is_some()
                    {
                        return true;
                    }
                    if let TypeId::Enum(n) = &ty
                        && let Some(contract) = self.planner.catalog.enumeration(&n.declaration)
                    {
                        let substitution = contract
                            .generic_params
                            .iter()
                            .cloned()
                            .zip(n.arguments.iter().cloned())
                            .collect();
                        pending.extend(
                            contract.variants.iter().flat_map(|v| {
                                v.payload.iter().map(|t| t.instantiate(&substitution))
                            }),
                        );
                    }
                }
                TypeId::Tuple(members) | TypeId::StandardEnum { args: members, .. } => {
                    pending.extend(members.iter().cloned())
                }
                _ => {}
            }
        }
        false
    }

    pub(super) fn emit_intrinsic(
        &mut self,
        intrinsic: StandardIntrinsic,
        args: &[IrValue],
        ty: ValueType,
    ) -> IrValue {
        let dst = self.alloc_temp(ty);
        self.emit(Instruction::Call {
            dst: Some(dst),
            callee: CallTarget::StandardIntrinsic(intrinsic),
            args: args.iter().copied().collect(),
        });
        dst
    }

    pub(crate) fn lower_protocol(
        &mut self,
        protocol: StandardTrait,
        ty: &TypeId,
        args: &[IrValue],
        depth: usize,
    ) -> Result<IrValue, IrLoweringError> {
        if matches!(
            ty,
            TypeId::Tuple(_) | TypeId::Enum(_) | TypeId::StandardEnum { .. }
        ) && self.has_custom_protocol(ty)
            && self
                .planner
                .catalog
                .implementation_method(&protocol.contract().methods[0].id, &protocol.nominal(), ty)
                .is_none()
        {
            let function = self.planner.enqueue_protocol(
                &self.instance,
                protocol,
                ty,
                self.function.debug.source_span,
            )?;
            let dst = self.alloc_temp(if protocol == StandardTrait::PartialEq {
                ValueType::Bool
            } else {
                ValueType::I64
            });
            self.emit(Instruction::Call {
                dst: Some(dst),
                callee: CallTarget::Function(function),
                args: args.iter().copied().collect(),
            });
            Ok(dst)
        } else {
            self.lower_protocol_body(protocol, ty, args, depth)
        }
    }

    pub(crate) fn lower_protocol_body(
        &mut self,
        protocol: StandardTrait,
        ty: &TypeId,
        args: &[IrValue],
        depth: usize,
    ) -> Result<IrValue, IrLoweringError> {
        self.planner.check()?;
        if depth > self.planner.options.max_type_depth {
            return Err(IrLoweringError::UnsupportedExpr(
                "protocol composition exceeds type depth limit",
            ));
        }
        let result_ty = if protocol == StandardTrait::PartialEq {
            ValueType::Bool
        } else {
            ValueType::I64
        };
        if !self.has_custom_protocol(ty) {
            if protocol == StandardTrait::PartialEq {
                let dst = self.alloc_temp(ValueType::Bool);
                self.emit(Instruction::Binary {
                    dst,
                    op: BinaryOp::Eq,
                    lhs: args[0],
                    rhs: args[1],
                });
                return Ok(dst);
            }
            return Ok(self.emit_intrinsic(StandardIntrinsic::ValueHash, args, result_ty));
        }
        let method = &protocol.contract().methods[0].id;
        if let Some((declaration, arguments)) =
            self.planner
                .catalog
                .implementation_method(method, &protocol.nominal(), ty)
        {
            let callee =
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
                            params: args.iter().map(|arg| arg.ty).collect(),
                            return_type: result_ty,
                        },
                    ))
                };
            let dst = self.alloc_temp(result_ty);
            self.emit(Instruction::Call {
                dst: Some(dst),
                callee,
                args: args.iter().copied().collect(),
            });
            return Ok(dst);
        }
        match ty {
            TypeId::Tuple(members) => {
                let mut inputs = Vec::new();
                for (index, member) in members.iter().enumerate() {
                    let index = self.lower_constant(Constant::I32(index as i32), ValueType::I32);
                    let mut values = Vec::new();
                    for base in args {
                        let dst = self.alloc_temp(self.value_type(member)?);
                        self.emit(Instruction::ReadAggregateIndex {
                            dst,
                            base: *base,
                            index,
                        });
                        values.push(dst);
                    }
                    inputs.push((member.clone(), values));
                }
                self.combine_protocol(protocol, &inputs, None, depth)
            }
            TypeId::Enum(nominal) => {
                let contract = self
                    .planner
                    .catalog
                    .enumeration(&nominal.declaration)
                    .ok_or(IrLoweringError::MissingBinding("enum protocol contract"))?
                    .clone();
                let substitution = contract
                    .generic_params
                    .iter()
                    .cloned()
                    .zip(nominal.arguments.iter().cloned())
                    .collect();
                let variants = contract
                    .variants
                    .iter()
                    .map(|v| {
                        (
                            v.slot,
                            v.payload
                                .iter()
                                .map(|ty| ty.instantiate(&substitution))
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>();
                self.enum_protocol(protocol, ty, args, &variants, depth)
            }
            TypeId::StandardEnum {
                kind,
                args: members,
            } => {
                let variants = match kind {
                    kagari_hir::builtin::surface::StandardEnum::Option => {
                        vec![(0, vec![members[0].clone()]), (1, vec![])]
                    }
                    kagari_hir::builtin::surface::StandardEnum::Result => {
                        vec![(0, vec![members[0].clone()]), (1, vec![members[1].clone()])]
                    }
                };
                self.enum_protocol(protocol, ty, args, &variants, depth)
            }
            _ => {
                let dst = self.alloc_temp(result_ty);
                let intrinsic = if protocol == StandardTrait::PartialEq {
                    StandardIntrinsic::ValueEq
                } else {
                    StandardIntrinsic::ValueHash
                };
                self.emit(Instruction::Call {
                    dst: Some(dst),
                    callee: CallTarget::StandardIntrinsic(intrinsic),
                    args: args.iter().copied().collect(),
                });
                Ok(dst)
            }
        }
    }

    fn combine_protocol(
        &mut self,
        protocol: StandardTrait,
        inputs: &[(TypeId, Vec<IrValue>)],
        tag: Option<Constant>,
        depth: usize,
    ) -> Result<IrValue, IrLoweringError> {
        if protocol == StandardTrait::Hash {
            let mut hashes = ValueBuffer::new();
            if let Some(tag) = tag {
                let ty = if matches!(tag, Constant::Str(_)) {
                    ValueType::Str
                } else {
                    ValueType::I64
                };
                hashes.push(self.lower_constant(tag, ty));
            }
            for (ty, args) in inputs {
                hashes.push(self.lower_protocol(protocol, ty, args, depth + 1)?);
            }
            let tuple = self.alloc_temp(ValueType::HeapObject);
            self.emit(Instruction::MakeTuple {
                dst: tuple,
                elements: hashes,
            });
            let dst = self.alloc_temp(ValueType::I64);
            self.emit(Instruction::Call {
                dst: Some(dst),
                callee: CallTarget::StandardIntrinsic(StandardIntrinsic::ValueHash),
                args: [tuple].into_iter().collect(),
            });
            return Ok(dst);
        }
        let dst = self.alloc_temp(ValueType::Bool);
        let fail = self.new_block();
        let join = self.new_block();
        for (ty, args) in inputs {
            let cond = self.lower_protocol(protocol, ty, args, depth + 1)?;
            let next = self.new_block();
            self.set_terminator(Terminator::Branch {
                cond,
                then_block: next,
                else_block: fail,
            });
            self.switch_to_block(next);
        }
        let yes = self.lower_constant(Constant::Bool(true), ValueType::Bool);
        self.emit(Instruction::Move { dst, src: yes });
        self.set_terminator(Terminator::Jump(join));
        self.switch_to_block(fail);
        let no = self.lower_constant(Constant::Bool(false), ValueType::Bool);
        self.emit(Instruction::Move { dst, src: no });
        self.set_terminator(Terminator::Jump(join));
        self.switch_to_block(join);
        Ok(dst)
    }

    fn enum_protocol(
        &mut self,
        protocol: StandardTrait,
        ty: &TypeId,
        args: &[IrValue],
        variants: &[(usize, Vec<TypeId>)],
        depth: usize,
    ) -> Result<IrValue, IrLoweringError> {
        let dst = self.alloc_temp(if protocol == StandardTrait::PartialEq {
            ValueType::Bool
        } else {
            ValueType::I64
        });
        let join = self.new_block();
        let fail = self.new_block();
        for (variant, members) in variants {
            let matched = self.new_block();
            let next = self.new_block();
            let cond = self.enum_test(ty, args[0], *variant)?;
            self.set_terminator(Terminator::Branch {
                cond,
                then_block: matched,
                else_block: next,
            });
            self.switch_to_block(matched);
            if protocol == StandardTrait::PartialEq {
                let cond = self.enum_test(ty, args[1], *variant)?;
                let payload = self.new_block();
                self.set_terminator(Terminator::Branch {
                    cond,
                    then_block: payload,
                    else_block: fail,
                });
                self.switch_to_block(payload);
            }
            let mut inputs = Vec::new();
            for (index, member) in members.iter().enumerate() {
                let values = args
                    .iter()
                    .map(|arg| self.enum_member(ty, *arg, *variant, index, member))
                    .collect::<Result<Vec<_>, _>>()?;
                inputs.push((member.clone(), values));
            }
            // Declaration identity, not the current slot, survives variant reordering.
            let tag = if let TypeId::Enum(nominal) = ty {
                use bincode::Options;
                use std::fmt::Write;
                let declaration = &self
                    .planner
                    .catalog
                    .enumeration(&nominal.declaration)
                    .ok_or(IrLoweringError::MissingBinding("enum hash identity"))?
                    .variants[*variant]
                    .id;
                let bytes = bincode::DefaultOptions::new()
                    .with_fixint_encoding()
                    .with_little_endian()
                    .serialize(&(
                        crate::module::abi::NominalAbiType::from_checked_type(nominal),
                        declaration,
                    ))
                    .expect("validated enum identity");
                let mut encoded = String::new();
                for byte in bytes {
                    write!(encoded, "{byte:02x}").expect("String write");
                }
                Constant::Str(encoded)
            } else {
                Constant::I64(*variant as i64)
            };
            let result = self.combine_protocol(protocol, &inputs, Some(tag), depth)?;
            self.emit(Instruction::Move { dst, src: result });
            self.set_terminator(Terminator::Jump(join));
            self.switch_to_block(next);
        }
        self.set_terminator(Terminator::Jump(fail));
        self.switch_to_block(fail);
        let zero = if protocol == StandardTrait::PartialEq {
            self.lower_constant(Constant::Bool(false), ValueType::Bool)
        } else {
            self.lower_constant(Constant::I64(0), ValueType::I64)
        };
        self.emit(Instruction::Move { dst, src: zero });
        self.set_terminator(Terminator::Jump(join));
        self.switch_to_block(join);
        Ok(dst)
    }

    fn enum_test(
        &mut self,
        ty: &TypeId,
        value: IrValue,
        variant: usize,
    ) -> Result<IrValue, IrLoweringError> {
        if let TypeId::Enum(nominal) = ty {
            let dst = self.alloc_temp(ValueType::Bool);
            self.emit(Instruction::TestEnumVariant {
                dst,
                value,
                enumeration: crate::module::abi::NominalAbiType::from_checked_type(nominal),
                variant,
            });
            Ok(dst)
        } else {
            self.standard_enum_op(
                ty,
                crate::module::instruction::StandardEnumOp::Test(variant as u32),
                Some(value),
            )
        }
    }

    fn enum_member(
        &mut self,
        ty: &TypeId,
        value: IrValue,
        variant: usize,
        index: usize,
        member: &TypeId,
    ) -> Result<IrValue, IrLoweringError> {
        if let TypeId::Enum(nominal) = ty {
            let dst = self.alloc_temp(self.value_type(member)?);
            self.emit(Instruction::ReadEnumPayload {
                dst,
                value,
                enumeration: crate::module::abi::NominalAbiType::from_checked_type(nominal),
                variant,
                index,
            });
            Ok(dst)
        } else {
            self.standard_enum_op(
                ty,
                crate::module::instruction::StandardEnumOp::Read(variant as u32),
                Some(value),
            )
        }
    }
}
