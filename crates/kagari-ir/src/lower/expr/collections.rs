use super::*;
use kagari_hir::types::TypeId;

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_collection_factory(
        &mut self,
        site: hir::ExprId,
        intrinsic: StandardIntrinsic,
        input: IrValue,
    ) -> Result<IrValue, IrLoweringError> {
        use StandardIntrinsic::*;
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
        let (new, item_type) = match &ty {
            TypeId::Array(item, _) => (MutableArrayNew, (**item).clone()),
            TypeId::Map { key, value, .. } => (
                MutableMapNew,
                TypeId::Tuple(vec![(**key).clone(), (**value).clone()]),
            ),
            TypeId::Set(item, _) => (MutableSetNew, (**item).clone()),
            _ => return Err(IrLoweringError::MissingBinding("collection factory result")),
        };
        let output = self.emit_intrinsic(new, &[], ValueType::HeapObject);
        let mutable = match &ty {
            TypeId::Array(item, _) => TypeId::Array(
                item.clone(),
                kagari_common::collection::CollectionAccess::Mutable,
            ),
            TypeId::Map { key, value, .. } => TypeId::Map {
                key: key.clone(),
                value: value.clone(),
                access: kagari_common::collection::CollectionAccess::Mutable,
            },
            TypeId::Set(item, _) => TypeId::Set(
                item.clone(),
                kagari_common::collection::CollectionAccess::Mutable,
            ),
            _ => unreachable!(),
        };
        self.function.semantic.registers.insert(
            output.temp.index(),
            crate::module::abi::AbiType::from_checked_type(&mutable),
        );
        self.emit(Instruction::BeginIteration { collection: input });
        let length = self.emit_intrinsic(ArrayLen, &[input], ValueType::I64);
        let index = self.lower_constant(Constant::I64(0), ValueType::I64);
        let head = self.new_block();
        let body = self.new_block();
        let done = self.new_block();
        self.ensure_jump(head);
        self.switch_to_block(head);
        let condition = self.alloc_temp(ValueType::Bool);
        self.emit(Instruction::Binary {
            dst: condition,
            op: BinaryOp::Lt,
            lhs: index,
            rhs: length,
        });
        self.set_terminator(Terminator::Branch {
            cond: condition,
            then_block: body,
            else_block: done,
        });
        self.switch_to_block(body);
        let item = self.alloc_temp(self.value_type(&item_type)?);
        self.emit(Instruction::ReadAggregateIndex {
            dst: item,
            base: input,
            index,
        });
        match (&ty, intrinsic) {
            (TypeId::Array(_, _), _) => {
                self.emit_intrinsic(ArrayPush, &[output, item], ValueType::HeapObject);
            }
            (TypeId::Set(key, _), _) => {
                if self.has_custom_protocol(key) {
                    self.lower_key_operation(SetInsert, key, &[output, item])?;
                } else {
                    self.emit_intrinsic(SetInsert, &[output, item], ValueType::HeapObject);
                }
            }
            (TypeId::Map { key, value, .. }, _) => {
                let first = self.lower_constant(Constant::I32(0), ValueType::I32);
                let second = self.lower_constant(Constant::I32(1), ValueType::I32);
                let key_value = self.alloc_temp(self.value_type(key)?);
                let payload = self.alloc_temp(self.value_type(value)?);
                self.emit(Instruction::ReadAggregateIndex {
                    dst: key_value,
                    base: item,
                    index: first,
                });
                self.emit(Instruction::ReadAggregateIndex {
                    dst: payload,
                    base: item,
                    index: second,
                });
                if self.has_custom_protocol(key) {
                    self.lower_key_operation(MapInsert, key, &[output, key_value, payload])?;
                } else {
                    self.emit_intrinsic(
                        MapInsert,
                        &[output, key_value, payload],
                        ValueType::HeapObject,
                    );
                }
            }
            _ => unreachable!("checked collection factory"),
        }
        let one = self.lower_constant(Constant::I64(1), ValueType::I64);
        let next = self.alloc_temp(ValueType::I64);
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
        self.emit(Instruction::EndIteration);
        Ok(output)
    }
}
