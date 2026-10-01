use kagari_common::diagnostic::Diagnostic;
use smallvec::SmallVec;

pub mod ast;
pub mod kind;
pub mod lexer;
pub mod parser;
pub mod syntax_node;
pub mod token;

pub type TokenBuffer = SmallVec<[token::Token; 64]>;
pub type DiagnosticBuffer = SmallVec<[Diagnostic; 4]>;
pub type BoxedDiagnosticBuffer = Box<DiagnosticBuffer>;

#[cfg(test)]
mod tests;
