use std::collections::HashMap;

use kagari_hir::AnalyzedModule;
use kagari_hir::hir;

use crate::lower::IrLoweringError;
use crate::lower::state::FunctionLowerer;
use crate::module::function::IrFunction;
use crate::module::instruction::Terminator;

pub(crate) fn lower_function(
    module: &AnalyzedModule,
    function: &hir::Function,
) -> Result<IrFunction, IrLoweringError> {
    let typed_by_id = module
        .typed
        .functions
        .iter()
        .map(|function| (function.id, function))
        .collect::<HashMap<_, _>>();

    let typed = typed_by_id
        .get(&function.id)
        .copied()
        .ok_or(IrLoweringError::MissingTypedFunction(function.id))?;

    let mut lowerer = FunctionLowerer::new(module, function, typed);
    let tail = lowerer.lower_block(function.body)?;
    if !lowerer.current_block_terminated() {
        let value = match tail {
            Some(temp) => Some(temp),
            None => Some(lowerer.lower_unit()),
        };
        lowerer.set_terminator(Terminator::Return(value));
    }

    Ok(lowerer.finish())
}
