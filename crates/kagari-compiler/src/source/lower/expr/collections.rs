use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::{
    operations::IterOp,
    representation::ValueType,
    standard::{StandardIntrinsic, traits::StandardTrait},
};
use kagari_common::collection::CollectionAccess;
use kagari_hir::types::abi::lower_type;
use kagari_hir::{
    builtin::traits::{self, StandardTraitSemantics},
    hir,
    types::TypeId,
};
use kagari_mir::instruction::{Instruction, MirValue};

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
}

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
