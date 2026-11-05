//! Empty-capture callable values from admitted canonical function applications.

use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::representation::ValueType;
use kagari_common::identity::table::DefinitionId;
use kagari_hir::{hir::ids::ExprId, types::TypeId};
use kagari_mir::instruction::{Instruction, MirValue, ValueBuffer};

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_local_function_value(
        &mut self,
        expr: ExprId,
        declaration: DefinitionId,
        arguments: &[TypeId],
    ) -> Result<MirValue, MirLoweringError> {
        let span = self.expression_span(expr);
        let arguments = self
            .planner
            .arguments(arguments, &self.instance.substitution, span)?;
        let function = self.planner.enqueue_local_function_value(
            &self.instance,
            declaration,
            arguments,
            span,
        )?;
        let dst = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::MakeClosure {
            dst,
            function,
            captures: ValueBuffer::new(),
        });
        Ok(dst)
    }
}
