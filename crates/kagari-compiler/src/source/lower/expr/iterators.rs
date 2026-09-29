use crate::source::{
    lower::{MirLoweringError, state::FunctionLowerer},
    types::lower_type,
};
use kagari_abi::{
    operations::{BinaryOp, IterOp, StandardEnumOp},
    representation::ValueType,
    standard::{StandardIntrinsic, surface::StandardEnum, traits::StandardTrait},
};
use kagari_common::{
    collection::CollectionAccess::{self, Mutable},
    identity::associated_type_id,
};
use kagari_hir::{
    builtin::traits::{self, StandardTraitSemantics},
    types::TypeId,
};
use kagari_mir::instruction::{Constant, Instruction, MirValue, Terminator};

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_fallible_collect(
        &mut self,
        target: &TypeId,
        source: &TypeId,
        value: MirValue,
    ) -> Result<MirValue, MirLoweringError> {
        let TypeId::StandardEnum { kind, args } = target else {
            unreachable!()
        };
        let destination = &args[0];
        let iterator_type = self.iteration_output(StandardTrait::Iterable, source, "Iter")?;
        let iterator = self.lower_applied_operator(
            StandardTrait::Iterable.nominal(),
            source.clone(),
            &self.protocol_method(StandardTrait::Iterable, 0)?,
            &[value],
        )?;
        let input = self.iterator_item(&iterator_type)?;
        let TypeId::StandardEnum {
            args: input_args, ..
        } = &input
        else {
            unreachable!()
        };
        let element = input_args[0].clone();
        let buffer_type = TypeId::Array(Box::new(element.clone()), CollectionAccess::Mutable);
        let buffer = self.collection_new(&buffer_type)?;
        let optional = TypeId::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![input.clone()],
        };
        let result = self.alloc_temp(ValueType::HeapObject);
        self.function
            .semantic
            .registers
            .insert(result.temp.index(), lower_type(target));
        let guarded = matches!(iterator_type, TypeId::Iter(_));
        if guarded {
            self.emit(Instruction::BeginIteration {
                collection: iterator,
            });
        }
        let head = self.new_block();
        let body = self.new_block();
        let success = self.new_block();
        let failure = self.new_block();
        let done = self.new_block();
        self.ensure_jump(head);
        self.switch_to_block(head);
        let next = self.iterator_next(&iterator_type, iterator)?;
        let present = self.standard_enum_op(&optional, StandardEnumOp::Test(0), Some(next))?;
        self.set_terminator(Terminator::Branch {
            cond: present,
            then_block: body,
            else_block: success,
        });
        self.switch_to_block(body);
        let item = self.standard_enum_op(&optional, StandardEnumOp::Read(0), Some(next))?;
        let valid = self.standard_enum_op(&input, StandardEnumOp::Test(0), Some(item))?;
        let append = self.new_block();
        self.set_terminator(Terminator::Branch {
            cond: valid,
            then_block: append,
            else_block: failure,
        });
        self.switch_to_block(append);
        let payload = self.standard_enum_op(&input, StandardEnumOp::Read(0), Some(item))?;
        self.collection_insert(&buffer_type, buffer, payload)?;
        self.ensure_jump(head);
        self.switch_to_block(failure);
        let failed = if *kind == StandardEnum::Result {
            let error = self.standard_enum_op(&input, StandardEnumOp::Read(1), Some(item))?;
            let failed = self.alloc_temp(ValueType::HeapObject);
            self.emit(Instruction::MapResultError {
                dst: failed,
                original: item,
                error,
                ty: lower_type(target),
            });
            failed
        } else {
            self.standard_enum_op(target, StandardEnumOp::Make(1), None)?
        };
        self.emit(Instruction::Move {
            dst: result,
            src: failed,
        });
        if guarded {
            self.iterator_close(&iterator_type, iterator);
            self.emit(Instruction::EndIteration);
        }
        self.ensure_jump(done);
        self.switch_to_block(success);
        if guarded {
            self.iterator_close(&iterator_type, iterator);
            self.emit(Instruction::EndIteration);
        }
        let mut contract = StandardTrait::FromIterator.nominal();
        contract.arguments.push(element);
        let collection = self.lower_applied_method(
            contract,
            destination.clone(),
            &self.protocol_method(StandardTrait::FromIterator, 0)?,
            &[buffer_type],
            &[buffer],
        )?;
        let succeeded = self.standard_enum_op(target, StandardEnumOp::Make(0), Some(collection))?;
        self.emit(Instruction::Move {
            dst: result,
            src: succeeded,
        });
        self.ensure_jump(done);
        self.switch_to_block(done);
        Ok(result)
    }
    pub(super) fn lower_numeric_aggregate(
        &mut self,
        protocol: StandardTrait,
        target: &TypeId,
        source: &TypeId,
        value: MirValue,
    ) -> Result<MirValue, MirLoweringError> {
        let one = protocol == StandardTrait::Product;
        let representation = self.value_type(target)?;
        let identity = match representation {
            ValueType::I32 => Constant::I32(i32::from(one)),
            ValueType::I64 => Constant::I64(i64::from(one)),
            ValueType::U64 => Constant::U64(u64::from(one)),
            ValueType::F32 => Constant::F32(if one { 1.0 } else { 0.0 }),
            ValueType::F64 => Constant::F64(if one { 1.0 } else { 0.0 }),
            _ => return Err(MirLoweringError::MissingBinding("numeric aggregate")),
        };
        let output = self.lower_constant(identity, representation);
        self.function
            .semantic
            .registers
            .insert(output.temp.index(), lower_type(target));
        let iterator_type = self.iteration_output(StandardTrait::Iterable, source, "Iter")?;
        let iterator = self.lower_applied_operator(
            StandardTrait::Iterable.nominal(),
            source.clone(),
            &self.protocol_method(StandardTrait::Iterable, 0)?,
            &[value],
        )?;
        let optional = TypeId::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![target.clone()],
        };
        let guarded = matches!(iterator_type, TypeId::Iter(_));
        if guarded {
            self.emit(Instruction::BeginIteration {
                collection: iterator,
            });
        }
        let head = self.new_block();
        let body = self.new_block();
        let done = self.new_block();
        self.ensure_jump(head);
        self.switch_to_block(head);
        let next = self.iterator_next(&iterator_type, iterator)?;
        let present = self.standard_enum_op(&optional, StandardEnumOp::Test(0), Some(next))?;
        self.set_terminator(Terminator::Branch {
            cond: present,
            then_block: body,
            else_block: done,
        });
        self.switch_to_block(body);
        let item = self.standard_enum_op(&optional, StandardEnumOp::Read(0), Some(next))?;
        let next = self.alloc_temp(representation);
        self.function
            .semantic
            .registers
            .insert(next.temp.index(), lower_type(target));
        self.emit(Instruction::Binary {
            dst: next,
            op: if one { BinaryOp::Mul } else { BinaryOp::Add },
            lhs: output,
            rhs: item,
        });
        self.check_integer_range(next, target);
        self.emit(Instruction::Move {
            dst: output,
            src: next,
        });
        self.ensure_jump(head);
        self.switch_to_block(done);
        if guarded {
            self.iterator_close(&iterator_type, iterator);
            self.emit(Instruction::EndIteration);
        }
        Ok(output)
    }
    pub(super) fn iteration_output(
        &self,
        protocol: StandardTrait,
        receiver: &TypeId,
        name: &str,
    ) -> Result<TypeId, MirLoweringError> {
        let interface = protocol.nominal();
        let member = associated_type_id(&interface.declaration, name);
        if let Some(output) = traits::iteration_outputs(
            protocol,
            receiver,
            Some(self.planner.catalog),
            &Default::default(),
        )
        .and_then(|outputs| outputs.get(&member).cloned())
        {
            return Ok(output);
        }
        let output = self.planner.catalog.normalize_type(&TypeId::Projection {
            receiver: Box::new(receiver.clone()),
            interface: Box::new(interface),
            member,
            arguments: vec![],
        });
        if !output.is_concrete() {
            return Err(MirLoweringError::MissingBinding(
                "concrete iteration output",
            ));
        }
        Ok(output)
    }

    pub(super) fn iterator_item(&self, receiver: &TypeId) -> Result<TypeId, MirLoweringError> {
        self.iteration_output(StandardTrait::Iterator, receiver, "Item")
    }

    pub(super) fn collection_new(&mut self, target: &TypeId) -> Result<MirValue, MirLoweringError> {
        let (new, mutable) = match target {
            TypeId::Array(item, _) => (
                StandardIntrinsic::ArrayListNew,
                TypeId::Array(item.clone(), Mutable),
            ),
            TypeId::Set(item, _) => (
                StandardIntrinsic::LinkedHashSetNew,
                TypeId::Set(item.clone(), Mutable),
            ),
            TypeId::Map { key, value, .. } => (
                StandardIntrinsic::LinkedHashMapNew,
                TypeId::Map {
                    key: key.clone(),
                    value: value.clone(),
                    access: Mutable,
                },
            ),
            _ => return Err(MirLoweringError::MissingBinding("collection destination")),
        };
        let output = self.emit_intrinsic(new, &[], ValueType::HeapObject);
        self.function
            .semantic
            .registers
            .insert(output.temp.index(), lower_type(&mutable));
        Ok(output)
    }

    pub(super) fn collection_insert(
        &mut self,
        target: &TypeId,
        output: MirValue,
        item: MirValue,
    ) -> Result<(), MirLoweringError> {
        match target {
            TypeId::Array(_, _) => {
                self.emit_intrinsic(
                    StandardIntrinsic::ArrayPush,
                    &[output, item],
                    ValueType::HeapObject,
                );
            }
            TypeId::Set(key, _) => {
                if self.has_custom_protocol(key)? {
                    self.lower_key_operation(StandardIntrinsic::SetInsert, key, &[output, item])?;
                } else {
                    self.emit_intrinsic(
                        StandardIntrinsic::SetInsert,
                        &[output, item],
                        ValueType::HeapObject,
                    );
                }
            }
            TypeId::Map { key, value, .. } => {
                let first = self.lower_constant(Constant::I32(0), ValueType::I32);
                let second = self.lower_constant(Constant::I32(1), ValueType::I32);
                let k = self.alloc_temp(self.value_type(key)?);
                let v = self.alloc_temp(self.value_type(value)?);
                self.emit(Instruction::ReadAggregateIndex {
                    dst: k,
                    base: item,
                    index: first,
                });
                self.emit(Instruction::ReadAggregateIndex {
                    dst: v,
                    base: item,
                    index: second,
                });
                if self.has_custom_protocol(key)? {
                    self.lower_key_operation(StandardIntrinsic::MapInsert, key, &[output, k, v])?;
                } else {
                    self.emit_intrinsic(
                        StandardIntrinsic::MapInsert,
                        &[output, k, v],
                        ValueType::HeapObject,
                    );
                }
            }
            _ => return Err(MirLoweringError::MissingBinding("collection insertion")),
        }
        Ok(())
    }

    pub(super) fn lower_collect(
        &mut self,
        target: &TypeId,
        source: &TypeId,
        value: MirValue,
    ) -> Result<MirValue, MirLoweringError> {
        let iterator_type = self.iteration_output(StandardTrait::Iterable, source, "Iter")?;
        let iterator = self.lower_applied_operator(
            StandardTrait::Iterable.nominal(),
            source.clone(),
            &self.protocol_method(StandardTrait::Iterable, 0)?,
            &[value],
        )?;
        let item_type = self.iterator_item(&iterator_type)?;
        let output = self.collection_new(target)?;
        let guarded = matches!(iterator_type, TypeId::Iter(_));
        if guarded {
            self.emit(Instruction::BeginIteration {
                collection: iterator,
            });
        }
        let head = self.new_block();
        let body = self.new_block();
        let done = self.new_block();
        self.ensure_jump(head);
        self.switch_to_block(head);
        let next = self.lower_applied_operator(
            StandardTrait::Iterator.nominal(),
            iterator_type.clone(),
            &self.protocol_method(StandardTrait::Iterator, 0)?,
            &[iterator],
        )?;
        let optional = TypeId::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![item_type],
        };
        let present = self.standard_enum_op(&optional, StandardEnumOp::Test(0), Some(next))?;
        self.set_terminator(Terminator::Branch {
            cond: present,
            then_block: body,
            else_block: done,
        });
        self.switch_to_block(body);
        let item = self.standard_enum_op(&optional, StandardEnumOp::Read(0), Some(next))?;
        self.collection_insert(target, output, item)?;
        self.ensure_jump(head);
        self.switch_to_block(done);
        if guarded {
            let dst = self.alloc_temp(ValueType::Unit);
            self.emit(Instruction::Iter {
                dst,
                value: Some(iterator),
                ty: lower_type(&iterator_type),
                op: IterOp::Close,
            });
            self.emit(Instruction::EndIteration);
        }
        let destination = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::Move {
            dst: destination,
            src: output,
        });
        self.function
            .semantic
            .registers
            .insert(destination.temp.index(), lower_type(target));
        Ok(destination)
    }
}
