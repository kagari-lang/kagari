pub mod ast;
pub mod kind;
pub mod lexer;
pub mod parser;
pub mod syntax_node;
pub mod token;

pub type TokenBuffer = smallvec::SmallVec<[token::Token; 64]>;
pub type DiagnosticBuffer = smallvec::SmallVec<[kagari_common::Diagnostic; 4]>;
pub type BoxedDiagnosticBuffer = Box<DiagnosticBuffer>;

pub use parser::{Parse, parse, parse_module};

#[cfg(test)]
mod tests;
