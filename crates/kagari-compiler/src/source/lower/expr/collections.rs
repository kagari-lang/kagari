use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::{
    operations::{BinaryOp, IterOp, StandardEnumOp},
    representation::ValueType,
    standard::{
        StandardIntrinsic, bindings::NativeDefaultMethod, surface::StandardEnum,
        traits::StandardTrait,
    },
};
use kagari_common::collection::CollectionAccess;
use kagari_hir::types::abi::lower_type;
use kagari_hir::{
    builtin::traits::{self, StandardTraitSemantics},
    hir,
    types::TypeId,
};
use kagari_mir::instruction::{Instruction, MirValue, Terminator};

impl FunctionLowerer<'_, '_> {
    pub(super) fn readonly_array(
        &mut self,
        item: TypeId,
        array: MirValue,
    ) -> Result<MirValue, MirLoweringError> {
        let mut interface = StandardTrait::List.nominal();
        interface.arguments.push(item.clone());
        let storage = TypeId::Array(Box::new(item), CollectionAccess::Mutable);
        let span = self.function.debug.source_span;
        self.planner
            .require_parent_interfaces(&storage, &interface, span)?;
        let implementation = self.planner.native_interface(&storage, &interface, span)?;
        let dst = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::MakeInterface {
            dst,
            value: array,
            implementation,
            arguments: vec![],
        });
        Ok(dst)
    }

    pub(super) fn lower_map_view_snapshot(
        &mut self,
        operation: NativeDefaultMethod,
        source: &TypeId,
        value: MirValue,
    ) -> Result<MirValue, MirLoweringError> {
        let iter_type = self.iteration_output(StandardTrait::Iterable, source, "Iter")?;
        let pair = self.iterator_item(&iter_type)?;
        let TypeId::Tuple(fields) = &pair else {
            return Err(MirLoweringError::MissingBinding("map entry tuple"));
        };
        let index = match operation {
            NativeDefaultMethod::MapKeysView => Some(0),
            NativeDefaultMethod::MapValuesView => Some(1),
            NativeDefaultMethod::MapEntriesView => None,
            _ => unreachable!(),
        };
        let item = index.map_or_else(|| pair.clone(), |i| fields[i].clone());
        let array_type = TypeId::Array(Box::new(item.clone()), CollectionAccess::Mutable);
        let array = self.collection_new(&array_type)?;
        let iterator = self.lower_applied_operator(
            StandardTrait::Iterable.nominal(),
            source.clone(),
            &self.protocol_method(StandardTrait::Iterable, 0)?,
            &[value],
        )?;
        let optional = TypeId::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![pair],
        };
        self.emit(Instruction::BeginIteration {
            collection: iterator,
        });
        let head = self.new_block();
        let body = self.new_block();
        let done = self.new_block();
        self.ensure_jump(head);
        self.switch_to_block(head);
        let next = self.iterator_next(&iter_type, iterator)?;
        let present = self.standard_enum_op(&optional, StandardEnumOp::Test(0), Some(next))?;
        self.set_terminator(Terminator::Branch {
            cond: present,
            then_block: body,
            else_block: done,
        });
        self.switch_to_block(body);
        let pair = self.standard_enum_op(&optional, StandardEnumOp::Read(0), Some(next))?;
        let selected = if let Some(index) = index {
            let index = self.usize_constant(index as u64);
            let dst = self.alloc_temp(self.value_type(&item)?);
            self.emit(Instruction::ReadAggregateIndex {
                dst,
                base: pair,
                index,
            });
            dst
        } else {
            pair
        };
        self.collection_insert(&array_type, array, selected)?;
        self.ensure_jump(head);
        self.switch_to_block(done);
        self.iterator_close(&iter_type, iterator);
        self.emit(Instruction::EndIteration);
        self.readonly_array(item, array)
    }

    pub(super) fn lower_map_snapshot(
        &mut self,
        site: hir::ExprId,
        intrinsic: StandardIntrinsic,
        map: MirValue,
    ) -> Result<MirValue, MirLoweringError> {
        let result = self
            .analyzed
            .typed
            .type_table
            .expr_type(site)
            .ok_or(MirLoweringError::MissingExprType(site))?;
        let span = self.analyzed.lowered.source_map.expr_span(site);
        let result = self
            .planner
            .arguments(&[result], &self.instance.substitution, span)?
            .remove(0);
        let TypeId::Trait(interface) = result else {
            return Err(MirLoweringError::MissingBinding("snapshot list interface"));
        };
        let [item] = interface.arguments.as_slice() else {
            return Err(MirLoweringError::MissingBinding("snapshot element type"));
        };
        let storage = TypeId::Array(Box::new(item.clone()), CollectionAccess::Mutable);
        self.planner
            .require_parent_interfaces(&storage, &interface, span)?;
        let implementation = self.planner.native_interface(&storage, &interface, span)?;
        let raw = match intrinsic {
            StandardIntrinsic::MapKeys => StandardIntrinsic::MapKeysStorage,
            StandardIntrinsic::MapValues => StandardIntrinsic::MapValuesStorage,
            StandardIntrinsic::MapEntries => StandardIntrinsic::MapEntriesStorage,
            _ => return Err(MirLoweringError::MissingBinding("map snapshot operation")),
        };
        let snapshot = self.emit_intrinsic(raw, &[map], ValueType::HeapObject);
        let dst = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::MakeInterface {
            dst,
            value: snapshot,
            implementation,
            arguments: Vec::new(),
        });
        Ok(dst)
    }

    pub(super) fn lower_collection_factory(
        &mut self,
        site: hir::ExprId,
        input: MirValue,
    ) -> Result<MirValue, MirLoweringError> {
        let ty = self
            .analyzed
            .typed
            .type_table
            .expr_type(site)
            .ok_or(MirLoweringError::MissingExprType(site))?;
        let span = self.analyzed.lowered.source_map.expr_span(site);
        let ty = self
            .planner
            .arguments(&[ty], &self.instance.substitution, span)?
            .remove(0);
        let item = traits::collection_item(&ty).ok_or(MirLoweringError::MissingBinding(
            "collection factory result",
        ))?;
        let mut interface = StandardTrait::List.nominal();
        interface.arguments.push(item);
        let source = TypeId::Trait(interface);
        self.lower_collect(&ty, &source, input)
    }
    pub(super) fn lower_array_from_fn(
        &mut self,
        site: hir::ExprId,
        count: MirValue,
        callback: MirValue,
    ) -> Result<MirValue, MirLoweringError> {
        let ty = self
            .analyzed
            .typed
            .type_table
            .expr_type(site)
            .ok_or(MirLoweringError::MissingExprType(site))?;
        let span = self.analyzed.lowered.source_map.expr_span(site);
        let ty = self
            .planner
            .arguments(&[ty], &self.instance.substitution, span)?
            .remove(0);
        let TypeId::Array(item, _) = &ty else {
            return Err(MirLoweringError::MissingBinding("array factory result"));
        };
        let array = self.collection_new(&ty)?;
        let index = self.usize_constant(0);
        let head = self.new_block();
        let body = self.new_block();
        let done = self.new_block();
        self.ensure_jump(head);
        self.switch_to_block(head);
        let more = self.alloc_temp(ValueType::Bool);
        self.emit(Instruction::Binary {
            dst: more,
            op: BinaryOp::Lt,
            lhs: index,
            rhs: count,
        });
        self.set_terminator(Terminator::Branch {
            cond: more,
            then_block: body,
            else_block: done,
        });
        self.switch_to_block(body);
        let value = self.call_function_value(callback, item, &[index])?;
        self.collection_insert(&ty, array, value)?;
        let one = self.usize_constant(1);
        let next = self.alloc_temp(ValueType::U64);
        self.emit(Instruction::Binary {
            dst: next,
            op: BinaryOp::Add,
            lhs: index,
            rhs: one,
        });
        self.emit(Instruction::Move {
            dst: index,
            src: next,
        });
        self.ensure_jump(head);
        self.switch_to_block(done);
        Ok(array)
    }
}

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_native_collection_method(
        &mut self,
        ty: &TypeId,
        name: &str,
        args: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        if matches!(name, "get_or_insert_with" | "update") {
            return self.lower_map_update(
                if name == "update" {
                    StandardIntrinsic::MapUpdate
                } else {
                    StandardIntrinsic::MapGetOrInsertWith
                },
                ty,
                args,
            );
        }
        if name == "iter" {
            let dst = self.alloc_temp(ValueType::HeapObject);
            self.emit(Instruction::Iter {
                dst,
                value: Some(args[0]),
                ty: lower_type(ty),
                op: IterOp::New,
            });
            return Ok(dst);
        }
        if name == "set" {
            self.emit(Instruction::WriteAggregateIndex {
                base: args[0],
                index: args[1],
                value: args[2],
            });
            return Ok(self.lower_unit());
        }
        let (intrinsic, output, discard) = match (ty, name) {
            (TypeId::Array(_, _), "len") => (StandardIntrinsic::ArrayLen, ValueType::U64, false),
            (TypeId::Array(_, _), "is_empty") => {
                (StandardIntrinsic::ArrayIsEmpty, ValueType::Bool, false)
            }
            (TypeId::Array(_, _), "get") => {
                (StandardIntrinsic::ArrayGet, ValueType::HeapObject, false)
            }
            (TypeId::Array(_, _), "push") => {
                (StandardIntrinsic::ArrayPush, ValueType::HeapObject, true)
            }
            (TypeId::Array(_, _), "pop") => {
                (StandardIntrinsic::ArrayPop, ValueType::HeapObject, false)
            }
            (TypeId::Array(_, _), "insert") => {
                (StandardIntrinsic::ArrayInsert, ValueType::HeapObject, true)
            }
            (TypeId::Array(_, _), "remove") => {
                (StandardIntrinsic::ArrayRemove, ValueType::HeapObject, false)
            }
            (TypeId::Array(_, _), "swap") => (StandardIntrinsic::ArraySwap, ValueType::Unit, false),
            (TypeId::Array(_, _), "reverse") => {
                (StandardIntrinsic::ArrayReverse, ValueType::Unit, false)
            }
            (TypeId::Array(_, _), "truncate") => {
                (StandardIntrinsic::ArrayTruncate, ValueType::Unit, false)
            }
            (TypeId::Array(item, _), "extend") => {
                let mut interface = StandardTrait::List.nominal();
                interface.arguments.push((**item).clone());
                return self.lower_list_copy(TypeId::Trait(interface), args[0], args[1], true);
            }
            (TypeId::Array(_, _), "clear") => {
                (StandardIntrinsic::ArrayClear, ValueType::HeapObject, true)
            }
            (TypeId::Map { key, .. }, "get" | "contains_key" | "insert" | "remove") => {
                let intrinsic = match name {
                    "get" => StandardIntrinsic::MapGet,
                    "contains_key" => StandardIntrinsic::MapContainsKey,
                    "insert" => StandardIntrinsic::MapInsert,
                    _ => StandardIntrinsic::MapRemove,
                };
                let result = if self.has_custom_protocol(key)? {
                    self.lower_key_operation(intrinsic, key, args)?
                } else {
                    self.emit_intrinsic(
                        intrinsic,
                        args,
                        if name == "contains_key" {
                            ValueType::Bool
                        } else {
                            ValueType::HeapObject
                        },
                    )
                };
                return Ok(if name == "insert" {
                    self.lower_unit()
                } else {
                    result
                });
            }
            (TypeId::Map { .. }, "len") => (StandardIntrinsic::MapLen, ValueType::U64, false),
            (TypeId::Map { .. }, "is_empty") => {
                (StandardIntrinsic::MapIsEmpty, ValueType::Bool, false)
            }
            (TypeId::Map { .. }, "clear") => {
                (StandardIntrinsic::MapClear, ValueType::HeapObject, true)
            }
            (TypeId::Set(item, _), "contains" | "insert" | "remove") => {
                let intrinsic = match name {
                    "contains" => StandardIntrinsic::SetContains,
                    "insert" => StandardIntrinsic::SetInsert,
                    _ => StandardIntrinsic::SetRemove,
                };
                let result = if self.has_custom_protocol(item)? {
                    self.lower_key_operation(intrinsic, item, args)?
                } else {
                    self.emit_intrinsic(
                        intrinsic,
                        args,
                        if name == "insert" {
                            ValueType::HeapObject
                        } else {
                            ValueType::Bool
                        },
                    )
                };
                return Ok(if name == "insert" {
                    self.lower_unit()
                } else {
                    result
                });
            }
            (TypeId::Set(_, _), "len") => (StandardIntrinsic::SetLen, ValueType::U64, false),
            (TypeId::Set(_, _), "is_empty") => {
                (StandardIntrinsic::SetIsEmpty, ValueType::Bool, false)
            }
            (TypeId::Set(_, _), "clear") => {
                (StandardIntrinsic::SetClear, ValueType::HeapObject, true)
            }
            _ => return Err(MirLoweringError::MissingBinding("native collection method")),
        };
        let result = self.emit_intrinsic(intrinsic, args, output);
        Ok(if discard { self.lower_unit() } else { result })
    }
}

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_list_copy(
        &mut self,
        source: TypeId,
        destination: MirValue,
        input: MirValue,
        extend: bool,
    ) -> Result<MirValue, MirLoweringError> {
        let TypeId::Trait(interface) = &source else {
            return Err(MirLoweringError::MissingBinding("list copy source"));
        };
        let storage = TypeId::Array(
            Box::new(interface.arguments[0].clone()),
            CollectionAccess::Mutable,
        );
        let snapshot = self.lower_collect(&storage, &source, input)?;
        Ok(self.emit_intrinsic(
            if extend {
                StandardIntrinsic::ArrayExtendStorage
            } else {
                StandardIntrinsic::ArrayCopyFromStorage
            },
            &[destination, snapshot],
            ValueType::Unit,
        ))
    }
}
