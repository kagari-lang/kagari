use super::*;
use crate::module::instruction::StandardEnumOp as Op;
use kagari_hir::{builtin::surface::StandardEnum, types::TypeId};

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_map_update(
        &mut self,
        operation: StandardIntrinsic,
        source: &TypeId,
        args: &[IrValue],
    ) -> Result<IrValue, IrLoweringError> {
        use StandardIntrinsic::*;
        let TypeId::Map { value, .. } = source else {
            return Err(IrLoweringError::MissingBinding("map update storage"));
        };
        let optional = TypeId::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![(**value).clone()],
        };
        self.emit_intrinsic(CollectionMutationBegin, &[args[0]], ValueType::Unit);
        let previous = self.lower_native_collection_method(source, "get", &args[..2])?;
        let prepare = |this: &mut Self, input: &[IrValue]| -> Result<IrValue, IrLoweringError> {
            let result = this.call_function_value(args[2], value, input)?;
            this.emit_intrinsic(CollectionMutationEnd, &[args[0]], ValueType::Unit);
            this.lower_native_collection_method(source, "insert", &[args[0], args[1], result])?;
            Ok(result)
        };
        if operation == MapUpdate {
            prepare(self, &[previous])
        } else {
            let present = self.standard_enum_op(&optional, Op::Test(0), Some(previous))?;
            self.branch_enum_value(
                present,
                value,
                |this| {
                    this.emit_intrinsic(CollectionMutationEnd, &[args[0]], ValueType::Unit);
                    this.standard_enum_op(&optional, Op::Read(0), Some(previous))
                },
                |this| prepare(this, &[]),
            )
        }
    }
}
