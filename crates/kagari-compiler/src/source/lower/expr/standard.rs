//! Declaration-driven enum operations used by protocol lowering.
use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::representation::ValueType;
use kagari_hir::types::TypeId;
use kagari_mir::instruction::{Instruction, MirValue};

impl FunctionLowerer<'_, '_> {
    pub(crate) fn test_enum_variant(
        &mut self,
        ty: &TypeId,
        value: MirValue,
        variant: usize,
    ) -> Result<MirValue, MirLoweringError> {
        let enumeration = self.nominal_instance(ty)?;
        self.planner.record_layout_root(
            ty,
            &self.instance.substitution,
            self.function.debug.source_span,
        )?;
        let dst = self.alloc_temp(ValueType::Bool);
        self.emit(Instruction::TestEnumVariant {
            dst,
            value,
            enumeration,
            variant,
        });
        Ok(dst)
    }

    pub(crate) fn read_enum_field(
        &mut self,
        ty: &TypeId,
        value: MirValue,
        variant: usize,
        index: usize,
    ) -> Result<MirValue, MirLoweringError> {
        let TypeId::Enum(nominal) = ty else {
            return Err(MirLoweringError::MissingBinding("enum field owner"));
        };
        let template = self
            .planner
            .aggregate_catalog(&nominal.declaration)
            .enumeration(&nominal.declaration)
            .ok_or(MirLoweringError::MissingBinding("enum declaration"))?;
        let substitution = template
            .generic_params
            .iter()
            .cloned()
            .zip(nominal.arguments.iter().cloned())
            .collect();
        let member = template
            .variants
            .get(variant)
            .and_then(|member| member.payload.get(index))
            .ok_or(MirLoweringError::MissingBinding("enum payload field"))?
            .instantiate(&substitution);
        let enumeration = self.nominal_instance(ty)?;
        self.planner.record_layout_root(
            ty,
            &self.instance.substitution,
            self.function.debug.source_span,
        )?;
        let dst = self.alloc_temp(self.value_type(&member)?);
        self.emit(Instruction::ReadEnumPayload {
            dst,
            value,
            enumeration,
            variant,
            index,
        });
        Ok(dst)
    }
}
