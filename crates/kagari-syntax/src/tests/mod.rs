mod attributes;
mod cancellation;
mod common;
mod declarations;
// Source owns the diagnostic schema/projection shared only with the HIR harness.
#[path = "../../../kagari-source/tests/support/diagnostics.rs"]
mod diagnostics;
mod grammar_coverage;
mod lexer;
mod limits;
mod parser;

mod interpolation;
