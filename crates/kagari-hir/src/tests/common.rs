use kagari_common::SourceFile;
use kagari_syntax::ast;
use kagari_syntax::parse_module;

use crate::lower::{LoweredModule, lower_module};

pub fn parse_ok(text: &str) -> ast::SourceFile {
    let source = SourceFile::new("test.kg", text);
    parse_module(&source).expect("source should parse")
}

pub fn lower_ok(text: &str) -> LoweredModule {
    let module = parse_ok(text);
    lower_module(&module)
}
