use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::{
    operations::{BinaryOp, StandardEnumOp as Op},
    representation::ValueType,
    standard::{bindings::NativeDefaultMethod, surface::StandardEnum, traits::StandardTrait},
};
use kagari_hir::{builtin::traits::StandardTraitSemantics, types::TypeId};
use kagari_mir::instruction::{Constant, Instruction, MirValue, Terminator};

impl FunctionLowerer<'_, '_> {
    pub(super) fn list_call(
        &mut self,
        source: &TypeId,
        item: &TypeId,
        name: &str,
        args: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let mut interface = StandardTrait::List.nominal();
        interface.arguments.push(item.clone());
        let method = self
            .planner
            .catalog
            .trait_(&interface.declaration)
            .and_then(|contract| contract.methods.iter().find(|method| method.name == name))
            .ok_or(MirLoweringError::MissingBinding("list query member"))?
            .id
            .clone();
        self.lower_applied_operator(interface, source.clone(), &method, args)
    }
    fn list_value(
        &mut self,
        source: &TypeId,
        item: &TypeId,
        value: MirValue,
        index: MirValue,
    ) -> Result<MirValue, MirLoweringError> {
        let optional = TypeId::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![item.clone()],
        };
        let member = self.list_call(source, item, "get", &[value, index])?;
        self.standard_enum_op(&optional, Op::Read(0), Some(member))
    }
    pub(super) fn query_binary(
        &mut self,
        op: BinaryOp,
        left: MirValue,
        right: MirValue,
        ty: ValueType,
    ) -> MirValue {
        let dst = self.alloc_temp(ty);
        self.emit(Instruction::Binary {
            dst,
            op,
            lhs: left,
            rhs: right,
        });
        dst
    }
    pub(super) fn query_guard(
        &mut self,
        source: &TypeId,
        value: MirValue,
    ) -> Result<(TypeId, MirValue), MirLoweringError> {
        let iterator_type = self.iteration_output(StandardTrait::Iterable, source, "Iter")?;
        let iterator = self.lower_applied_operator(
            StandardTrait::Iterable.nominal(),
            source.clone(),
            &self.protocol_method(StandardTrait::Iterable, 0)?,
            &[value],
        )?;
        self.emit(Instruction::BeginIteration {
            collection: iterator,
        });
        Ok((iterator_type, iterator))
    }
    pub(super) fn end_query_guard(&mut self, guard: (TypeId, MirValue)) {
        self.iterator_close(&guard.0, guard.1);
        self.emit(Instruction::EndIteration);
    }

    pub(super) fn lower_list_query(
        &mut self,
        operation: NativeDefaultMethod,
        source: &TypeId,
        args: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let item = self.iteration_output(StandardTrait::Iterable, source, "Item")?;
        self.usize_constant(0);
        let guard = self.query_guard(source, args[0])?;
        let length = self.list_call(source, &item, "len", &[args[0]])?;
        let one = self.usize_constant(1);
        let needle_source = if matches!(
            operation,
            NativeDefaultMethod::ListStartsWith | NativeDefaultMethod::ListEndsWith
        ) {
            let mut interface = StandardTrait::List.nominal();
            interface.arguments.push(item.clone());
            Some(TypeId::Trait(interface))
        } else {
            None
        };
        let other_guard = needle_source
            .as_ref()
            .map(|ty| self.query_guard(ty, args[1]))
            .transpose()?;
        let limit = if let Some(ty) = &needle_source {
            self.list_call(ty, &item, "len", &[args[1]])?
        } else {
            length
        };
        let result = self.lower_constant(
            Constant::Bool(operation != NativeDefaultMethod::ListContains),
            ValueType::Bool,
        );
        let index = self.usize_constant(0);
        let head = self.new_block();
        let body = self.new_block();
        let advanced = self.new_block();
        let mismatch = self.new_block();
        let found = self.new_block();
        let done = self.new_block();
        if needle_source.is_some() {
            let longer = self.query_binary(BinaryOp::Gt, limit, length, ValueType::Bool);
            self.set_terminator(Terminator::Branch {
                cond: longer,
                then_block: mismatch,
                else_block: head,
            });
        } else {
            self.ensure_jump(head);
        }
        self.switch_to_block(head);
        let more = self.query_binary(BinaryOp::Lt, index, limit, ValueType::Bool);
        self.set_terminator(Terminator::Branch {
            cond: more,
            then_block: body,
            else_block: done,
        });
        self.switch_to_block(body);
        let offset = if operation == NativeDefaultMethod::ListEndsWith {
            let start = self.query_binary(BinaryOp::Sub, length, limit, ValueType::U64);
            self.query_binary(BinaryOp::Add, start, index, ValueType::U64)
        } else {
            index
        };
        let left = self.list_value(source, &item, args[0], offset)?;
        let right = if let Some(ty) = &needle_source {
            self.list_value(ty, &item, args[1], index)?
        } else {
            args[1]
        };
        let equal = self.lower_protocol(StandardTrait::PartialEq, &item, &[left, right], 0)?;
        self.set_terminator(Terminator::Branch {
            cond: equal,
            then_block: if operation == NativeDefaultMethod::ListContains {
                found
            } else {
                advanced
            },
            else_block: if operation == NativeDefaultMethod::ListContains {
                advanced
            } else {
                mismatch
            },
        });
        self.switch_to_block(advanced);
        let next = self.query_binary(BinaryOp::Add, index, one, ValueType::U64);
        self.emit(Instruction::Move {
            dst: index,
            src: next,
        });
        self.ensure_jump(head);
        self.switch_to_block(found);
        let yes = self.lower_constant(Constant::Bool(true), ValueType::Bool);
        self.emit(Instruction::Move {
            dst: result,
            src: yes,
        });
        self.ensure_jump(done);
        self.switch_to_block(mismatch);
        let no = self.lower_constant(Constant::Bool(false), ValueType::Bool);
        self.emit(Instruction::Move {
            dst: result,
            src: no,
        });
        self.ensure_jump(done);
        self.switch_to_block(done);
        if let Some(guard) = other_guard {
            self.end_query_guard(guard);
        }
        self.end_query_guard(guard);
        Ok(result)
    }
}
