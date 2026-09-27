use super::*;
use crate::{
    lower::instances::IteratorInstance,
    module::{
        abi::AbiType,
        instruction::{IterOp, StandardEnumOp as Op},
    },
};
use kagari_hir::{
    builtin::{declarations::NativeDefaultMethod, surface::StandardEnum, traits::StandardTrait},
    types::{BuiltinType, TypeId},
};

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_list_windows(
        &mut self,
        operation: NativeDefaultMethod,
        source: &TypeId,
        args: &[IrValue],
    ) -> Result<IrValue, IrLoweringError> {
        let item = self.iteration_output(StandardTrait::Iterable, source, "Item")?;
        let zero = self.usize_constant(0);
        let valid = self.query_binary(BinaryOp::Gt, args[1], zero, ValueType::Bool);
        let message = self.lower_constant(
            Constant::Str("window or chunk size must be nonzero".into()),
            ValueType::Str,
        );
        self.emit_intrinsic(
            StandardIntrinsic::DebugAssert,
            &[valid, message],
            ValueType::Unit,
        );
        let iterator_type = self.iteration_output(StandardTrait::Iterable, source, "Iter")?;
        let iterator = self.lower_applied_operator(
            StandardTrait::Iterable.nominal(),
            source.clone(),
            &StandardTrait::Iterable.contract().methods[0].id,
            &[args[0]],
        )?;
        let length = self.list_call(source, &item, "len", &[args[0]])?;
        let size_type = TypeId::Builtin(BuiltinType::USize);
        let state_type = TypeId::Array(
            Box::new(size_type.clone()),
            kagari_common::collection::CollectionAccess::Mutable,
        );
        let state = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::MakeArray {
            dst: state,
            elements: vec![zero].into(),
        });
        self.function
            .semantic
            .registers
            .insert(state.temp.index(), AbiType::from_checked_type(&state_type));
        let mut interface = StandardTrait::List.nominal();
        interface.arguments.push(item);
        let output = TypeId::Trait(interface);
        let function = self.planner.enqueue_iterator(
            &self.instance,
            IteratorInstance {
                operation,
                captures: vec![
                    source.clone(),
                    size_type.clone(),
                    state_type,
                    iterator_type.clone(),
                    size_type,
                ],
                output: output.clone(),
                span: self.debug_span(),
            },
        )?;
        let step = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::MakeClosure {
            dst: step,
            function,
            captures: vec![args[0], args[1], state, iterator, length].into(),
        });
        let tuple = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::MakeTuple {
            dst: tuple,
            elements: vec![step, iterator].into(),
        });
        let ty = TypeId::Tuple(vec![
            TypeId::Function {
                params: vec![],
                result: Box::new(TypeId::StandardEnum {
                    kind: StandardEnum::Option,
                    args: vec![output],
                }),
            },
            iterator_type,
        ]);
        let dst = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::Iter {
            dst,
            value: Some(tuple),
            ty: AbiType::from_checked_type(&ty),
            op: IterOp::FromClosure,
        });
        Ok(dst)
    }

    pub(super) fn lower_window_step(
        &mut self,
        body: &IteratorInstance,
        args: &[IrValue],
    ) -> Result<(), IrLoweringError> {
        let source = &body.captures[0];
        let item = self.iteration_output(StandardTrait::Iterable, source, "Item")?;
        let zero = self.usize_constant(0);
        let start = self.prepared_read(args[2], zero, &TypeId::Builtin(BuiltinType::USize))?;
        let remaining = self.query_binary(BinaryOp::Sub, args[4], start, ValueType::U64);
        let available = if body.operation == NativeDefaultMethod::ListWindows {
            self.query_binary(BinaryOp::Ge, remaining, args[1], ValueType::Bool)
        } else {
            self.query_binary(BinaryOp::Gt, remaining, zero, ValueType::Bool)
        };
        let optional = TypeId::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![body.output.clone()],
        };
        let work = self.new_block();
        let done = self.new_block();
        self.set_terminator(Terminator::Branch {
            cond: available,
            then_block: work,
            else_block: done,
        });
        self.switch_to_block(work);
        let end = self.sort_bound(start, args[1], args[4])?;
        let count = self.query_binary(BinaryOp::Sub, end, start, ValueType::U64);
        let storage = TypeId::Array(
            Box::new(item.clone()),
            kagari_common::collection::CollectionAccess::Mutable,
        );
        let snapshot = self.collection_new(&storage)?;
        let member = TypeId::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![item.clone()],
        };
        self.prepared_indices(count, |this, offset| {
            let index = this.query_binary(BinaryOp::Add, start, offset, ValueType::U64);
            let value = this.list_call(source, &item, "get", &[args[0], index])?;
            let value = this.standard_enum_op(&member, Op::Read(0), Some(value))?;
            this.collection_insert(&storage, snapshot, value)
        })?;
        let snapshot = self.readonly_array(item, snapshot)?;
        let some = self.standard_enum_op(&optional, Op::Make(0), Some(snapshot))?;
        let next = if body.operation == NativeDefaultMethod::ListWindows {
            let one = self.usize_constant(1);
            self.query_binary(BinaryOp::Add, start, one, ValueType::U64)
        } else {
            end
        };
        self.emit(Instruction::WriteAggregateIndex {
            base: args[2],
            index: zero,
            value: next,
        });
        self.set_terminator(Terminator::Return(Some(some)));
        self.switch_to_block(done);
        self.iterator_close(&body.captures[3], args[3]);
        self.emit(Instruction::WriteAggregateIndex {
            base: args[2],
            index: zero,
            value: args[4],
        });
        let none = self.standard_enum_op(&optional, Op::Make(1), None)?;
        self.set_terminator(Terminator::Return(Some(none)));
        Ok(())
    }
}
