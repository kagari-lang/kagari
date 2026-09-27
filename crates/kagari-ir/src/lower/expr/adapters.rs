use super::*;
use crate::lower::instances::IteratorInstance;
use crate::module::{
    abi::AbiType,
    instruction::{IterOp, StandardEnumOp},
};
use kagari_hir::{
    builtin::{declarations::IteratorMethod, surface::StandardEnum, traits::StandardTrait},
    types::{BuiltinType, TypeId},
};

fn option(item: TypeId) -> TypeId {
    TypeId::StandardEnum {
        kind: StandardEnum::Option,
        args: vec![item],
    }
}

impl FunctionLowerer<'_, '_> {
    pub(super) fn usize_constant(&mut self, value: i64) -> IrValue {
        let result = self.lower_constant(Constant::I64(value), ValueType::I64);
        self.function
            .semantic
            .registers
            .insert(result.temp.index(), AbiType::Builtin(BuiltinType::USize));
        result
    }
    pub(super) fn lower_iterator_adapter(
        &mut self,
        operation: IteratorMethod,
        source: &TypeId,
        arguments: &[TypeId],
        values: &[IrValue],
    ) -> Result<IrValue, IrLoweringError> {
        use IteratorMethod::*;
        let item = self.iterator_item(source)?;
        let mut captures = vec![values[0]];
        let mut types = vec![source.clone()];
        let mut dependencies = vec![values[0]];
        let mut dependency_types = vec![source.clone()];
        let output = match operation {
            Map | FilterMap | Filter => {
                let output = if operation == Filter {
                    item.clone()
                } else {
                    arguments[0].clone()
                };
                let result = match operation {
                    Filter => TypeId::Builtin(BuiltinType::Bool),
                    FilterMap => option(output.clone()),
                    _ => output.clone(),
                };
                captures.push(values[1]);
                types.push(TypeId::Function {
                    params: vec![item.clone()],
                    result: Box::new(result),
                });
                output
            }
            Take | Skip | Enumerate => {
                let initial = if operation == Enumerate {
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
                    kagari_common::collection::CollectionAccess::Mutable,
                );
                self.function
                    .semantic
                    .registers
                    .insert(state.temp.index(), AbiType::from_checked_type(&state_type));
                captures.push(state);
                types.push(state_type);
                if operation == Enumerate {
                    TypeId::Tuple(vec![TypeId::Builtin(BuiltinType::USize), item.clone()])
                } else {
                    item.clone()
                }
            }
            Zip | Chain => {
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
                if operation == Chain {
                    let initial = self.lower_constant(Constant::I32(0), ValueType::I32);
                    let state = self.alloc_temp(ValueType::HeapObject);
                    self.emit(Instruction::MakeArray {
                        dst: state,
                        elements: vec![initial].into(),
                    });
                    let state_type = TypeId::Array(
                        Box::new(TypeId::Builtin(BuiltinType::I32)),
                        kagari_common::collection::CollectionAccess::Mutable,
                    );
                    self.function
                        .semantic
                        .registers
                        .insert(state.temp.index(), AbiType::from_checked_type(&state_type));
                    captures.push(state);
                    types.push(state_type);
                    item.clone()
                } else {
                    TypeId::Tuple(vec![item.clone(), self.iterator_item(&other)?])
                }
            }
            _ => return Err(IrLoweringError::MissingBinding("lazy adapter")),
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
            elements: std::iter::once(step).chain(dependencies).collect(),
        });
        let source_type =
            TypeId::Tuple(std::iter::once(step_type).chain(dependency_types).collect());
        let dst = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::Iter {
            dst,
            value: Some(source),
            ty: AbiType::from_checked_type(&source_type),
            op: IterOp::FromClosure,
        });
        Ok(dst)
    }

    pub(super) fn iterator_next(
        &mut self,
        ty: &TypeId,
        value: IrValue,
    ) -> Result<IrValue, IrLoweringError> {
        self.lower_applied_operator(
            StandardTrait::Iterator.nominal(),
            ty.clone(),
            &StandardTrait::Iterator.contract().methods[0].id,
            &[value],
        )
    }

    pub(super) fn iterator_close(&mut self, ty: &TypeId, value: IrValue) {
        if matches!(ty, TypeId::Iter(_)) {
            let dst = self.alloc_temp(ValueType::Unit);
            self.emit(Instruction::Iter {
                dst,
                value: Some(value),
                ty: AbiType::from_checked_type(ty),
                op: IterOp::Close,
            });
        }
    }

    pub(super) fn iterator_callback(
        &mut self,
        callback: IrValue,
        result: &TypeId,
        args: &[IrValue],
    ) -> Result<IrValue, IrLoweringError> {
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
            .insert(dst.temp.index(), AbiType::from_checked_type(result));
        Ok(dst)
    }

    fn adapter_state(&mut self, state: IrValue, ty: ValueType) -> IrValue {
        let index = self.lower_constant(Constant::I32(0), ValueType::I32);
        let dst = self.alloc_temp(ty);
        self.emit(Instruction::ReadAggregateIndex {
            dst,
            base: state,
            index,
        });
        dst
    }
    fn set_adapter_state(&mut self, state: IrValue, value: IrValue) {
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
        lhs: IrValue,
        rhs: IrValue,
        ty: ValueType,
    ) -> IrValue {
        let dst = self.alloc_temp(ty);
        self.emit(Instruction::Binary { dst, op, lhs, rhs });
        dst
    }

    pub(crate) fn lower_iterator_step(
        &mut self,
        body: &IteratorInstance,
        args: &[IrValue],
    ) -> Result<(), IrLoweringError> {
        use IteratorMethod::*;
        let source = &body.captures[0];
        let item = self.iterator_item(source)?;
        let input_option = option(item.clone());
        let output_option = option(body.output.clone());
        let head = self.new_block();
        let done = self.new_block();
        self.ensure_jump(head);
        self.switch_to_block(head);
        if body.operation == Chain {
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
            if body.operation == Take {
                let count = self.adapter_state(args[1], ValueType::I64);
                let zero = self.lower_constant(Constant::I64(0), ValueType::I64);
                let empty = self.adapter_binary(BinaryOp::Eq, count, zero, ValueType::Bool);
                let advance = self.new_block();
                self.set_terminator(Terminator::Branch {
                    cond: empty,
                    then_block: done,
                    else_block: advance,
                });
                self.switch_to_block(advance);
                let one = self.lower_constant(Constant::I64(1), ValueType::I64);
                let remaining = self.adapter_binary(BinaryOp::Sub, count, one, ValueType::I64);
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
                Map => self.iterator_callback(args[1], &body.output, &[value])?,
                Filter => {
                    let keep = self.iterator_callback(
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
                FilterMap => {
                    let mapped = self.iterator_callback(args[1], &output_option, &[value])?;
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
                Skip => {
                    let count = self.adapter_state(args[1], ValueType::I64);
                    let zero = self.lower_constant(Constant::I64(0), ValueType::I64);
                    let empty = self.adapter_binary(BinaryOp::Eq, count, zero, ValueType::Bool);
                    let yield_item = self.new_block();
                    let skip = self.new_block();
                    self.set_terminator(Terminator::Branch {
                        cond: empty,
                        then_block: yield_item,
                        else_block: skip,
                    });
                    self.switch_to_block(skip);
                    let one = self.lower_constant(Constant::I64(1), ValueType::I64);
                    let remaining = self.adapter_binary(BinaryOp::Sub, count, one, ValueType::I64);
                    self.set_adapter_state(args[1], remaining);
                    self.ensure_jump(head);
                    self.switch_to_block(yield_item);
                    value
                }
                Enumerate => {
                    let index = self.adapter_state(args[1], ValueType::I64);
                    let one = self.lower_constant(Constant::I64(1), ValueType::I64);
                    let next_index = self.adapter_binary(BinaryOp::Add, index, one, ValueType::I64);
                    self.set_adapter_state(args[1], next_index);
                    let pair = self.alloc_temp(ValueType::HeapObject);
                    self.emit(Instruction::MakeTuple {
                        dst: pair,
                        elements: vec![index, value].into(),
                    });
                    pair
                }
                Zip => {
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
                Take => value,
                _ => return Err(IrLoweringError::MissingBinding("iterator step operation")),
            };
            let some =
                self.standard_enum_op(&output_option, StandardEnumOp::Make(0), Some(value))?;
            self.set_terminator(Terminator::Return(Some(some)));
        }
        self.switch_to_block(done);
        if body.operation == Chain {
            let end = self.lower_constant(Constant::I32(2), ValueType::I32);
            self.set_adapter_state(args[2], end);
        }
        if body.operation == Skip {
            let zero = self.usize_constant(0);
            self.set_adapter_state(args[1], zero);
        }
        self.iterator_close(source, args[0]);
        if matches!(body.operation, Zip | Chain) {
            self.iterator_close(&body.captures[1], args[1]);
        }
        let none = self.standard_enum_op(&output_option, StandardEnumOp::Make(1), None)?;
        self.set_terminator(Terminator::Return(Some(none)));
        Ok(())
    }
}
