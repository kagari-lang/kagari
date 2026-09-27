use crate::lower::IrLoweringError;
use crate::lower::state::FunctionLowerer;
use crate::module::instruction::BinaryOp;
use crate::module::instruction::Constant;
use crate::module::instruction::Instruction;
use crate::module::instruction::IrValue;
use crate::module::instruction::Terminator;
use crate::module::{abi::AbiType, instruction::StandardEnumOp};
use kagari_abi::representation::ValueType;
use kagari_abi::scalar::BuiltinType;
use kagari_abi::standard::StandardIntrinsic;
use kagari_common::collection::CollectionAccess::Mutable;
use kagari_hir::builtin::declarations::NativeDefaultMethod;
use kagari_hir::builtin::surface::StandardEnum;
use kagari_hir::builtin::traits::StandardTrait;
use kagari_hir::types::TypeId;
use std::slice;

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_iterator_terminal(
        &mut self,
        operation: NativeDefaultMethod,
        source: &TypeId,
        arguments: &[TypeId],
        values: &[IrValue],
    ) -> Result<IrValue, IrLoweringError> {
        let item_type = self.iterator_item(source)?;
        let optional = TypeId::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![item_type.clone()],
        };
        let array_type = TypeId::Array(Box::new(item_type.clone()), Mutable);
        let bool_type = TypeId::Builtin(BuiltinType::Bool);
        let unit_type = TypeId::Builtin(BuiltinType::Unit);
        let result_optional = TypeId::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![match operation {
                NativeDefaultMethod::FindMap => arguments[0].clone(),
                NativeDefaultMethod::Position => TypeId::Builtin(BuiltinType::USize),
                _ => item_type.clone(),
            }],
        };
        let counter = if matches!(
            operation,
            NativeDefaultMethod::Position | NativeDefaultMethod::Nth
        ) {
            let initial = if operation == NativeDefaultMethod::Nth {
                values[1]
            } else {
                self.usize_constant(0)
            };
            let counter = self.alloc_temp(ValueType::U64);
            self.emit(Instruction::Move {
                dst: counter,
                src: initial,
            });
            self.function
                .semantic
                .registers
                .insert(counter.temp.index(), AbiType::Builtin(BuiltinType::USize));
            Some(counter)
        } else {
            None
        };
        let result = match operation {
            NativeDefaultMethod::Find
            | NativeDefaultMethod::FindMap
            | NativeDefaultMethod::Position
            | NativeDefaultMethod::Nth
            | NativeDefaultMethod::Last
            | NativeDefaultMethod::Reduce
            | NativeDefaultMethod::MinBy
            | NativeDefaultMethod::MaxBy
            | NativeDefaultMethod::Min
            | NativeDefaultMethod::Max
            | NativeDefaultMethod::MinByKey
            | NativeDefaultMethod::MaxByKey => {
                self.standard_enum_op(&result_optional, StandardEnumOp::Make(1), None)?
            }
            NativeDefaultMethod::Any | NativeDefaultMethod::All => self.lower_constant(
                Constant::Bool(operation == NativeDefaultMethod::All),
                ValueType::Bool,
            ),
            NativeDefaultMethod::Count => self.usize_constant(0),
            NativeDefaultMethod::Fold => {
                let result = self.alloc_temp(values[1].ty);
                self.emit(Instruction::Move {
                    dst: result,
                    src: values[1],
                });
                self.function.semantic.registers.insert(
                    result.temp.index(),
                    AbiType::from_checked_type(&arguments[0]),
                );
                result
            }
            NativeDefaultMethod::ForEach => self.lower_constant(Constant::Unit, ValueType::Unit),
            NativeDefaultMethod::Join | NativeDefaultMethod::Partition => {
                self.collection_new(&array_type)?
            }
            NativeDefaultMethod::GroupBy => self.collection_new(&TypeId::Map {
                key: Box::new(arguments[0].clone()),
                value: Box::new(array_type.clone()),
                access: Mutable,
            })?,
            _ => return Err(IrLoweringError::MissingBinding("iterator terminal")),
        };
        let rejected = if operation == NativeDefaultMethod::Partition {
            Some(self.collection_new(&array_type)?)
        } else {
            None
        };
        let key_state = if matches!(
            operation,
            NativeDefaultMethod::MinByKey | NativeDefaultMethod::MaxByKey
        ) {
            let ty = TypeId::StandardEnum {
                kind: StandardEnum::Option,
                args: vec![arguments[0].clone()],
            };
            Some((
                self.standard_enum_op(&ty, StandardEnumOp::Make(1), None)?,
                ty,
            ))
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
            NativeDefaultMethod::Join => {
                self.collection_insert(&array_type, result, item)?;
                self.ensure_jump(head);
            }
            NativeDefaultMethod::FindMap => {
                let mapped = self.call_function_value(values[1], &result_optional, &[item])?;
                let present =
                    self.standard_enum_op(&result_optional, StandardEnumOp::Test(0), Some(mapped))?;
                self.emit(Instruction::Move {
                    dst: result,
                    src: mapped,
                });
                self.set_terminator(Terminator::Branch {
                    cond: present,
                    then_block: done,
                    else_block: head,
                });
            }
            NativeDefaultMethod::Position | NativeDefaultMethod::Nth => {
                let counter = counter.unwrap();
                let matched = if operation == NativeDefaultMethod::Position {
                    self.call_function_value(values[1], &bool_type, &[item])?
                } else {
                    let zero = self.usize_constant(0);
                    let matched = self.alloc_temp(ValueType::Bool);
                    self.emit(Instruction::Binary {
                        dst: matched,
                        op: BinaryOp::Eq,
                        lhs: counter,
                        rhs: zero,
                    });
                    matched
                };
                let found = self.new_block();
                let advance = self.new_block();
                self.set_terminator(Terminator::Branch {
                    cond: matched,
                    then_block: found,
                    else_block: advance,
                });
                self.switch_to_block(found);
                let value = if operation == NativeDefaultMethod::Position {
                    self.standard_enum_op(&result_optional, StandardEnumOp::Make(0), Some(counter))?
                } else {
                    next
                };
                self.emit(Instruction::Move {
                    dst: result,
                    src: value,
                });
                self.ensure_jump(done);
                self.switch_to_block(advance);
                let one = self.usize_constant(1);
                let next_counter = self.alloc_temp(ValueType::U64);
                self.emit(Instruction::Binary {
                    dst: next_counter,
                    op: if operation == NativeDefaultMethod::Position {
                        BinaryOp::Add
                    } else {
                        BinaryOp::Sub
                    },
                    lhs: counter,
                    rhs: one,
                });
                self.emit(Instruction::Move {
                    dst: counter,
                    src: next_counter,
                });
                self.ensure_jump(head);
            }
            NativeDefaultMethod::Last => {
                self.emit(Instruction::Move {
                    dst: result,
                    src: next,
                });
                self.ensure_jump(head);
            }
            NativeDefaultMethod::Reduce
            | NativeDefaultMethod::MinBy
            | NativeDefaultMethod::MaxBy
            | NativeDefaultMethod::Min
            | NativeDefaultMethod::Max
            | NativeDefaultMethod::MinByKey
            | NativeDefaultMethod::MaxByKey => {
                let current_key = if key_state.is_some() {
                    Some(self.call_function_value(values[1], &arguments[0], &[item])?)
                } else {
                    None
                };
                let present =
                    self.standard_enum_op(&optional, StandardEnumOp::Test(0), Some(result))?;
                let combine = self.new_block();
                let replace = self.new_block();
                self.set_terminator(Terminator::Branch {
                    cond: present,
                    then_block: combine,
                    else_block: replace,
                });
                self.switch_to_block(combine);
                let previous =
                    self.standard_enum_op(&optional, StandardEnumOp::Read(0), Some(result))?;
                if operation == NativeDefaultMethod::Reduce {
                    let combined =
                        self.call_function_value(values[1], &item_type, &[previous, item])?;
                    let wrapped =
                        self.standard_enum_op(&optional, StandardEnumOp::Make(0), Some(combined))?;
                    self.emit(Instruction::Move {
                        dst: result,
                        src: wrapped,
                    });
                    self.ensure_jump(head);
                } else {
                    let ordering = TypeId::StandardEnum {
                        kind: StandardEnum::Ordering,
                        args: vec![],
                    };
                    let comparison = if matches!(
                        operation,
                        NativeDefaultMethod::MinBy | NativeDefaultMethod::MaxBy
                    ) {
                        self.call_function_value(values[1], &ordering, &[previous, item])?
                    } else {
                        let (ty, left, right) = if let Some((state, option)) = &key_state {
                            (
                                &arguments[0],
                                self.standard_enum_op(
                                    option,
                                    StandardEnumOp::Read(0),
                                    Some(*state),
                                )?,
                                current_key.unwrap(),
                            )
                        } else {
                            (&item_type, previous, item)
                        };
                        self.lower_applied_operator(
                            StandardTrait::Ord.nominal(),
                            ty.clone(),
                            &StandardTrait::Ord.contract().methods[0].id,
                            &[left, right],
                        )?
                    };
                    let greater = self.standard_enum_op(
                        &ordering,
                        StandardEnumOp::Test(2),
                        Some(comparison),
                    )?;
                    self.set_terminator(Terminator::Branch {
                        cond: greater,
                        then_block: if matches!(
                            operation,
                            NativeDefaultMethod::MinBy
                                | NativeDefaultMethod::Min
                                | NativeDefaultMethod::MinByKey
                        ) {
                            replace
                        } else {
                            head
                        },
                        else_block: if matches!(
                            operation,
                            NativeDefaultMethod::MinBy
                                | NativeDefaultMethod::Min
                                | NativeDefaultMethod::MinByKey
                        ) {
                            head
                        } else {
                            replace
                        },
                    });
                }
                self.switch_to_block(replace);
                if let Some((state, option)) = &key_state {
                    let key =
                        self.standard_enum_op(option, StandardEnumOp::Make(0), current_key)?;
                    self.emit(Instruction::Move {
                        dst: *state,
                        src: key,
                    });
                }
                self.emit(Instruction::Move {
                    dst: result,
                    src: next,
                });
                self.ensure_jump(head);
            }
            NativeDefaultMethod::Find | NativeDefaultMethod::Any | NativeDefaultMethod::All => {
                let predicate = self.call_function_value(values[1], &bool_type, &[item])?;
                let found = self.new_block();
                let (then_block, else_block) = if operation == NativeDefaultMethod::All {
                    (head, found)
                } else {
                    (found, head)
                };
                self.set_terminator(Terminator::Branch {
                    cond: predicate,
                    then_block,
                    else_block,
                });
                self.switch_to_block(found);
                let value = if operation == NativeDefaultMethod::Find {
                    next
                } else {
                    self.lower_constant(
                        Constant::Bool(operation == NativeDefaultMethod::Any),
                        ValueType::Bool,
                    )
                };
                self.emit(Instruction::Move {
                    dst: result,
                    src: value,
                });
                self.ensure_jump(done);
            }
            NativeDefaultMethod::Count => {
                let one = self.usize_constant(1);
                let next = self.alloc_temp(ValueType::U64);
                self.emit(Instruction::Binary {
                    dst: next,
                    op: BinaryOp::Add,
                    lhs: result,
                    rhs: one,
                });
                self.emit(Instruction::Move {
                    dst: result,
                    src: next,
                });
                self.ensure_jump(head);
            }
            NativeDefaultMethod::Fold => {
                let next = self.call_function_value(values[2], &arguments[0], &[result, item])?;
                self.emit(Instruction::Move {
                    dst: result,
                    src: next,
                });
                self.ensure_jump(head);
            }
            NativeDefaultMethod::ForEach => {
                self.call_function_value(values[1], &unit_type, &[item])?;
                self.ensure_jump(head);
            }
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
                let key = self.call_function_value(values[1], key_type, &[item])?;
                let custom = self.has_custom_protocol(key_type);
                let group = if custom {
                    self.lower_key_operation(StandardIntrinsic::MapGet, key_type, &[result, key])?
                } else {
                    self.emit_intrinsic(
                        StandardIntrinsic::MapGet,
                        &[result, key],
                        ValueType::HeapObject,
                    )
                };
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
                if custom {
                    self.lower_key_operation(
                        StandardIntrinsic::MapInsert,
                        key_type,
                        &[result, key, group],
                    )?;
                } else {
                    self.emit_intrinsic(
                        StandardIntrinsic::MapInsert,
                        &[result, key, group],
                        ValueType::HeapObject,
                    );
                }
                self.ensure_jump(head);
            }
            _ => unreachable!(),
        }
        self.switch_to_block(done);
        if guarded {
            self.iterator_close(source, values[0]);
            self.emit(Instruction::EndIteration);
        }
        if operation == NativeDefaultMethod::Join {
            return Ok(self.emit_intrinsic(
                StandardIntrinsic::ArrayJoin,
                &[result, values[1]],
                ValueType::Str,
            ));
        }
        if let Some(rejected) = rejected {
            let mut contract = StandardTrait::FromIterator.nominal();
            contract.arguments.push(item_type);
            let method = StandardTrait::FromIterator.contract().methods[0].id.clone();
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
