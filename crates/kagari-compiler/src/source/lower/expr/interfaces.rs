//! Emit a planned interface construction and its optional bounded view.
use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::representation::ValueType;
use kagari_hir::{hir::ids::ExprId, typeck::table::ResolvedInterfaceCoercion};
use kagari_mir::instruction::{Instruction, MirValue};
use kagari_types::ty::Ty;

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_interface_coercion(
        &mut self,
        expr_id: ExprId,
        mut value: MirValue,
        coercion: ResolvedInterfaceCoercion,
    ) -> Result<MirValue, MirLoweringError> {
        let plan = self.planner.interface_coercion(
            coercion,
            &self.instance.substitution,
            self.expression_span(expr_id),
        )?;
        self.function
            .semantic
            .registers
            .insert(value.temp.index(), plan.receiver);
        let upcast = plan.construction.is_none() || plan.source != plan.target;
        if let Some(target) = plan.construction {
            let dst = self.alloc_temp(ValueType::HeapObject);
            self.emit(Instruction::MakeInterface { dst, value, target });
            self.function
                .semantic
                .registers
                .insert(dst.temp.index(), Ty::Trait(plan.source.clone()));
            value = dst;
        }
        if upcast {
            let dst = self.alloc_temp(ValueType::HeapObject);
            self.emit(Instruction::UpcastInterface {
                dst,
                value,
                source: plan.source,
                target: plan.target,
            });
            value = dst;
        }
        Ok(value)
    }
}
