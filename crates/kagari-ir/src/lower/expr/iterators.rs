use super::*;
use crate::module::{
    abi::AbiType,
    instruction::{CursorOp, StandardEnumOp},
};
use kagari_hir::{
    builtin::{
        surface::StandardEnum,
        traits::{self, StandardTrait},
    },
    types::{TypeId, associated_type_id},
};

impl FunctionLowerer<'_, '_> {
    pub(super) fn iteration_output(
        &self,
        protocol: StandardTrait,
        receiver: &TypeId,
        name: &str,
    ) -> Result<TypeId, IrLoweringError> {
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
            return Err(IrLoweringError::MissingBinding("concrete iteration output"));
        }
        Ok(output)
    }

    pub(super) fn iterator_item(&self, receiver: &TypeId) -> Result<TypeId, IrLoweringError> {
        self.iteration_output(StandardTrait::Iterator, receiver, "Item")
    }

    pub(super) fn collection_new(&mut self, target: &TypeId) -> Result<IrValue, IrLoweringError> {
        use kagari_common::collection::CollectionAccess::Mutable;
        let (new, mutable) = match target {
            TypeId::Array(item, _) => (
                StandardIntrinsic::MutableArrayNew,
                TypeId::Array(item.clone(), Mutable),
            ),
            TypeId::Set(item, _) => (
                StandardIntrinsic::MutableSetNew,
                TypeId::Set(item.clone(), Mutable),
            ),
            TypeId::Map { key, value, .. } => (
                StandardIntrinsic::MutableMapNew,
                TypeId::Map {
                    key: key.clone(),
                    value: value.clone(),
                    access: Mutable,
                },
            ),
            _ => return Err(IrLoweringError::MissingBinding("collection destination")),
        };
        let output = self.emit_intrinsic(new, &[], ValueType::HeapObject);
        self.function
            .semantic
            .registers
            .insert(output.temp.index(), AbiType::from_checked_type(&mutable));
        Ok(output)
    }

    pub(super) fn collection_insert(
        &mut self,
        target: &TypeId,
        output: IrValue,
        item: IrValue,
    ) -> Result<(), IrLoweringError> {
        use StandardIntrinsic::*;
        match target {
            TypeId::Array(_, _) => {
                self.emit_intrinsic(ArrayPush, &[output, item], ValueType::HeapObject);
            }
            TypeId::Set(key, _) => {
                if self.has_custom_protocol(key) {
                    self.lower_key_operation(SetInsert, key, &[output, item])?;
                } else {
                    self.emit_intrinsic(SetInsert, &[output, item], ValueType::HeapObject);
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
                if self.has_custom_protocol(key) {
                    self.lower_key_operation(MapInsert, key, &[output, k, v])?;
                } else {
                    self.emit_intrinsic(MapInsert, &[output, k, v], ValueType::HeapObject);
                }
            }
            _ => return Err(IrLoweringError::MissingBinding("collection insertion")),
        }
        Ok(())
    }

    pub(super) fn lower_collect(
        &mut self,
        target: &TypeId,
        source: &TypeId,
        value: IrValue,
    ) -> Result<IrValue, IrLoweringError> {
        let iterator_type = self.iteration_output(StandardTrait::Iterable, source, "Iter")?;
        let iterator = self.lower_applied_operator(
            StandardTrait::Iterable.nominal(),
            source.clone(),
            &StandardTrait::Iterable.contract().methods[0].id,
            &[value],
        )?;
        let item_type = self.iterator_item(&iterator_type)?;
        let output = self.collection_new(target)?;
        let guarded = matches!(iterator_type, TypeId::Cursor(_));
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
            &StandardTrait::Iterator.contract().methods[0].id,
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
            self.emit(Instruction::Cursor {
                dst,
                value: Some(iterator),
                ty: AbiType::from_checked_type(&iterator_type),
                op: CursorOp::Close,
            });
            self.emit(Instruction::EndIteration);
        }
        Ok(output)
    }
}
