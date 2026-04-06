use crate::{
    bytecode::{BytecodeModule, lower_to_bytecode},
    lower_to_ir,
};
use kagari_common::SourceFile;
use kagari_hir::{AnalyzedModule, analyze_module};
use kagari_syntax::parse_module;

pub fn analyze_ok(text: &str) -> Box<AnalyzedModule> {
    let source = SourceFile::new("test.kg", text);
    let ast = parse_module(&source).expect("source should parse");
    Box::new(analyze_module(&ast).expect("analysis should succeed"))
}

pub fn bytecode_ok(text: &str) -> BytecodeModule {
    let analyzed = analyze_ok(text);
    let ir = lower_to_ir(&analyzed).expect("ir lowering should succeed");
    lower_to_bytecode(&ir).expect("bytecode lowering should succeed")
}
