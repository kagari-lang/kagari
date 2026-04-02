pub mod resolver;
pub mod typeck;
pub mod types;

use kagari_common::Diagnostic;
use kagari_syntax::ast;

pub use resolver::NameTable;
pub use typeck::{TypedFunction, TypedModule};
pub use types::{BuiltinType, TypeId};

pub fn analyze(module: &ast::Module) -> Result<TypedModule, Vec<Diagnostic>> {
    let names = resolver::resolve_names(module)?;
    typeck::check_module(module, &names)
}
