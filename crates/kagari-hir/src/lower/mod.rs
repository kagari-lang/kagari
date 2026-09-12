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
    pub source: std::sync::Arc<kagari_common::SourceFile>,
    pub module: Module,
    pub source_map: SourceMap,
}

pub fn lower_module(source: &kagari_common::SourceFile) -> LoweredModule {
    let parsed = kagari_syntax::parse(source);
    lower_module_controlled(
        std::sync::Arc::new(source.clone()),
        &parsed.syntax(),
        &Default::default(),
    )
}

pub(crate) fn lower_module_controlled(
    source: std::sync::Arc<kagari_common::SourceFile>,
    module: &ast::SourceFile,
    cancel: &kagari_common::cancellation::CancellationToken,
) -> LoweredModule {
    let mut lowerer = Lowerer::new(cancel.clone());
    lowerer.lower_module(module);
    let (module, source_map) = lowerer.finish();
    LoweredModule {
        source,
        module,
        source_map,
    }
}
