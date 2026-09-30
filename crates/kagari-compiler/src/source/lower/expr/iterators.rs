use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::{
    operations::StandardEnumOp,
    representation::ValueType,
    standard::{StandardIntrinsic, surface::StandardEnum, traits::StandardTrait},
};
use kagari_common::{
    collection::CollectionAccess::{self, Mutable},
    identity::associated_type_id,
};
use kagari_hir::types::abi::lower_type;
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
            TypeId::Set(_, _) => {
                self.lower_key_storage_call(target, StandardIntrinsic::SetInsert, &[output, item])?;
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
                self.lower_key_storage_call(target, StandardIntrinsic::MapInsert, &[output, k, v])?;
            }
            _ => return Err(MirLoweringError::MissingBinding("collection insertion")),
        }
        Ok(())
    }
}
