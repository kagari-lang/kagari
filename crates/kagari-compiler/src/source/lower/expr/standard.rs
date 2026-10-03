use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_contract::operations::StandardEnumOp;
use kagari_hir::types::{TypeId, abi::lower_type};
use kagari_mir::instruction::{Instruction, MirValue};
use std::slice;

impl FunctionLowerer<'_, '_> {
    pub(crate) fn standard_enum_op(
        &mut self,
        ty: &TypeId,
        op: StandardEnumOp,
        value: Option<MirValue>,
    ) -> Result<MirValue, MirLoweringError> {
        let concrete = self.planner.arguments(
            slice::from_ref(ty),
            &self.instance.substitution,
            self.function.debug.source_span,
        )?;
        let ty = lower_type(&concrete[0]);
        let (_, output) = op
            .contract_in(
                &ty,
                self.function
                    .semantic
                    .generic
                    .as_ref()
                    .map_or(&[], |body| body.parameters.as_slice()),
            )
            .ok_or(MirLoweringError::MissingBinding("standard enum contract"))?;
        let dst = self.alloc_temp(output);
        self.emit(Instruction::StandardEnum { dst, value, ty, op });
        Ok(dst)
    }
}
