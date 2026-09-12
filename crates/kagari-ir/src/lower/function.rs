use kagari_hir::AnalyzedModule;
use kagari_hir::hir;

use crate::lower::IrLoweringError;
use crate::lower::state::FunctionLowerer;
use crate::module::{function::IrFunction, instruction::Terminator};

pub(crate) fn lower_function<'a>(
    module: &'a AnalyzedModule,
    function: &hir::Function,
    instance: super::instances::Instance,
    planner: &mut super::instances::InstancePlanner<'a>,
) -> Result<IrFunction, IrLoweringError> {
    let typed = module
        .typed
        .functions
        .iter()
        .find(|typed| typed.id == function.id)
        .ok_or(IrLoweringError::MissingTypedFunction(function.id))?;

    let mut lowerer = FunctionLowerer::new(module, function, typed, instance, planner)?;
    let tail = lowerer.lower_block(function.body)?;
    if !lowerer.current_block_terminated() {
        let value = match tail {
            Some(temp) => Some(temp),
            None => Some(lowerer.lower_unit()),
        };
        lowerer.set_terminator(Terminator::Return(value));
    }

    lowerer.planner.check()?;
    Ok(lowerer.finish())
}
