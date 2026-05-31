use kagari_common::SourceFile;
use kagari_hir::analyze_module;
use kagari_ir::{
    bytecode::{
        BytecodeFunction, BytecodeInstruction, BytecodeModule, ConstantOperand, FunctionMetadata,
        FunctionRecord, FunctionRef, lower_to_bytecode,
    },
    lower_to_ir,
    module::ValueType,
};
use kagari_runtime::{LoadedModule, Runtime};
use kagari_syntax::parse_module;

pub fn load_bytecode_module(name: &str, bytecode: BytecodeModule) -> (Runtime, LoadedModule) {
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_module(name, bytecode)
        .expect("test module should load");
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

pub fn test_function_module(
    name: &str,
    instructions: Vec<BytecodeInstruction>,
    return_type: ValueType,
    registers: Vec<ValueType>,
) -> BytecodeModule {
    let metadata = FunctionMetadata {
        return_type,
        registers,
        ..FunctionMetadata::default()
    };
    BytecodeModule {
        constants: constants_for_instructions(&instructions),
        types: unique_types(
            std::iter::once(ValueType::Unit)
                .chain(std::iter::once(metadata.return_type))
                .chain(metadata.registers.iter().copied()),
        ),
        function_table: vec![FunctionRecord {
            id: FunctionRef::new(0),
            name: name.to_owned(),
            params: metadata.params.clone(),
            return_type: metadata.return_type,
            effects: metadata.effects,
        }],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            name: name.to_owned(),
            parameter_count: 0,
            register_count: metadata.registers.len() as u16,
            local_count: 0,
            metadata,
            instructions,
        }],
        ..BytecodeModule::default()
    }
}

pub fn constants_for_instructions(instructions: &[BytecodeInstruction]) -> Vec<ConstantOperand> {
    let mut constants = Vec::new();
    for instruction in instructions {
        if let BytecodeInstruction::LoadConst { constant, .. } = instruction
            && !constants.contains(constant)
        {
            constants.push(constant.clone());
        }
    }
    constants
}

pub fn unique_types(types: impl IntoIterator<Item = ValueType>) -> Vec<ValueType> {
    let mut unique = Vec::new();
    for ty in types {
        if !unique.contains(&ty) {
            unique.push(ty);
        }
    }
    unique
}
