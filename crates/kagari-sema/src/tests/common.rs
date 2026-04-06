use kagari_common::SourceFile;
use kagari_syntax::ast;

pub fn parse_ok(text: &str) -> ast::SourceFile {
    let source = SourceFile::new("test.kg", text);
    kagari_syntax::parse_module(&source).expect("source should parse")
}
