//! Engine calls consume checked applications without public-method expansion.

use crate::source::lower::{
    MirLoweringError, expr::native_contracts::NativeApplication, state::FunctionLowerer,
};
use kagari_abi::callable::{EngineNativeBinding, NativeCall};
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
        let binding = match signature.implementation {
            FunctionImplementation::Script => return Ok(false),
            FunctionImplementation::Required => {
                return Err(MirLoweringError::MissingBinding(
                    "unimplemented callable requirement",
                ));
            }
            FunctionImplementation::Native(NativeBinding::Engine(binding)) => binding,
            FunctionImplementation::Native(NativeBinding::Host(_)) => {
                return Err(MirLoweringError::MissingBinding(
                    "source callable has a host binding",
                ));
            }
        };
        match binding {
            EngineNativeBinding::Intrinsic(_)
            | EngineNativeBinding::Integer(_)
            | EngineNativeBinding::ParseRadix => Ok(true),
            EngineNativeBinding::TraitDefault(_) | EngineNativeBinding::Protocol(_) => Err(
                MirLoweringError::MissingBinding("native trait callable witness"),
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
            callee: MirCallTarget::Native(NativeCall::Engine(Box::new(contract))),
            args: lowered,
        });
        Ok(dst)
    }
}
