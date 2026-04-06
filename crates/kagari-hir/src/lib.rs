pub mod hir;
pub mod lower;
pub mod resolver;
pub mod source_map;
pub mod typeck;
pub mod types;

use kagari_common::Diagnostic;
use kagari_syntax::ast;

pub type DiagnosticBuffer = smallvec::SmallVec<[Diagnostic; 4]>;
pub type BoxedDiagnosticBuffer = Box<DiagnosticBuffer>;

pub fn analyze(module: &ast::SourceFile) -> Result<typeck::TypedModule, BoxedDiagnosticBuffer> {
    let lowered = lower::lower_module(module);
    let names = resolver::resolve_names(&lowered)?;
    typeck::check_module(&lowered, &names)
}

#[cfg(test)]
mod tests;
