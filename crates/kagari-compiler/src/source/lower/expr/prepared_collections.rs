use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::{
    operations::{BinaryOp, StandardEnumOp as Op},
    representation::ValueType,
    scalar::BuiltinType,
    standard::{StandardIntrinsic, surface::StandardEnum, traits::StandardTrait},
};
use kagari_common::collection::CollectionAccess;
use kagari_hir::{builtin::traits::StandardTraitSemantics, types::TypeId};
use kagari_mir::instruction::{Constant, Instruction, MirValue, Terminator};

fn array(item: TypeId) -> TypeId {
    TypeId::Array(Box::new(item), CollectionAccess::Mutable)
}

impl FunctionLowerer<'_, '_> {
    pub(super) fn prepared_read(
        &mut self,
        source: MirValue,
        index: MirValue,
        ty: &TypeId,
    ) -> Result<MirValue, MirLoweringError> {
        let dst = self.alloc_temp(self.value_type(ty)?);
        self.emit(Instruction::ReadAggregateIndex {
            dst,
            base: source,
            index,
        });
        Ok(dst)
    }
    pub(super) fn prepared_field(
        &mut self,
        value: MirValue,
        field: u64,
        ty: &TypeId,
    ) -> Result<MirValue, MirLoweringError> {
        let index = self.usize_constant(field);
        self.prepared_read(value, index, ty)
    }
    fn increment(&mut self, index: MirValue) {
        let one = self.usize_constant(1);
        let next = self.query_binary(BinaryOp::Add, index, one, ValueType::U64);
        self.emit(Instruction::Move {
            dst: index,
            src: next,
        });
    }
    fn prepared_while(
        &mut self,
        condition: impl FnOnce(&mut Self) -> Result<MirValue, MirLoweringError>,
        body: impl FnOnce(&mut Self) -> Result<(), MirLoweringError>,
    ) -> Result<(), MirLoweringError> {
        let head = self.new_block();
        let work = self.new_block();
        let done = self.new_block();
        self.ensure_jump(head);
        self.switch_to_block(head);
        let cond = condition(self)?;
        self.set_terminator(Terminator::Branch {
            cond,
            then_block: work,
            else_block: done,
        });
        self.switch_to_block(work);
        body(self)?;
        self.ensure_jump(head);
        self.switch_to_block(done);
        Ok(())
    }
    pub(super) fn prepared_indices(
        &mut self,
        length: MirValue,
        body: impl FnOnce(&mut Self, MirValue) -> Result<(), MirLoweringError>,
    ) -> Result<(), MirLoweringError> {
        let index = self.usize_constant(0);
        self.prepared_while(
            |this| Ok(this.query_binary(BinaryOp::Lt, index, length, ValueType::Bool)),
            |this| {
                body(this, index)?;
                this.increment(index);
                Ok(())
            },
        )
    }

    pub(super) fn lower_prepared_collection(
        &mut self,
        operation: StandardIntrinsic,
        source: &TypeId,
        args: &[MirValue],
        callback: Option<&TypeId>,
    ) -> Result<MirValue, MirLoweringError> {
        self.emit_intrinsic(
            StandardIntrinsic::CollectionMutationBegin,
            &[args[0]],
            ValueType::Unit,
        );
        let item = self.iteration_output(StandardTrait::Iterable, source, "Item")?;
        let prepared = if matches!(
            operation,
            StandardIntrinsic::ArraySort
                | StandardIntrinsic::ArraySortBy
                | StandardIntrinsic::ArraySortByKey
        ) {
            self.prepare_sort(operation, source, &item, args, callback)?
        } else if operation == StandardIntrinsic::ArrayDedup {
            self.prepare_dedup(&item, args[0])?
        } else {
            self.prepare_retain(source, &item, args)?
        };
        self.emit_intrinsic(
            StandardIntrinsic::CollectionMutationEnd,
            &[args[0]],
            ValueType::Unit,
        );
        Ok(self.emit_intrinsic(
            if matches!(
                operation,
                StandardIntrinsic::ArraySort
                    | StandardIntrinsic::ArraySortBy
                    | StandardIntrinsic::ArraySortByKey
            ) {
                StandardIntrinsic::ArrayReplaceStorage
            } else {
                StandardIntrinsic::CollectionRetainStorage
            },
            &[args[0], prepared],
            ValueType::Unit,
        ))
    }

