use super::*;
use crate::module::{abi::AbiType, instruction::StandardEnumOp};
use kagari_common::collection::CollectionAccess::Mutable;
use kagari_hir::{
    builtin::{declarations::IteratorMethod, surface::StandardEnum, traits::StandardTrait},
    types::{BuiltinType, TypeId},
};

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_iterator_terminal(
        &mut self,
        operation: IteratorMethod,
        source: &TypeId,
        arguments: &[TypeId],
        values: &[IrValue],
    ) -> Result<IrValue, IrLoweringError> {
        use IteratorMethod::*;
        let item_type = self.iterator_item(source)?;
        let optional = TypeId::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![item_type.clone()],
        };
        let array_type = TypeId::Array(Box::new(item_type.clone()), Mutable);
        let bool_type = TypeId::Builtin(BuiltinType::Bool);
        let unit_type = TypeId::Builtin(BuiltinType::Unit);
        let result = match operation {
            Find => self.standard_enum_op(&optional, StandardEnumOp::Make(1), None)?,
            Any | All => self.lower_constant(Constant::Bool(operation == All), ValueType::Bool),
            Count => self.usize_constant(0),
            Fold => {
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
            ForEach => self.lower_constant(Constant::Unit, ValueType::Unit),
            Partition => self.collection_new(&array_type)?,
            GroupBy => self.collection_new(&TypeId::Map {
                key: Box::new(arguments[0].clone()),
                value: Box::new(array_type.clone()),
                access: Mutable,
            })?,
            _ => return Err(IrLoweringError::MissingBinding("iterator terminal")),
        };
        let rejected = if operation == Partition {
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
            Find | Any | All => {
                let predicate = self.iterator_callback(values[1], &bool_type, &[item])?;
                let found = self.new_block();
                let (then_block, else_block) = if operation == All {
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
                let value = if operation == Find {
                    next
                } else {
                    self.lower_constant(Constant::Bool(operation == Any), ValueType::Bool)
                };
                self.emit(Instruction::Move {
                    dst: result,
                    src: value,
                });
                self.ensure_jump(done);
            }
            Count => {
                let one = self.usize_constant(1);
                let next = self.alloc_temp(ValueType::I64);
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
            Fold => {
                let next = self.iterator_callback(values[2], &arguments[0], &[result, item])?;
                self.emit(Instruction::Move {
                    dst: result,
                    src: next,
                });
                self.ensure_jump(head);
            }
            ForEach => {
                self.iterator_callback(values[1], &unit_type, &[item])?;
                self.ensure_jump(head);
            }
            Partition => {
                let predicate = self.iterator_callback(values[1], &bool_type, &[item])?;
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
            GroupBy => {
                let key_type = &arguments[0];
                let key = self.iterator_callback(values[1], key_type, &[item])?;
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
        if let Some(rejected) = rejected {
            let mut contract = StandardTrait::FromIterator.nominal();
            contract.arguments.push(item_type);
            let method = StandardTrait::FromIterator.contract().methods[0].id.clone();
            let left = self.lower_applied_method(
                contract.clone(),
                arguments[0].clone(),
                &method,
                std::slice::from_ref(&array_type),
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
