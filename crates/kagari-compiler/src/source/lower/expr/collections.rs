use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::{operations::IterOp, representation::ValueType, standard::StandardIntrinsic};
use kagari_hir::types::{TypeId, abi::lower_type};
use kagari_mir::instruction::{Instruction, MirValue};

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_native_collection_method(
        &mut self,
        ty: &TypeId,
        name: &str,
        args: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
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
            (TypeId::Array(_, _), "clear") => {
                (StandardIntrinsic::ArrayClear, ValueType::HeapObject, true)
            }
            (TypeId::Map { .. }, "len") => (StandardIntrinsic::MapLen, ValueType::U64, false),
            (TypeId::Map { .. }, "is_empty") => {
                (StandardIntrinsic::MapIsEmpty, ValueType::Bool, false)
            }
            (TypeId::Map { .. }, "clear") => {
                (StandardIntrinsic::MapClear, ValueType::HeapObject, true)
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
