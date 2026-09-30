use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::{
    operations::StandardEnumOp,
    representation::ValueType,
    scalar::BuiltinType,
    standard::{surface::StandardEnum, traits::StandardTrait},
};
use kagari_common::collection::CollectionAccess::Mutable;
use kagari_hir::{builtin::traits::StandardTraitSemantics, types::TypeId};
use kagari_mir::instruction::{Instruction, MirValue, Terminator};
use std::slice;

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_iterator_partition(
        &mut self,
        source: &TypeId,
        arguments: &[TypeId],
        values: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let item_type = self.iterator_item(source)?;
        let optional = TypeId::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![item_type.clone()],
        };
        let array_type = TypeId::Array(Box::new(item_type.clone()), Mutable);
        let bool_type = TypeId::Builtin(BuiltinType::Bool);
        let accepted = self.collection_new(&array_type)?;
        let rejected = self.collection_new(&array_type)?;
        let guarded = matches!(source, TypeId::Iter(_));
        if guarded {
            self.emit(Instruction::BeginIteration {
                collection: values[0],
            });
        }
        let head = self.new_block();
        let body = self.new_block();
        let done = self.new_block();
        self.ensure_jump(head);
        self.switch_to_block(head);
        let next = self.iterator_next(source, values[0])?;
        let present = self.standard_enum_op(&optional, StandardEnumOp::Test(0), Some(next))?;
        self.set_terminator(Terminator::Branch {
            cond: present,
            then_block: body,
            else_block: done,
        });
        self.switch_to_block(body);
        let item = self.standard_enum_op(&optional, StandardEnumOp::Read(0), Some(next))?;
        let predicate = self.call_function_value(values[1], &bool_type, &[item])?;
        let yes = self.new_block();
        let no = self.new_block();
        self.set_terminator(Terminator::Branch {
            cond: predicate,
            then_block: yes,
            else_block: no,
        });
        self.switch_to_block(yes);
        self.collection_insert(&array_type, accepted, item)?;
        self.ensure_jump(head);
        self.switch_to_block(no);
        self.collection_insert(&array_type, rejected, item)?;
        self.ensure_jump(head);
        self.switch_to_block(done);
        if guarded {
            self.iterator_close(source, values[0]);
            self.emit(Instruction::EndIteration);
        }
        let mut contract = StandardTrait::FromIterator.nominal();
        contract.arguments.push(item_type);
        let method = self.protocol_method(StandardTrait::FromIterator, 0)?;
        let left = self.lower_applied_method(
            contract.clone(),
            arguments[0].clone(),
            &method,
            slice::from_ref(&array_type),
            &[accepted],
        )?;
        let right = self.lower_applied_method(
            contract,
            arguments[0].clone(),
            &method,
            &[array_type],
            &[rejected],
        )?;
        let pair = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::MakeTuple {
            dst: pair,
            elements: vec![left, right].into(),
        });
        Ok(pair)
    }
}
