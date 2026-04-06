use kagari_common::SourceFile;
use kagari_hir::analyze_module;
use kagari_ir::{
    bytecode::{BytecodeModule, lower_to_bytecode},
    lower_to_ir,
};
use kagari_runtime::{LoadedModule, Runtime};
use kagari_syntax::parse_module;

pub fn load_bytecode_module(name: &str, bytecode: BytecodeModule) -> (Runtime, LoadedModule) {
    let mut runtime = Runtime::default();
    let loaded = runtime.load_module(name, bytecode);
    (runtime, loaded)
}

pub fn load_test_module(source_text: &str) -> (Runtime, LoadedModule) {
    let bytecode = compile_test_bytecode(source_text);
    load_bytecode_module("test.kgr", bytecode)
}

pub fn compile_test_bytecode(source_text: &str) -> BytecodeModule {
    let source = SourceFile::new("test.kgr", source_text);
    let ast = parse_module(&source).expect("source should parse");
    let analyzed = analyze_module(&ast).expect("analysis should succeed");
    let ir = lower_to_ir(&analyzed).expect("ir lowering should succeed");
    lower_to_bytecode(&ir).expect("bytecode lowering should succeed")
}
