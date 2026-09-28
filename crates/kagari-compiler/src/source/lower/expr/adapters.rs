use crate::source::{
    lower::{MirLoweringError, instances::IteratorInstance, state::FunctionLowerer},
    types::lower_type,
};
use kagari_abi::{
    operations::{BinaryOp, IterOp, StandardEnumOp},
    representation::ValueType,
    scalar::BuiltinType,
    standard::{bindings::NativeDefaultMethod, surface::StandardEnum, traits::StandardTrait},
    types::AbiType,
};
use kagari_common::collection::CollectionAccess;
use kagari_hir::{builtin::traits::StandardTraitSemantics, types::TypeId};
use kagari_mir::instruction::{CallTarget, Constant, Instruction, MirValue, Terminator};
use std::iter;

fn option(item: TypeId) -> TypeId {
    TypeId::StandardEnum {
        kind: StandardEnum::Option,
        args: vec![item],
    }
}

impl FunctionLowerer<'_, '_> {
    pub(super) fn usize_constant(&mut self, value: u64) -> MirValue {
        let result = self.lower_constant(Constant::U64(value), ValueType::U64);
        self.function
            .semantic
            .registers
            .insert(result.temp.index(), AbiType::Builtin(BuiltinType::USize));
        result
    }
    pub(super) fn lower_iterator_adapter(
        &mut self,
        operation: NativeDefaultMethod,
        source: &TypeId,
        arguments: &[TypeId],
        values: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let item = self.iterator_item(source)?;
        let mut captures = vec![values[0]];
        let mut types = vec![source.clone()];
        let mut dependencies = vec![values[0]];
        let mut dependency_types = vec![source.clone()];
        let output = match operation {
            NativeDefaultMethod::FlatMap | NativeDefaultMethod::Flatten => {
                let inner_source = if operation == NativeDefaultMethod::FlatMap {
                    arguments[0].clone()
                } else {
                    item.clone()
                };
                let inner =
                    self.iteration_output(StandardTrait::Iterable, &inner_source, "Iter")?;
                let output = self.iterator_item(&inner)?;
                let empty =
                    self.standard_enum_op(&option(inner.clone()), StandardEnumOp::Make(1), None)?;
                let state_type =
                    TypeId::Array(Box::new(option(inner.clone())), CollectionAccess::Mutable);
                let state = self.alloc_temp(ValueType::HeapObject);
                self.emit(Instruction::MakeArray {
                    dst: state,
                    elements: vec![empty].into(),
                });
                self.function
                    .semantic
                    .registers
                    .insert(state.temp.index(), lower_type(&state_type));
                captures.push(state);
                types.push(state_type.clone());
                if matches!(inner, TypeId::Iter(_)) {
                    dependencies.push(state);
                    dependency_types.push(state_type);
                }
                captures.push(self.new_adapter_flag(false));
                types.push(TypeId::Array(
                    Box::new(TypeId::Builtin(BuiltinType::Bool)),
                    CollectionAccess::Mutable,
                ));
                if operation == NativeDefaultMethod::FlatMap {
                    captures.push(values[1]);
                    types.push(TypeId::Function {
                        params: vec![item.clone()],
                        result: Box::new(inner_source),
                    });
                }
                output
            }
            NativeDefaultMethod::Map
            | NativeDefaultMethod::FilterMap
            | NativeDefaultMethod::Filter
            | NativeDefaultMethod::Inspect
            | NativeDefaultMethod::TakeWhile
            | NativeDefaultMethod::SkipWhile => {
                let output = if matches!(
                    operation,
                    NativeDefaultMethod::Filter
                        | NativeDefaultMethod::Inspect
                        | NativeDefaultMethod::TakeWhile
                        | NativeDefaultMethod::SkipWhile
                ) {
                    item.clone()
                } else {
                    arguments[0].clone()
                };
                let result = match operation {
                    NativeDefaultMethod::Filter
                    | NativeDefaultMethod::TakeWhile
                    | NativeDefaultMethod::SkipWhile => TypeId::Builtin(BuiltinType::Bool),
                    NativeDefaultMethod::Inspect => TypeId::Builtin(BuiltinType::Unit),
                    NativeDefaultMethod::FilterMap => option(output.clone()),
                    _ => output.clone(),
                };
                captures.push(values[1]);
                types.push(TypeId::Function {
                    params: vec![item.clone()],
                    result: Box::new(result),
                });
                if matches!(
                    operation,
                    NativeDefaultMethod::TakeWhile | NativeDefaultMethod::SkipWhile
                ) {
                    let state = self.new_adapter_flag(false);
                    captures.push(state);
                    types.push(TypeId::Array(
                        Box::new(TypeId::Builtin(BuiltinType::Bool)),
                        CollectionAccess::Mutable,
                    ));
                }
                output
            }
            NativeDefaultMethod::Fuse => {
                captures.push(self.new_adapter_flag(false));
                types.push(TypeId::Array(
                    Box::new(TypeId::Builtin(BuiltinType::Bool)),
                    CollectionAccess::Mutable,
                ));
                item.clone()
            }
            NativeDefaultMethod::Take
            | NativeDefaultMethod::Skip
            | NativeDefaultMethod::Enumerate => {
                let initial = if operation == NativeDefaultMethod::Enumerate {
                    self.usize_constant(0)
                } else {
                    values[1]
                };
                let state = self.alloc_temp(ValueType::HeapObject);
                self.emit(Instruction::MakeArray {
                    dst: state,
                    elements: vec![initial].into(),
                });
                let state_type = TypeId::Array(
                    Box::new(TypeId::Builtin(BuiltinType::USize)),
                    CollectionAccess::Mutable,
                );
                self.function
                    .semantic
                    .registers
                    .insert(state.temp.index(), lower_type(&state_type));
                captures.push(state);
                types.push(state_type);
                if operation == NativeDefaultMethod::Enumerate {
                    TypeId::Tuple(vec![TypeId::Builtin(BuiltinType::USize), item.clone()])
                } else {
                    item.clone()
                }
            }
            NativeDefaultMethod::Zip | NativeDefaultMethod::Chain => {
                let other =
                    self.iteration_output(StandardTrait::Iterable, &arguments[0], "Iter")?;
                let value = self.lower_applied_operator(
                    StandardTrait::Iterable.nominal(),
                    arguments[0].clone(),
                    &StandardTrait::Iterable.contract().methods[0].id,
                    &[values[1]],
                )?;
                captures.push(value);
                types.push(other.clone());
                dependencies.push(value);
                dependency_types.push(other.clone());
                if operation == NativeDefaultMethod::Chain {
                    let initial = self.lower_constant(Constant::I32(0), ValueType::I32);
                    let state = self.alloc_temp(ValueType::HeapObject);
                    self.emit(Instruction::MakeArray {
                        dst: state,
                        elements: vec![initial].into(),
                    });
                    let state_type = TypeId::Array(
                        Box::new(TypeId::Builtin(BuiltinType::I32)),
                        CollectionAccess::Mutable,
                    );
                    self.function
                        .semantic
                        .registers
                        .insert(state.temp.index(), lower_type(&state_type));
                    captures.push(state);
                    types.push(state_type);
                    item.clone()
                } else {
                    TypeId::Tuple(vec![item.clone(), self.iterator_item(&other)?])
                }
            }
            _ => return Err(MirLoweringError::MissingBinding("lazy adapter")),
        };
        let function = self.planner.enqueue_iterator(
            &self.instance,
            IteratorInstance {
                operation,
                captures: types,
                output: output.clone(),
                span: self.debug_span(),
            },
        )?;
        let step = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::MakeClosure {
            dst: step,
            function,
            captures: captures.into(),
        });
        let step_type = TypeId::Function {
            params: vec![],
            result: Box::new(option(output)),
        };
        let source = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::MakeTuple {
            dst: source,
            elements: iter::once(step).chain(dependencies).collect(),
        });
        let source_type = TypeId::Tuple(iter::once(step_type).chain(dependency_types).collect());
        let dst = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::Iter {
            dst,
            value: Some(source),
            ty: lower_type(&source_type),
            op: IterOp::FromClosure,
        });
        Ok(dst)
    }

    pub(super) fn iterator_next(
        &mut self,
        ty: &TypeId,
        value: MirValue,
    ) -> Result<MirValue, MirLoweringError> {
        self.lower_applied_operator(
            StandardTrait::Iterator.nominal(),
            ty.clone(),
            &StandardTrait::Iterator.contract().methods[0].id,
            &[value],
        )
    }

    pub(super) fn iterator_close(&mut self, ty: &TypeId, value: MirValue) {
        if matches!(ty, TypeId::Iter(_)) {
            let dst = self.alloc_temp(ValueType::Unit);
            self.emit(Instruction::Iter {
                dst,
                value: Some(value),
                ty: lower_type(ty),
                op: IterOp::Close,
            });
        }
    }

    pub(super) fn call_function_value(
        &mut self,
        callback: MirValue,
        result: &TypeId,
        args: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let dst = self.alloc_temp(self.value_type(result)?);
        self.emit(Instruction::Call {
            dst: Some(dst),
            callee: CallTarget::Closure {
                value: callback,
                params: args.iter().map(|v| v.ty).collect(),
                return_type: dst.ty,
            },
            args: args.iter().copied().collect(),
        });
        self.function
            .semantic
            .registers
            .insert(dst.temp.index(), lower_type(result));
        Ok(dst)
    }

    fn new_adapter_flag(&mut self, value: bool) -> MirValue {
        let initial = self.lower_constant(Constant::Bool(value), ValueType::Bool);
        let state = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::MakeArray {
            dst: state,
            elements: vec![initial].into(),
        });
        self.function.semantic.registers.insert(
            state.temp.index(),
            AbiType::Array(
                Box::new(AbiType::Builtin(BuiltinType::Bool)),
                CollectionAccess::Mutable,
            ),
        );
        state
    }
    fn adapter_state(&mut self, state: MirValue, ty: ValueType) -> MirValue {
        let index = self.lower_constant(Constant::I32(0), ValueType::I32);
        let dst = self.alloc_temp(ty);
        self.emit(Instruction::ReadAggregateIndex {
            dst,
            base: state,
            index,
        });
        dst
    }
    fn set_adapter_state(&mut self, state: MirValue, value: MirValue) {
        let index = self.lower_constant(Constant::I32(0), ValueType::I32);
        self.emit(Instruction::WriteAggregateIndex {
            base: state,
            index,
            value,
        });
    }
    fn adapter_binary(
        &mut self,
        op: BinaryOp,
        lhs: MirValue,
        rhs: MirValue,
        ty: ValueType,
    ) -> MirValue {
        let dst = self.alloc_temp(ty);
        self.emit(Instruction::Binary { dst, op, lhs, rhs });
        dst
    }

    pub(crate) fn lower_iterator_step(
        &mut self,
        body: &IteratorInstance,
        args: &[MirValue],
    ) -> Result<(), MirLoweringError> {
        if matches!(
            body.operation,
            NativeDefaultMethod::ListWindows | NativeDefaultMethod::ListChunks
        ) {
            return self.lower_window_step(body, args);
        }
        if matches!(
            body.operation,
            NativeDefaultMethod::FlatMap | NativeDefaultMethod::Flatten
        ) {
            return self.lower_flatten_step(body, args);
        }
        let source = &body.captures[0];
        let item = self.iterator_item(source)?;
        let input_option = option(item.clone());
        let output_option = option(body.output.clone());
        let head = self.new_block();
        let done = self.new_block();
        self.ensure_jump(head);
        self.switch_to_block(head);
        if body.operation == NativeDefaultMethod::Chain {
            let state = self.adapter_state(args[2], ValueType::I32);
            let zero = self.lower_constant(Constant::I32(0), ValueType::I32);
            let left = self.adapter_binary(BinaryOp::Eq, state, zero, ValueType::Bool);
            let first = self.new_block();
            let second = self.new_block();
            let right = self.new_block();
            self.set_terminator(Terminator::Branch {
                cond: left,
                then_block: first,
                else_block: second,
            });
            self.switch_to_block(first);
            let next = self.iterator_next(source, args[0])?;
            let present =
                self.standard_enum_op(&input_option, StandardEnumOp::Test(0), Some(next))?;
            let yield_left = self.new_block();
            let switch = self.new_block();
            self.set_terminator(Terminator::Branch {
                cond: present,
                then_block: yield_left,
                else_block: switch,
            });
            self.switch_to_block(yield_left);
            self.set_terminator(Terminator::Return(Some(next)));
            self.switch_to_block(switch);
            let one = self.lower_constant(Constant::I32(1), ValueType::I32);
            self.set_adapter_state(args[2], one);
            self.iterator_close(source, args[0]);
            self.ensure_jump(head);
            self.switch_to_block(second);
            let one = self.lower_constant(Constant::I32(1), ValueType::I32);
            let active = self.adapter_binary(BinaryOp::Eq, state, one, ValueType::Bool);
            self.set_terminator(Terminator::Branch {
                cond: active,
                then_block: right,
                else_block: done,
            });
            self.switch_to_block(right);
            let next = self.iterator_next(&body.captures[1], args[1])?;
            let present =
                self.standard_enum_op(&input_option, StandardEnumOp::Test(0), Some(next))?;
            let yield_right = self.new_block();
            self.set_terminator(Terminator::Branch {
                cond: present,
                then_block: yield_right,
                else_block: done,
            });
            self.switch_to_block(yield_right);
            self.set_terminator(Terminator::Return(Some(next)));
        } else {
            if matches!(
                body.operation,
                NativeDefaultMethod::Fuse | NativeDefaultMethod::TakeWhile
            ) {
                let state = args[if body.operation == NativeDefaultMethod::Fuse {
                    1
                } else {
                    2
                }];
                let ended = self.adapter_state(state, ValueType::Bool);
                let advance = self.new_block();
                self.set_terminator(Terminator::Branch {
                    cond: ended,
                    then_block: done,
                    else_block: advance,
                });
                self.switch_to_block(advance);
            }
            if body.operation == NativeDefaultMethod::Take {
                let count = self.adapter_state(args[1], ValueType::U64);
                let zero = self.lower_constant(Constant::U64(0), ValueType::U64);
                let empty = self.adapter_binary(BinaryOp::Eq, count, zero, ValueType::Bool);
                let advance = self.new_block();
                self.set_terminator(Terminator::Branch {
                    cond: empty,
                    then_block: done,
                    else_block: advance,
                });
                self.switch_to_block(advance);
                let one = self.lower_constant(Constant::U64(1), ValueType::U64);
                let remaining = self.adapter_binary(BinaryOp::Sub, count, one, ValueType::U64);
                self.set_adapter_state(args[1], remaining);
            }
            let next = self.iterator_next(source, args[0])?;
            let present =
                self.standard_enum_op(&input_option, StandardEnumOp::Test(0), Some(next))?;
            let some = self.new_block();
            self.set_terminator(Terminator::Branch {
                cond: present,
                then_block: some,
                else_block: done,
            });
            self.switch_to_block(some);
            let value =
                self.standard_enum_op(&input_option, StandardEnumOp::Read(0), Some(next))?;
            let value = match body.operation {
                NativeDefaultMethod::Fuse => value,
                NativeDefaultMethod::Inspect => {
                    self.call_function_value(
                        args[1],
                        &TypeId::Builtin(BuiltinType::Unit),
                        &[value],
                    )?;
                    value
                }
                NativeDefaultMethod::TakeWhile | NativeDefaultMethod::SkipWhile => {
                    let yield_item = self.new_block();
                    if body.operation == NativeDefaultMethod::SkipWhile {
                        let passing = self.adapter_state(args[2], ValueType::Bool);
                        let test = self.new_block();
                        self.set_terminator(Terminator::Branch {
                            cond: passing,
                            then_block: yield_item,
                            else_block: test,
                        });
                        self.switch_to_block(test);
                    }
                    let keep = self.call_function_value(
                        args[1],
                        &TypeId::Builtin(BuiltinType::Bool),
                        &[value],
                    )?;
                    if body.operation == NativeDefaultMethod::TakeWhile {
                        self.set_terminator(Terminator::Branch {
                            cond: keep,
                            then_block: yield_item,
                            else_block: done,
                        });
                    } else {
                        let stop_skipping = self.new_block();
                        self.set_terminator(Terminator::Branch {
                            cond: keep,
                            then_block: head,
                            else_block: stop_skipping,
                        });
                        self.switch_to_block(stop_skipping);
                        let passing = self.lower_constant(Constant::Bool(true), ValueType::Bool);
                        self.set_adapter_state(args[2], passing);
                        self.ensure_jump(yield_item);
                    }
                    self.switch_to_block(yield_item);
                    value
                }
                NativeDefaultMethod::Map => {
                    self.call_function_value(args[1], &body.output, &[value])?
                }
                NativeDefaultMethod::Filter => {
                    let keep = self.call_function_value(
                        args[1],
                        &TypeId::Builtin(BuiltinType::Bool),
                        &[value],
                    )?;
                    let yield_item = self.new_block();
                    self.set_terminator(Terminator::Branch {
                        cond: keep,
                        then_block: yield_item,
                        else_block: head,
                    });
                    self.switch_to_block(yield_item);
                    value
                }
                NativeDefaultMethod::FilterMap => {
                    let mapped = self.call_function_value(args[1], &output_option, &[value])?;
                    let present = self.standard_enum_op(
                        &output_option,
                        StandardEnumOp::Test(0),
                        Some(mapped),
                    )?;
                    let yield_item = self.new_block();
                    self.set_terminator(Terminator::Branch {
                        cond: present,
                        then_block: yield_item,
                        else_block: head,
                    });
                    self.switch_to_block(yield_item);
                    self.set_terminator(Terminator::Return(Some(mapped)));
                    self.switch_to_block(done);
                    self.iterator_close(source, args[0]);
                    let none =
                        self.standard_enum_op(&output_option, StandardEnumOp::Make(1), None)?;
                    self.set_terminator(Terminator::Return(Some(none)));
                    return Ok(());
                }
                NativeDefaultMethod::Skip => {
                    let count = self.adapter_state(args[1], ValueType::U64);
                    let zero = self.lower_constant(Constant::U64(0), ValueType::U64);
                    let empty = self.adapter_binary(BinaryOp::Eq, count, zero, ValueType::Bool);
                    let yield_item = self.new_block();
                    let skip = self.new_block();
                    self.set_terminator(Terminator::Branch {
                        cond: empty,
                        then_block: yield_item,
                        else_block: skip,
                    });
                    self.switch_to_block(skip);
                    let one = self.lower_constant(Constant::U64(1), ValueType::U64);
                    let remaining = self.adapter_binary(BinaryOp::Sub, count, one, ValueType::U64);
                    self.set_adapter_state(args[1], remaining);
                    self.ensure_jump(head);
                    self.switch_to_block(yield_item);
                    value
                }
                NativeDefaultMethod::Enumerate => {
                    let index = self.adapter_state(args[1], ValueType::U64);
                    let one = self.lower_constant(Constant::U64(1), ValueType::U64);
                    let next_index = self.adapter_binary(BinaryOp::Add, index, one, ValueType::U64);
                    self.set_adapter_state(args[1], next_index);
                    let pair = self.alloc_temp(ValueType::HeapObject);
                    self.emit(Instruction::MakeTuple {
                        dst: pair,
                        elements: vec![index, value].into(),
                    });
                    pair
                }
                NativeDefaultMethod::Zip => {
                    let other = self.iterator_next(&body.captures[1], args[1])?;
                    let right_option = option(self.iterator_item(&body.captures[1])?);
                    let present =
                        self.standard_enum_op(&right_option, StandardEnumOp::Test(0), Some(other))?;
                    let pair_block = self.new_block();
                    self.set_terminator(Terminator::Branch {
                        cond: present,
                        then_block: pair_block,
                        else_block: done,
                    });
                    self.switch_to_block(pair_block);
                    let right =
                        self.standard_enum_op(&right_option, StandardEnumOp::Read(0), Some(other))?;
                    let pair = self.alloc_temp(ValueType::HeapObject);
                    self.emit(Instruction::MakeTuple {
                        dst: pair,
                        elements: vec![value, right].into(),
                    });
                    pair
                }
                NativeDefaultMethod::Take => value,
                _ => return Err(MirLoweringError::MissingBinding("iterator step operation")),
            };
            let some =
                self.standard_enum_op(&output_option, StandardEnumOp::Make(0), Some(value))?;
            self.set_terminator(Terminator::Return(Some(some)));
        }
        self.switch_to_block(done);
        if matches!(
            body.operation,
            NativeDefaultMethod::Fuse | NativeDefaultMethod::TakeWhile
        ) {
            let ended = self.lower_constant(Constant::Bool(true), ValueType::Bool);
            self.set_adapter_state(
                args[if body.operation == NativeDefaultMethod::Fuse {
                    1
                } else {
                    2
                }],
                ended,
            );
        }
        if body.operation == NativeDefaultMethod::Chain {
            let end = self.lower_constant(Constant::I32(2), ValueType::I32);
            self.set_adapter_state(args[2], end);
        }
        if body.operation == NativeDefaultMethod::Skip {
            let zero = self.usize_constant(0);
            self.set_adapter_state(args[1], zero);
        }
        self.iterator_close(source, args[0]);
        if matches!(
            body.operation,
            NativeDefaultMethod::Zip | NativeDefaultMethod::Chain
        ) {
            self.iterator_close(&body.captures[1], args[1]);
        }
        let none = self.standard_enum_op(&output_option, StandardEnumOp::Make(1), None)?;
        self.set_terminator(Terminator::Return(Some(none)));
        Ok(())
    }

    fn lower_flatten_step(
        &mut self,
        body: &IteratorInstance,
        args: &[MirValue],
    ) -> Result<(), MirLoweringError> {
        let source = &body.captures[0];
        let item = self.iterator_item(source)?;
        let input_option = option(item.clone());
        let output_option = option(body.output.clone());
        let TypeId::Array(inner_option, _) = &body.captures[1] else {
            unreachable!()
        };
        let TypeId::StandardEnum {
            args: inner_args, ..
        } = inner_option.as_ref()
        else {
            unreachable!()
        };
        let inner = &inner_args[0];
        let inner_source = if let Some(TypeId::Function { result, .. }) = body.captures.get(3) {
            result.as_ref()
        } else {
            &item
        };
        let head = self.new_block();
        let done = self.new_block();
        let ended = self.adapter_state(args[2], ValueType::Bool);
        self.set_terminator(Terminator::Branch {
            cond: ended,
            then_block: done,
            else_block: head,
        });
        self.switch_to_block(head);
        let state = self.adapter_state(args[1], ValueType::HeapObject);
        self.function
            .semantic
            .registers
            .insert(state.temp.index(), lower_type(inner_option));
        let present = self.standard_enum_op(inner_option, StandardEnumOp::Test(0), Some(state))?;
        let advance_inner = self.new_block();
        let advance_outer = self.new_block();
        self.set_terminator(Terminator::Branch {
            cond: present,
            then_block: advance_inner,
            else_block: advance_outer,
        });
        self.switch_to_block(advance_inner);
        let iterator = self.standard_enum_op(inner_option, StandardEnumOp::Read(0), Some(state))?;
        let next = self.iterator_next(inner, iterator)?;
        let present = self.standard_enum_op(&output_option, StandardEnumOp::Test(0), Some(next))?;
        let yield_item = self.new_block();
        let finish_inner = self.new_block();
        self.set_terminator(Terminator::Branch {
            cond: present,
            then_block: yield_item,
            else_block: finish_inner,
        });
        self.switch_to_block(yield_item);
        self.set_terminator(Terminator::Return(Some(next)));
        self.switch_to_block(finish_inner);
        self.iterator_close(inner, iterator);
        let empty = self.standard_enum_op(inner_option, StandardEnumOp::Make(1), None)?;
        self.set_adapter_state(args[1], empty);
        self.ensure_jump(advance_outer);
        self.switch_to_block(advance_outer);
        let next = self.iterator_next(source, args[0])?;
        let present = self.standard_enum_op(&input_option, StandardEnumOp::Test(0), Some(next))?;
        let start_inner = self.new_block();
        self.set_terminator(Terminator::Branch {
            cond: present,
            then_block: start_inner,
            else_block: done,
        });
        self.switch_to_block(start_inner);
        let value = self.standard_enum_op(&input_option, StandardEnumOp::Read(0), Some(next))?;
        let value = if body.operation == NativeDefaultMethod::FlatMap {
            self.call_function_value(args[3], inner_source, &[value])?
        } else {
            value
        };
        let iterator = self.lower_applied_operator(
            StandardTrait::Iterable.nominal(),
            inner_source.clone(),
            &StandardTrait::Iterable.contract().methods[0].id,
            &[value],
        )?;
        let state = self.standard_enum_op(inner_option, StandardEnumOp::Make(0), Some(iterator))?;
        self.set_adapter_state(args[1], state);
        self.ensure_jump(head);
        self.switch_to_block(done);
        let ended = self.lower_constant(Constant::Bool(true), ValueType::Bool);
        self.set_adapter_state(args[2], ended);
        self.iterator_close(source, args[0]);
        let none = self.standard_enum_op(&output_option, StandardEnumOp::Make(1), None)?;
        self.set_terminator(Terminator::Return(Some(none)));
        Ok(())
    }
}
