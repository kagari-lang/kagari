use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::{
    operations::StandardEnumOp,
    representation::ValueType,
    scalar::BuiltinType,
    standard::{
        StandardIntrinsic, bindings::NativeDefaultMethod, surface::StandardEnum,
        traits::StandardTrait,
    },
};
use kagari_common::collection::CollectionAccess::Mutable;
use kagari_hir::{builtin::traits::StandardTraitSemantics, types::TypeId};
use kagari_mir::instruction::{Instruction, MirValue, Terminator};
use std::slice;

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_iterator_terminal(
        &mut self,
        operation: NativeDefaultMethod,
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
        let result = match operation {
            NativeDefaultMethod::Partition => self.collection_new(&array_type)?,
            NativeDefaultMethod::GroupBy => self.collection_new(&TypeId::Map {
                key: Box::new(arguments[0].clone()),
                value: Box::new(array_type.clone()),
                access: Mutable,
            })?,
            _ => return Err(MirLoweringError::MissingBinding("iterator terminal")),
        };
        let rejected = if operation == NativeDefaultMethod::Partition {
            Some(self.collection_new(&array_type)?)
        } else {
            None
        };
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
        match operation {
            NativeDefaultMethod::Partition => {
                let predicate = self.call_function_value(values[1], &bool_type, &[item])?;
                let yes = self.new_block();
                let no = self.new_block();
                self.set_terminator(Terminator::Branch {
                    cond: predicate,
                    then_block: yes,
                    else_block: no,
                });
                self.switch_to_block(yes);
                self.collection_insert(&array_type, result, item)?;
                self.ensure_jump(head);
                self.switch_to_block(no);
                self.collection_insert(&array_type, rejected.unwrap(), item)?;
                self.ensure_jump(head);
            }
            NativeDefaultMethod::GroupBy => {
                let key_type = &arguments[0];
                let result_type = TypeId::Map {
                    key: Box::new(key_type.clone()),
                    value: Box::new(array_type.clone()),
                    access: Mutable,
                };
                let key = self.call_function_value(values[1], key_type, &[item])?;
                let group = self.lower_key_storage_call(
                    &result_type,
                    StandardIntrinsic::MapGet,
                    &[result, key],
                )?;
                let group_option = TypeId::StandardEnum {
                    kind: StandardEnum::Option,
                    args: vec![array_type.clone()],
                };
                let present =
                    self.standard_enum_op(&group_option, StandardEnumOp::Test(0), Some(group))?;
                let existing = self.new_block();
                let fresh = self.new_block();
                self.set_terminator(Terminator::Branch {
                    cond: present,
                    then_block: existing,
                    else_block: fresh,
                });
                self.switch_to_block(existing);
                let group =
                    self.standard_enum_op(&group_option, StandardEnumOp::Read(0), Some(group))?;
                self.collection_insert(&array_type, group, item)?;
                self.ensure_jump(head);
                self.switch_to_block(fresh);
                let group = self.collection_new(&array_type)?;
                self.collection_insert(&array_type, group, item)?;
                self.lower_key_storage_call(
                    &result_type,
                    StandardIntrinsic::MapInsert,
                    &[result, key, group],
                )?;
                self.ensure_jump(head);
            }
            _ => unreachable!(),
        }
        self.switch_to_block(done);
        if guarded {
            self.iterator_close(source, values[0]);
            self.emit(Instruction::EndIteration);
        }
        if let Some(rejected) = rejected {
            let mut contract = StandardTrait::FromIterator.nominal();
            contract.arguments.push(item_type);
            let method = self.protocol_method(StandardTrait::FromIterator, 0)?;
            let left = self.lower_applied_method(
                contract.clone(),
                arguments[0].clone(),
                &method,
                slice::from_ref(&array_type),
                &[result],
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
        } else {
            Ok(result)
        }
    }
}
