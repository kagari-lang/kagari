mod expr;
mod function;
mod state;
mod stmt;
mod support;

use kagari_hir::AnalyzedModule;

use crate::module::IrModule;

#[derive(Debug)]
pub enum IrLoweringError {
    MissingTypedFunction(kagari_hir::hir::FunctionId),
    MissingExprType(kagari_hir::hir::ExprId),
    MissingLocalType(kagari_hir::hir::LocalId),
    UnresolvedExpr(kagari_hir::hir::ExprId),
    UnresolvedPlace(kagari_hir::hir::PlaceId),
    MissingBinding(&'static str),
    UnsupportedExpr(&'static str),
    UnsupportedStatement(&'static str),
    InvalidLoopControl,
}

pub fn lower_to_ir(module: &AnalyzedModule) -> Result<IrModule, IrLoweringError> {
    let functions = module
        .lowered
        .module
        .functions
        .iter()
        .map(|function| function::lower_function(module, function))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(IrModule { functions })
}
