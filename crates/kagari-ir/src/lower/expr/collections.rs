use super::*;
use kagari_hir::types::TypeId;

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_map_snapshot(
        &mut self,
        site: hir::ExprId,
        intrinsic: StandardIntrinsic,
        map: IrValue,
    ) -> Result<IrValue, IrLoweringError> {
        let result = self
            .analyzed
            .typed
            .type_table
            .expr_type(site)
            .ok_or(IrLoweringError::MissingExprType(site))?;
        let span = self.analyzed.lowered.source_map.expr_span(site);
        let result = self
            .planner
            .arguments(&[result], &self.instance.substitution, span)?
            .remove(0);
        let TypeId::Trait(interface) = result else {
            return Err(IrLoweringError::MissingBinding("snapshot list interface"));
        };
        let [item] = interface.arguments.as_slice() else {
            return Err(IrLoweringError::MissingBinding("snapshot element type"));
        };
        let storage = TypeId::Array(
            Box::new(item.clone()),
            kagari_common::collection::CollectionAccess::Mutable,
        );
        self.planner
            .require_parent_interfaces(&storage, &interface, span)?;
        let implementation = self.planner.native_interface(&storage, &interface, span)?;
        let raw = match intrinsic {
            StandardIntrinsic::MapKeys => StandardIntrinsic::MapKeysStorage,
            StandardIntrinsic::MapValues => StandardIntrinsic::MapValuesStorage,
            StandardIntrinsic::MapEntries => StandardIntrinsic::MapEntriesStorage,
            _ => return Err(IrLoweringError::MissingBinding("map snapshot operation")),
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
        input: IrValue,
    ) -> Result<IrValue, IrLoweringError> {
        let ty = self
            .analyzed
            .typed
            .type_table
            .expr_type(site)
            .ok_or(IrLoweringError::MissingExprType(site))?;
        let span = self.analyzed.lowered.source_map.expr_span(site);
        let ty = self
            .planner
            .arguments(&[ty], &self.instance.substitution, span)?
            .remove(0);
        let item = kagari_hir::builtin::traits::collection_item(&ty)
            .ok_or(IrLoweringError::MissingBinding("collection factory result"))?;
        let mut interface = kagari_hir::builtin::traits::StandardTrait::List.nominal();
        interface.arguments.push(item);
        let source = TypeId::Trait(interface);
        self.lower_collect(&ty, &source, input)
    }
    pub(super) fn lower_array_from_fn(
        &mut self,
        site: hir::ExprId,
        count: IrValue,
        callback: IrValue,
    ) -> Result<IrValue, IrLoweringError> {
        let ty = self
            .analyzed
            .typed
            .type_table
            .expr_type(site)
            .ok_or(IrLoweringError::MissingExprType(site))?;
        let span = self.analyzed.lowered.source_map.expr_span(site);
        let ty = self
            .planner
            .arguments(&[ty], &self.instance.substitution, span)?
            .remove(0);
        let TypeId::Array(item, _) = &ty else {
            return Err(IrLoweringError::MissingBinding("array factory result"));
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
        let value = self.iterator_callback(callback, item, &[index])?;
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
        args: &[IrValue],
    ) -> Result<IrValue, IrLoweringError> {
        use StandardIntrinsic::*;
        if name == "iter" {
            let dst = self.alloc_temp(ValueType::HeapObject);
            self.emit(Instruction::Iter {
                dst,
                value: Some(args[0]),
                ty: crate::module::abi::AbiType::from_checked_type(ty),
                op: crate::module::instruction::IterOp::New,
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
            (TypeId::Array(_, _), "len") => (ArrayLen, ValueType::U64, false),
            (TypeId::Array(_, _), "is_empty") => (ArrayIsEmpty, ValueType::Bool, false),
            (TypeId::Array(_, _), "get") => (ArrayGet, ValueType::HeapObject, false),
            (TypeId::Array(_, _), "push") => (ArrayPush, ValueType::HeapObject, true),
            (TypeId::Array(_, _), "pop") => (ArrayPop, ValueType::HeapObject, false),
            (TypeId::Array(_, _), "insert") => (ArrayInsert, ValueType::HeapObject, true),
            (TypeId::Array(_, _), "remove") => (ArrayRemove, ValueType::HeapObject, false),
            (TypeId::Array(_, _), "clear") => (ArrayClear, ValueType::HeapObject, true),
            (TypeId::Map { key, .. }, "get" | "contains_key" | "insert" | "remove") => {
                let intrinsic = match name {
                    "get" => MapGet,
                    "contains_key" => MapContainsKey,
                    "insert" => MapInsert,
                    _ => MapRemove,
                };
                let result = if self.has_custom_protocol(key) {
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
            (TypeId::Map { .. }, "len") => (MapLen, ValueType::U64, false),
            (TypeId::Map { .. }, "is_empty") => (MapIsEmpty, ValueType::Bool, false),
            (TypeId::Map { .. }, "clear") => (MapClear, ValueType::HeapObject, true),
            (TypeId::Set(item, _), "contains" | "insert" | "remove") => {
                let intrinsic = match name {
                    "contains" => SetContains,
                    "insert" => SetInsert,
                    _ => SetRemove,
                };
                let result = if self.has_custom_protocol(item) {
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
            (TypeId::Set(_, _), "len") => (SetLen, ValueType::U64, false),
            (TypeId::Set(_, _), "is_empty") => (SetIsEmpty, ValueType::Bool, false),
            (TypeId::Set(_, _), "clear") => (SetClear, ValueType::HeapObject, true),
            _ => return Err(IrLoweringError::MissingBinding("native collection method")),
        };
        let result = self.emit_intrinsic(intrinsic, args, output);
        Ok(if discard { self.lower_unit() } else { result })
    }
}

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_list_copy(
        &mut self,
        source: TypeId,
        destination: IrValue,
        input: IrValue,
    ) -> Result<IrValue, IrLoweringError> {
        let TypeId::Trait(interface) = &source else {
            return Err(IrLoweringError::MissingBinding("list copy source"));
        };
        let storage = TypeId::Array(
            Box::new(interface.arguments[0].clone()),
            kagari_common::collection::CollectionAccess::Mutable,
        );
        let snapshot = self.lower_collect(&storage, &source, input)?;
        Ok(self.emit_intrinsic(
            StandardIntrinsic::ArrayCopyFromStorage,
            &[destination, snapshot],
            ValueType::Unit,
        ))
    }
}
