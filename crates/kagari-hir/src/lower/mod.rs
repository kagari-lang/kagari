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
    let mut lowerer = Lowerer::new();
    lowerer.lower_module(module);
    let (module, source_map) = lowerer.finish();
    LoweredModule { module, source_map }
}
