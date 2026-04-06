use kagari_common::SourceFile;
use kagari_hir::{AnalyzedModule, analyze_module};
use kagari_syntax::parse_module;

pub fn analyze_ok(text: &str) -> Box<AnalyzedModule> {
    let source = SourceFile::new("test.kg", text);
    let ast = parse_module(&source).expect("source should parse");
    Box::new(analyze_module(&ast).expect("analysis should succeed"))
}