    fn prepare_retain(
        &mut self,
        source: &TypeId,
        item: &TypeId,
        args: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let boolean = TypeId::Builtin(BuiltinType::Bool);
        let mask_type = array(boolean.clone());
        let mask = self.collection_new(&mask_type)?;
        let guard = self.query_guard(source, args[0])?;
        let optional = TypeId::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![item.clone()],
        };
        let next = self.alloc_temp(ValueType::HeapObject);
        self.prepared_while(
            |this| {
                let step = this.iterator_next(&guard.0, guard.1)?;
                this.emit(Instruction::Move {
                    dst: next,
                    src: step,
                });
                this.standard_enum_op(&optional, Op::Test(0), Some(next))
            },
            |this| {
                let value = this.standard_enum_op(&optional, Op::Read(0), Some(next))?;
                let arguments = if let TypeId::Map { key, value: ty, .. } = source {
                    vec![
                        this.prepared_field(value, 0, key)?,
                        this.prepared_field(value, 1, ty)?,
                    ]
                } else {
                    vec![value]
                };
                let keep = this.call_function_value(args[1], &boolean, &arguments)?;
                this.collection_insert(&mask_type, mask, keep)
            },
        )?;
        self.end_query_guard(guard);
        Ok(mask)
    }

    fn prepare_dedup(
        &mut self,
        item: &TypeId,
        source: MirValue,
    ) -> Result<MirValue, MirLoweringError> {
        let boolean = TypeId::Builtin(BuiltinType::Bool);
        let mask_type = array(boolean.clone());
        let mask = self.collection_new(&mask_type)?;
        let len = self.emit_intrinsic(StandardIntrinsic::ArrayLen, &[source], ValueType::U64);
        let previous = self.usize_constant(0);
        let zero = self.usize_constant(0);
        self.prepared_indices(len, |this, index| {
            let first = this.query_binary(BinaryOp::Eq, index, zero, ValueType::Bool);
            let keep = this.branch_enum_value(
                first,
                &boolean,
                |this| Ok(this.lower_constant(Constant::Bool(true), ValueType::Bool)),
                |this| {
                    let left = this.prepared_read(source, previous, item)?;
                    let right = this.prepared_read(source, index, item)?;
                    let equal =
                        this.lower_protocol(StandardTrait::PartialEq, item, &[left, right], 0)?;
                    let no = this.lower_constant(Constant::Bool(false), ValueType::Bool);
                    Ok(this.query_binary(BinaryOp::Eq, equal, no, ValueType::Bool))
                },
            )?;
            this.branch_enum_value(
                keep,
                &TypeId::Builtin(BuiltinType::Unit),
                |this| {
                    this.emit(Instruction::Move {
                        dst: previous,
                        src: index,
                    });
                    Ok(this.lower_unit())
                },
                |this| Ok(this.lower_unit()),
            )?;
            this.collection_insert(&mask_type, mask, keep)
        })?;
        Ok(mask)
    }

    pub(super) fn sort_bound(
        &mut self,
        start: MirValue,
        width: MirValue,
        len: MirValue,
    ) -> Result<MirValue, MirLoweringError> {
        let remaining = self.query_binary(BinaryOp::Sub, len, start, ValueType::U64);
        let short = self.query_binary(BinaryOp::Lt, remaining, width, ValueType::Bool);
        self.branch_enum_value(
            short,
            &TypeId::Builtin(BuiltinType::USize),
            |_| Ok(len),
            |this| Ok(this.query_binary(BinaryOp::Add, start, width, ValueType::U64)),
        )
    }

    fn prepare_sort(
        &mut self,
        operation: StandardIntrinsic,
        source: &TypeId,
        item: &TypeId,
        args: &[MirValue],
        callback: Option<&TypeId>,
    ) -> Result<MirValue, MirLoweringError> {
        let key = if let Some(TypeId::Function { result, .. }) = callback {
            Some(&**result)
        } else {
            None
        };
        let decorated = key.map_or_else(
            || item.clone(),
            |key| TypeId::Tuple(vec![key.clone(), item.clone()]),
        );
        let storage = array(decorated.clone());
        let values = self.collection_new(&storage)?;
        let len = self.emit_intrinsic(StandardIntrinsic::ArrayLen, &[args[0]], ValueType::U64);
        let guard = self.query_guard(source, args[0])?;
        self.prepared_indices(len, |this, index| {
            let value = this.prepared_read(args[0], index, item)?;
            let value = if let Some(key) = key {
                let extracted = this.call_function_value(args[1], key, &[value])?;
                let pair = this.alloc_temp(ValueType::HeapObject);
                this.emit(Instruction::MakeTuple {
                    dst: pair,
                    elements: vec![extracted, value].into(),
                });
                pair
            } else {
                value
            };
            this.collection_insert(&storage, values, value)
        })?;
        let width = self.usize_constant(1);
        let ordering = TypeId::StandardEnum {
            kind: StandardEnum::Ordering,
            args: vec![],
        };
        let boolean = TypeId::Builtin(BuiltinType::Bool);
        self.prepared_while(
            |this| Ok(this.query_binary(BinaryOp::Lt, width, len, ValueType::Bool)),
            |this| {
                let output = this.collection_new(&storage)?;
                let start = this.usize_constant(0);
                this.prepared_while(
                    |this| Ok(this.query_binary(BinaryOp::Lt, start, len, ValueType::Bool)),
                    |this| {
                        let middle = this.sort_bound(start, width, len)?;
                        let end = this.sort_bound(middle, width, len)?;
                        let left = this.alloc_temp(ValueType::U64);
                        let right = this.alloc_temp(ValueType::U64);
                        this.emit(Instruction::Move {
                            dst: left,
                            src: start,
                        });
                        this.emit(Instruction::Move {
                            dst: right,
                            src: middle,
                        });
                        this.prepared_while(
                            |this| {
                                let a =
                                    this.query_binary(BinaryOp::Lt, left, middle, ValueType::Bool);
                                let b =
                                    this.query_binary(BinaryOp::Lt, right, end, ValueType::Bool);
                                this.branch_enum_value(
                                    a,
                                    &boolean,
                                    |this| {
                                        Ok(this
                                            .lower_constant(Constant::Bool(true), ValueType::Bool))
                                    },
                                    |_| Ok(b),
                                )
                            },
                            |this| {
                                let left_available =
                                    this.query_binary(BinaryOp::Lt, left, middle, ValueType::Bool);
                                let take_left = this.branch_enum_value(
                                    left_available,
                                    &boolean,
                                    |this| {
                                        let right_done = this.query_binary(
                                            BinaryOp::Eq,
                                            right,
                                            end,
                                            ValueType::Bool,
                                        );
                                        this.branch_enum_value(
                                            right_done,
                                            &boolean,
                                            |this| {
                                                Ok(this.lower_constant(
                                                    Constant::Bool(true),
                                                    ValueType::Bool,
                                                ))
                                            },
                                            |this| {
                                                let a =
                                                    this.prepared_read(values, left, &decorated)?;
                                                let b =
                                                    this.prepared_read(values, right, &decorated)?;
                                                let compared = if operation
                                                    == StandardIntrinsic::ArraySortBy
                                                {
                                                    this.call_function_value(
                                                        args[1],
                                                        &ordering,
                                                        &[a, b],
                                                    )?
                                                } else {
                                                    let (a, b, comparison_type) =
                                                        if let Some(key) = key {
                                                            (
                                                                this.prepared_field(a, 0, key)?,
                                                                this.prepared_field(b, 0, key)?,
                                                                key,
                                                            )
                                                        } else {
                                                            (a, b, item)
                                                        };
                                                    this.lower_applied_operator(
                                                        StandardTrait::Ord.nominal(),
                                                        comparison_type.clone(),
                                                        &StandardTrait::Ord.contract().methods[0]
                                                            .id,
                                                        &[a, b],
                                                    )?
                                                };
                                                let greater = this.standard_enum_op(
                                                    &ordering,
                                                    Op::Test(2),
                                                    Some(compared),
                                                )?;
                                                let no = this.lower_constant(
                                                    Constant::Bool(false),
                                                    ValueType::Bool,
                                                );
                                                Ok(this.query_binary(
                                                    BinaryOp::Eq,
                                                    greater,
                                                    no,
                                                    ValueType::Bool,
                                                ))
                                            },
                                        )
                                    },
                                    |this| {
                                        Ok(this
                                            .lower_constant(Constant::Bool(false), ValueType::Bool))
                                    },
                                )?;
                                let selected = this.branch_enum_value(
                                    take_left,
                                    &decorated,
                                    |this| {
                                        let value = this.prepared_read(values, left, &decorated)?;
                                        this.increment(left);
                                        Ok(value)
                                    },
                                    |this| {
                                        let value =
                                            this.prepared_read(values, right, &decorated)?;
                                        this.increment(right);
                                        Ok(value)
                                    },
                                )?;
                                this.collection_insert(&storage, output, selected)
                            },
                        )?;
                        this.emit(Instruction::Move {
                            dst: start,
                            src: end,
                        });
                        Ok(())
                    },
                )?;
                this.emit(Instruction::Move {
                    dst: values,
                    src: output,
                });
                let next_width = this.sort_bound(width, width, len)?;
                this.emit(Instruction::Move {
                    dst: width,
                    src: next_width,
                });
                Ok(())
            },
        )?;
        self.end_query_guard(guard);
        if key.is_none() {
            return Ok(values);
        }
        let output_type = array(item.clone());
        let output = self.collection_new(&output_type)?;
        self.prepared_indices(len, |this, index| {
            let decorated = this.prepared_read(values, index, &decorated)?;
            let value = this.prepared_field(decorated, 1, item)?;
            this.collection_insert(&output_type, output, value)
        })?;
        Ok(output)
    }
}
