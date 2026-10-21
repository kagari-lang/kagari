//! Empty-capture callable values from checked local function applications.

use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::representation::ValueType;
use kagari_hir::{
    hir::{expr::ExprKind, ids::ExprId},
    typeck::table::CallTarget,
};
use kagari_mir::instruction::{Instruction, MirValue, ValueBuffer};

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_local_function_value(
        &mut self,
        expr: ExprId,
    ) -> Result<Option<MirValue>, MirLoweringError> {
        if !matches!(
            self.analyzed.lowered.module.expr(expr).kind,
            ExprKind::Name { .. }
        ) {
            return Ok(None);
        }
        let Some(application) = self.analyzed.typed.type_table.call_resolution(expr) else {
            return Ok(None);
        };
        let CallTarget::Function(function) = application.target else {
            return Err(MirLoweringError::MissingBinding(
                "local callable application",
            ));
        };
        let span = self.analyzed.lowered.source_map.expr_span(expr);
        let arguments = self.planner.arguments(
            &application.type_arguments,
            &self.instance.substitution,
            span,
        )?;
        let function = self.planner.enqueue_local_function_value(
            &self.instance,
            self.analyzed,
            function,
            arguments,
            span,
        )?;
        let dst = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::MakeClosure {
            dst,
            function,
            captures: ValueBuffer::new(),
        });
        Ok(Some(dst))
    }
}
