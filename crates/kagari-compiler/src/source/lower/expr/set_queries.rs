use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::{
    operations::StandardEnumOp as Op,
    representation::ValueType,
    standard::{bindings::NativeDefaultMethod, surface::StandardEnum, traits::StandardTrait},
};
use kagari_common::collection::CollectionAccess;
use kagari_hir::{builtin::traits::StandardTraitSemantics, types::TypeId};
use kagari_mir::instruction::{Constant, Instruction, MirValue, Terminator};

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_set_query(
        &mut self,
        operation: NativeDefaultMethod,
        source: &TypeId,
        args: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let item = self.iteration_output(StandardTrait::Iterable, source, "Item")?;
        let mut interface = StandardTrait::Set.nominal();
        interface.arguments.push(item.clone());
        let other = TypeId::Trait(interface.clone());
        let membership = StandardTrait::Set
            .contract()
            .methods
            .iter()
            .find(|m| m.name == "contains")
            .unwrap()
            .id
            .clone();
        let left = self.query_guard(source, args[0])?;
        let right = self.query_guard(&other, args[1])?;
        let relation = matches!(
            operation,
            NativeDefaultMethod::SetIsSubset
                | NativeDefaultMethod::SetIsSuperset
                | NativeDefaultMethod::SetIsDisjoint
        );
        let result_type = TypeId::Set(Box::new(item.clone()), CollectionAccess::Mutable);
        let result = if relation {
            self.lower_constant(Constant::Bool(true), ValueType::Bool)
        } else {
            self.collection_new(&result_type)?
        };
        let optional = TypeId::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![item],
        };
        let passes = if matches!(
            operation,
            NativeDefaultMethod::SetUnion | NativeDefaultMethod::SetSymmetricDifference
        ) {
            2
        } else {
            1
        };
        for pass in 0..passes {
            let reversed = pass == 1 || operation == NativeDefaultMethod::SetIsSuperset;
            let (iterator, queried_type, queried_value) = if reversed {
                (&right, source, args[0])
            } else {
                (&left, &other, args[1])
            };
            let head = self.new_block();
            let body = self.new_block();
            let selected = self.new_block();
            let done = self.new_block();
            self.ensure_jump(head);
            self.switch_to_block(head);
            let next = self.iterator_next(&iterator.0, iterator.1)?;
            let present = self.standard_enum_op(&optional, Op::Test(0), Some(next))?;
            self.set_terminator(Terminator::Branch {
                cond: present,
                then_block: body,
                else_block: done,
            });
            self.switch_to_block(body);
            let value = self.standard_enum_op(&optional, Op::Read(0), Some(next))?;
            if operation == NativeDefaultMethod::SetUnion {
                self.ensure_jump(selected);
            } else {
                let contains = self.lower_applied_operator(
                    interface.clone(),
                    queried_type.clone(),
                    &membership,
                    &[queried_value, value],
                )?;
                let positive = matches!(
                    operation,
                    NativeDefaultMethod::SetIntersection | NativeDefaultMethod::SetIsDisjoint
                );
                self.set_terminator(Terminator::Branch {
                    cond: contains,
                    then_block: if positive { selected } else { head },
                    else_block: if positive { head } else { selected },
                });
            }
            self.switch_to_block(selected);
            if relation {
                let no = self.lower_constant(Constant::Bool(false), ValueType::Bool);
                self.emit(Instruction::Move {
                    dst: result,
                    src: no,
                });
                self.ensure_jump(done);
            } else {
                self.collection_insert(&result_type, result, value)?;
                self.ensure_jump(head);
            }
            self.switch_to_block(done);
        }
        self.end_query_guard(right);
        self.end_query_guard(left);
        Ok(result)
    }
}
