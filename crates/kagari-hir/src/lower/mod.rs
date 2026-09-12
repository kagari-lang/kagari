mod context;
mod expr;
mod item;
mod stmt;
mod ty;

use kagari_syntax::ast;

use crate::hir::Module;
use crate::source_map::SourceMap;

use crate::lower::context::Lowerer;

#[derive(Debug, Clone)]
pub struct LoweredModule {
    pub module: Module,
    pub source_map: SourceMap,
}

pub fn lower_module(module: &ast::SourceFile) -> LoweredModule {
    lower_module_controlled(module, &Default::default())
}

pub(crate) fn lower_module_controlled(
    module: &ast::SourceFile,
    cancel: &kagari_common::cancellation::CancellationToken,
) -> LoweredModule {
    let mut lowerer = Lowerer::new(cancel.clone());
    lowerer.lower_module(module);
    let (module, source_map) = lowerer.finish();
    LoweredModule { module, source_map }
}
