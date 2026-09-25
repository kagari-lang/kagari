use kagari_common::SourceFile;
use kagari_hir::analyze_source;
use kagari_ir::{
    bytecode::{
        BytecodeFunction, BytecodeInstruction, BytecodeModule, ConstantOperand, FunctionMetadata,
        FunctionRecord, FunctionRef, lower_to_bytecode,
    },
    lower_to_ir,
    module::ValueType,
};
use kagari_runtime::{LoadedModule, Runtime};

pub fn load_bytecode_module(name: &str, bytecode: BytecodeModule) -> (Runtime, LoadedModule) {
    load_bytecode_module_with_runtime(Runtime::default(), name, bytecode)
}

pub fn load_bytecode_module_with_runtime(
    mut runtime: Runtime,
    name: &str,
    bytecode: BytecodeModule,
) -> (Runtime, LoadedModule) {
    let loaded = runtime
        .load_program(
            name,
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![bytecode],
            },
        )
        .expect("test module should load");
    (runtime, loaded)
}

pub fn load_test_module(source_text: &str) -> (Runtime, LoadedModule) {
    let bytecode = compile_test_bytecode(source_text);
    load_bytecode_module("test.kgr", bytecode)
}

pub fn compile_test_bytecode(source_text: &str) -> BytecodeModule {
    let source = SourceFile::new("test.kgr", source_text);

    let analyzed = analyze_source(
        &source,
        kagari_hir::LanguageFeatureProfile {
            allow_host_calls: true,
            allow_reflection: true,
            allow_reflection_write: true,
            ..Default::default()
        },
    )
    .into_codegen()
    .expect("analysis should succeed");
    let ir = lower_to_ir(&analyzed, &Default::default()).expect("ir lowering should succeed");
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
        roots: kagari_ir::bytecode::RootSlotLayout::from_types(&[], &registers),
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
            identity: None,
            name: name.to_owned(),
            params: metadata.params.clone(),
            return_type: metadata.return_type,
            effects: metadata.effects,
        }],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            identity: None,
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

pub fn with_host_imports(
    mut module: BytecodeModule,
    functions: Vec<kagari_common::host_interface::HostFunctionDeclaration>,
) -> BytecodeModule {
    module.host_interface = kagari_common::host_interface::HostInterface {
        paths: vec![],
        types: Vec::new(),
        functions,
    };
    module
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

/// Explicit Point layout for reflection permission fixtures.
pub fn point_function_module(
    name: &str,
    instructions: Vec<BytecodeInstruction>,
    return_type: ValueType,
    registers: Vec<ValueType>,
) -> BytecodeModule {
    let mut module = test_function_module(name, instructions, return_type, registers);
    module.structures = compile_test_bytecode("struct Point { var x: i32 }").structures;
    module
}
