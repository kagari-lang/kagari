use kagari_bytecode::{module::RootSlotLayout, program::ModuleRef};
use kagari_contract::native_import::NativeImport;
use {kagari_abi::representation::ValueType, kagari_contract::ids::FunctionRef};

use kagari_bytecode::{
    instruction::{BytecodeInstruction, ConstantOperand},
    module::{BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord},
    program::BytecodeProgram,
};
use kagari_common::host_interface::{HostInterface, standard_log};
use kagari_common::source_database::{SourceDatabase, SourceLayer};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::{analysis::AnalysisDatabase, host::HostDeclarations};
use kagari_runtime::{Runtime, module::LoadedModule};

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
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![bytecode],
            },
        )
        .expect("test module should load");
    (runtime, loaded)
}

pub fn load_bytecode_program(name: &str, program: BytecodeProgram) -> (Runtime, LoadedModule) {
    load_bytecode_program_with_runtime(Runtime::default(), name, program)
}

pub fn load_bytecode_program_with_runtime(
    mut runtime: Runtime,
    name: &str,
    program: BytecodeProgram,
) -> (Runtime, LoadedModule) {
    let loaded = runtime
        .load_program(name, program)
        .expect("test program should load");
    (runtime, loaded)
}

pub fn load_test_module(source_text: &str) -> (Runtime, LoadedModule) {
    load_bytecode_program("test.kgr", compile_test_bytecode(source_text))
}

pub fn compile_test_bytecode(source_text: &str) -> BytecodeProgram {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("test.kgr", source_text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_host_declarations(
        HostDeclarations::new(HostInterface {
            paths: vec![],
            types: vec![],
            functions: vec![standard_log()],
        })
        .unwrap(),
    );
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .expect("analysis snapshot should succeed");
    let checked = snapshot
        .check_program(root, &Default::default())
        .expect("program analysis should succeed");
    let ir = lower_program_to_mir(&checked, &Default::default())
        .expect("program MIR lowering should succeed");
    lower_program_to_bytecode(&ir).expect("program bytecode lowering should succeed")
}

pub fn test_function_module(
    name: &str,
    instructions: Vec<BytecodeInstruction>,
    return_type: ValueType,
    registers: Vec<ValueType>,
) -> BytecodeModule {
    let metadata = FunctionMetadata {
        return_type,
        roots: RootSlotLayout::from_types(&[], &registers),
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
    module.native_imports = functions.iter().map(NativeImport::from_host).collect();
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
    let program = compile_test_bytecode("struct Point { var x: i32 }");
    module.structures = program.modules[program.root.index()].structures.clone();
    module
}
