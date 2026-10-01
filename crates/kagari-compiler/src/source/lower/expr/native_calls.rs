//! Engine calls consume checked applications without public-method expansion.

use crate::source::lower::{
    MirLoweringError, expr::native_contracts::NativeApplication, state::FunctionLowerer,
};

use kagari_hir::{
    hir,
    native::NativeBinding,
    typeck::{CallTarget, FunctionImplementation},
};
use kagari_mir::instruction::{CallTarget as MirCallTarget, Instruction, MirValue, ValueBuffer};

impl FunctionLowerer<'_, '_> {
    pub(super) fn engine_native_for_call(
        &self,
        target: &CallTarget,
    ) -> Result<bool, MirLoweringError> {
        let signature = match target {
            CallTarget::Function(id) => self
                .analyzed
                .typed
                .functions
                .iter()
                .find(|function| function.id == *id)
                .ok_or(MirLoweringError::MissingTypedFunction(*id))?,
            CallTarget::SourceFunction(id) => {
                &self
                    .analyzed
                    .imported_functions
                    .target(id)
                    .ok_or(MirLoweringError::MissingBinding("checked source callable"))?
                    .signature
            }
            _ => return Ok(false),
        };
        match &signature.implementation {
            FunctionImplementation::Script => Ok(false),
            FunctionImplementation::Required => Err(MirLoweringError::MissingBinding(
                "unimplemented callable requirement",
            )),
            FunctionImplementation::Native(NativeBinding::Provider(_)) => Ok(true),
            FunctionImplementation::Native(NativeBinding::Host(_)) => Err(
                MirLoweringError::MissingBinding("source callable has a host binding"),
            ),
        }
    }

    pub(super) fn lower_engine_call(
        &mut self,
        expr: hir::ExprId,
        lowered: ValueBuffer,
        application: NativeApplication<'_>,
    ) -> Result<MirValue, MirLoweringError> {
        let span = self.analyzed.lowered.source_map.expr_span(expr);
        let contract = self.engine_native_contract(
            application.target,
            application.signature,
            application.arguments,
            span,
        )?;
        let dst = self.alloc_temp(self.expr_type(expr)?);
        self.emit(Instruction::Call {
            dst: Some(dst),
            callee: MirCallTarget::Native(Box::new(contract)),
            args: lowered,
        });
        Ok(dst)
    }
}
