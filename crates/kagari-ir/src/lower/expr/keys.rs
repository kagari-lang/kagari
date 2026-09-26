use super::*;
use kagari_hir::{builtin::traits::StandardTrait, types::TypeId};

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_set_algebra(
        &mut self,
        intrinsic: StandardIntrinsic,
        key_ty: &TypeId,
        args: &[IrValue],
    ) -> Result<IrValue, IrLoweringError> {
        use StandardIntrinsic::*;
        let result = self.emit_intrinsic(MutableSetNew, &[], ValueType::HeapObject);
        self.emit(Instruction::BeginIteration {
            collection: args[0],
        });
        self.emit(Instruction::BeginIteration {
            collection: args[1],
        });
        let sources = if intrinsic == SetUnion {
            args
        } else {
            &args[..1]
        };
        for source in sources {
            let values = self.emit_intrinsic(SetToArray, &[*source], ValueType::HeapObject);
            let len = self.emit_intrinsic(ArrayLen, &[values], ValueType::I64);
            let index = self.lower_constant(Constant::I64(0), ValueType::I64);
            let head = self.new_block();
            let body = self.new_block();
            let insert = self.new_block();
            let step = self.new_block();
            let done = self.new_block();
            self.set_terminator(Terminator::Jump(head));
            self.switch_to_block(head);
            let cond = self.alloc_temp(ValueType::Bool);
            self.emit(Instruction::Binary {
                dst: cond,
                op: BinaryOp::Lt,
                lhs: index,
                rhs: len,
            });
            self.set_terminator(Terminator::Branch {
                cond,
                then_block: body,
                else_block: done,
            });
            self.switch_to_block(body);
            let item = self.alloc_temp(self.value_type(key_ty)?);
            self.emit(Instruction::ReadAggregateIndex {
                dst: item,
                base: values,
                index,
            });
            if intrinsic == SetUnion {
                self.set_terminator(Terminator::Jump(insert));
            } else {
                let contains = self.lower_key_operation(SetContains, key_ty, &[args[1], item])?;
                let (then_block, else_block) = if intrinsic == SetIntersection {
                    (insert, step)
                } else {
                    (step, insert)
                };
                self.set_terminator(Terminator::Branch {
                    cond: contains,
                    then_block,
                    else_block,
                });
            }
            self.switch_to_block(insert);
            self.lower_key_operation(SetInsert, key_ty, &[result, item])?;
            self.set_terminator(Terminator::Jump(step));
            self.switch_to_block(step);
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
            self.set_terminator(Terminator::Jump(head));
            self.switch_to_block(done);
        }
        self.emit(Instruction::EndIteration);
        self.emit(Instruction::EndIteration);
        Ok(result)
    }

    pub(super) fn lower_key_operation(
        &mut self,
        intrinsic: StandardIntrinsic,
        key_ty: &TypeId,
        args: &[IrValue],
    ) -> Result<IrValue, IrLoweringError> {
        use StandardIntrinsic::*;
        let collection = args[0];
        let query = args[1];
        self.emit_intrinsic(KeyLookupBegin, &[collection], ValueType::Unit);
        let hash = self.lower_protocol(StandardTrait::Hash, key_ty, &[query], 0)?;
        let candidates =
            self.emit_intrinsic(KeyCandidates, &[collection, hash], ValueType::HeapObject);
        let len = self.emit_intrinsic(ArrayLen, &[candidates], ValueType::I64);
        let token = self.lower_constant(Constant::I64(-1), ValueType::I64);
        let index = self.lower_constant(Constant::I64(0), ValueType::I64);
        let cond_block = self.new_block();
        let body = self.new_block();
        let found = self.new_block();
        let step = self.new_block();
        let done = self.new_block();
        self.set_terminator(Terminator::Jump(cond_block));
        self.switch_to_block(cond_block);
        let cond = self.alloc_temp(ValueType::Bool);
        self.emit(Instruction::Binary {
            dst: cond,
            op: BinaryOp::Lt,
            lhs: index,
            rhs: len,
        });
        self.set_terminator(Terminator::Branch {
            cond,
            then_block: body,
            else_block: done,
        });
        self.switch_to_block(body);
        let candidate = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::ReadAggregateIndex {
            dst: candidate,
            base: candidates,
            index,
        });
        let zero = self.lower_constant(Constant::I32(0), ValueType::I32);
        let one = self.lower_constant(Constant::I32(1), ValueType::I32);
        let candidate_token = self.alloc_temp(ValueType::I64);
        self.emit(Instruction::ReadAggregateIndex {
            dst: candidate_token,
            base: candidate,
            index: zero,
        });
        let key = self.alloc_temp(self.value_type(key_ty)?);
        self.emit(Instruction::ReadAggregateIndex {
            dst: key,
            base: candidate,
            index: one,
        });
        let equal = self.lower_protocol(StandardTrait::PartialEq, key_ty, &[query, key], 0)?;
        self.set_terminator(Terminator::Branch {
            cond: equal,
            then_block: found,
            else_block: step,
        });
        self.switch_to_block(found);
        self.emit(Instruction::Move {
            dst: token,
            src: candidate_token,
        });
        self.set_terminator(Terminator::Jump(done));
        self.switch_to_block(step);
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
        self.set_terminator(Terminator::Jump(cond_block));
        self.switch_to_block(done);
        let (commit, result_ty) = match intrinsic {
            MapGet | MapContainsKey => (KeyMapGet, ValueType::HeapObject),
            MapInsert => (KeyMapInsert, ValueType::HeapObject),
            MapRemove => (KeyMapRemove, ValueType::HeapObject),
            SetContains => (KeySetContains, ValueType::Bool),
            SetInsert => (KeySetInsert, ValueType::HeapObject),
            SetRemove => (KeySetRemove, ValueType::Bool),
            _ => return Err(IrLoweringError::MissingBinding("custom key operation")),
        };
        let mut values = vec![collection, hash, token];
        if matches!(intrinsic, MapInsert | SetInsert) {
            values.extend_from_slice(&args[1..]);
        }
        let result = self.emit_intrinsic(commit, &values, result_ty);
        if intrinsic == MapContainsKey {
            Ok(self.emit_intrinsic(OptionIsSome, &[result], ValueType::Bool))
        } else {
            Ok(result)
        }
    }
}
