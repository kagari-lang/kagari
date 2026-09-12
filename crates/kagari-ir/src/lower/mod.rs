mod abi;
mod expr;
mod function;
mod state;
mod stmt;
mod support;

use crate::module::IrModule;
use kagari_hir::hir::{ExprId, FunctionId, FunctionKind, LocalId, PlaceId};

#[derive(Debug)]
pub enum IrLoweringError {
    MissingTypedFunction(FunctionId),
    MissingExprType(ExprId),
    MissingLocalType(LocalId),
    UnresolvedExpr(ExprId),
    UnresolvedPlace(PlaceId),
    MissingBinding(&'static str),
    UnsupportedExpr(&'static str),
    UnsupportedStatement(&'static str),
    InvalidLoopControl,
}

pub fn lower_to_ir(module: &kagari_hir::CheckedAnalysis) -> Result<IrModule, IrLoweringError> {
    let functions = module
        .lowered
        .module
        .functions
        .iter()
        .filter(|function| matches!(function.kind, FunctionKind::User | FunctionKind::ModuleInit))
        .map(|function| function::lower_function(module, function))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(IrModule {
        module_init: module.lowered.module.module_init,
        module_slots: Vec::new(),
        abi: abi::collect_module_abi(module),
        functions,
    })
}
