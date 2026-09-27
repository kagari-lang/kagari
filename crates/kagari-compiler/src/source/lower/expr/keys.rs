use crate::source::lower::MirLoweringError;
use crate::source::lower::state::FunctionLowerer;
use kagari_abi::operations::BinaryOp;
use kagari_abi::representation::ValueType;
use kagari_abi::standard::StandardIntrinsic;
use kagari_abi::standard::traits::StandardTrait;
use kagari_hir::types::TypeId;
use kagari_mir::instruction::Constant;
use kagari_mir::instruction::Instruction;
use kagari_mir::instruction::MirValue;
use kagari_mir::instruction::Terminator;

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_key_operation(
        &mut self,
        intrinsic: StandardIntrinsic,
        key_ty: &TypeId,
        args: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let collection = args[0];
        let query = args[1];
        self.emit_intrinsic(
            StandardIntrinsic::KeyLookupBegin,
            &[collection],
            ValueType::Unit,
        );
        let hash = self.lower_protocol(StandardTrait::Hash, key_ty, &[query], 0)?;
        let candidates = self.emit_intrinsic(
            StandardIntrinsic::KeyCandidates,
            &[collection, hash],
            ValueType::HeapObject,
        );
        let len = self.emit_intrinsic(StandardIntrinsic::ArrayLen, &[candidates], ValueType::U64);
        let token = self.lower_constant(Constant::I64(-1), ValueType::I64);
        let index = self.lower_constant(Constant::U64(0), ValueType::U64);
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
        let one = self.lower_constant(Constant::U64(1), ValueType::U64);
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
        self.set_terminator(Terminator::Jump(cond_block));
        self.switch_to_block(done);
        let (commit, result_ty) = match intrinsic {
            StandardIntrinsic::MapGet | StandardIntrinsic::MapContainsKey => {
                (StandardIntrinsic::KeyMapGet, ValueType::HeapObject)
            }
            StandardIntrinsic::MapInsert => {
                (StandardIntrinsic::KeyMapInsert, ValueType::HeapObject)
            }
            StandardIntrinsic::MapRemove => {
                (StandardIntrinsic::KeyMapRemove, ValueType::HeapObject)
            }
            StandardIntrinsic::SetContains => (StandardIntrinsic::KeySetContains, ValueType::Bool),
            StandardIntrinsic::SetInsert => {
                (StandardIntrinsic::KeySetInsert, ValueType::HeapObject)
            }
            StandardIntrinsic::SetRemove => (StandardIntrinsic::KeySetRemove, ValueType::Bool),
            _ => return Err(MirLoweringError::MissingBinding("custom key operation")),
        };
        let mut values = vec![collection, hash, token];
        if matches!(
            intrinsic,
            StandardIntrinsic::MapInsert | StandardIntrinsic::SetInsert
        ) {
            values.extend_from_slice(&args[1..]);
        }
        let result = self.emit_intrinsic(commit, &values, result_ty);
        if intrinsic == StandardIntrinsic::MapContainsKey {
            Ok(self.emit_intrinsic(StandardIntrinsic::OptionIsSome, &[result], ValueType::Bool))
        } else {
            Ok(result)
        }
    }
}
