pub mod ast;
pub mod lexer;
pub mod parser;
pub mod token;

pub use parser::parse_module;
