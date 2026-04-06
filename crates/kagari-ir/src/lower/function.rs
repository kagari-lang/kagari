use std::collections::HashMap;

use kagari_hir::AnalyzedModule;
use kagari_hir::hir;

use crate::lower::EvaluatedConst;
use crate::lower::IrLoweringError;
use crate::lower::state::FunctionLowerer;
use crate::module::{
    function::IrFunction,
    ids::ModuleSlotId,
    instruction::{Instruction, Terminator},
};

pub(crate) fn lower_function(
    module: &AnalyzedModule,
    function: &hir::Function,
    const_values: &HashMap<hir::ConstId, EvaluatedConst>,
    static_slots: &HashMap<hir::StaticId, ModuleSlotId>,
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

    let mut lowerer = FunctionLowerer::new(module, function, typed, const_values, static_slots);
    if matches!(function.kind, hir::FunctionKind::ModuleInit) {
        lower_module_slots(&mut lowerer)?;
    }
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

fn lower_module_slots(lowerer: &mut FunctionLowerer<'_>) -> Result<(), IrLoweringError> {
    for item in &lowerer.analyzed.lowered.module.items {
        match item {
            hir::Item::Static(id) => {
                let static_item = lowerer
                    .analyzed
                    .lowered
                    .module
                    .statics
                    .iter()
                    .find(|item| item.id == *id)
                    .ok_or(IrLoweringError::MissingBinding("static item"))?;
                let slot = lowerer
                    .static_slots
                    .get(id)
                    .copied()
                    .ok_or(IrLoweringError::MissingBinding("static slot"))?;
                let value = lowerer.lower_expr(static_item.initializer)?;
                lowerer.emit(Instruction::StoreModule { slot, src: value });
            }
            hir::Item::Const(_)
            | hir::Item::Function(_)
            | hir::Item::Struct(_)
            | hir::Item::Enum(_) => {}
        }
    }

    Ok(())
}
