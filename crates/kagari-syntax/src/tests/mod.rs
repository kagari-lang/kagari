mod attributes;
mod cancellation;
mod common;
mod declarations;
// Shared only with the planned TO04 HIR UI consumer; source owns the schema.
#[path = "../../../kagari-source/tests/support/diagnostics.rs"]
mod diagnostics;
mod grammar_coverage;
mod lexer;
mod limits;
mod parser;

mod interpolation;
