pub mod resolver;
pub mod typeck;
pub mod types;

use kagari_common::Diagnostic;
use kagari_syntax::ast;

pub type DiagnosticBuffer = smallvec::SmallVec<[Diagnostic; 4]>;
pub type BoxedDiagnosticBuffer = Box<DiagnosticBuffer>;
pub type FunctionBuffer = smallvec::SmallVec<[typeck::TypedFunction; 8]>;
pub type ParameterBuffer = smallvec::SmallVec<[typeck::TypedParameter; 4]>;

pub use resolver::NameTable;
pub use typeck::{TypedFunction, TypedModule};
pub use types::{BuiltinType, TypeId};

pub fn analyze(module: &ast::SourceFile) -> Result<TypedModule, BoxedDiagnosticBuffer> {
    let names = resolver::resolve_names(module)?;
    typeck::check_module(module, &names)
}

#[cfg(test)]
mod tests;
