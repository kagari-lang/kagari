use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::{
    representation::ValueType,
    standard::{StandardIntrinsic, traits::StandardTrait},
};
use kagari_common::{collection::CollectionAccess::Mutable, identity::associated_type_id};
use kagari_hir::types::abi::lower_type;
use kagari_hir::{
    builtin::traits::{self, StandardTraitSemantics},
    types::TypeId,
};
use kagari_mir::instruction::{Constant, Instruction, MirValue};

impl FunctionLowerer<'_, '_> {
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
