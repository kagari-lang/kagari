use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, ConstantOperand},
    module::{BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, RootSlotLayout},
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::{ids::FunctionRef, native_import::NativeImport};
use kagari_hir::{analysis::AnalysisDatabase, host::HostDeclarations};
use kagari_runtime::{Runtime, RuntimeConfig, module::LoadedModule, native::module::NativeModule};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib as foundation;
use kagari_types::host_interface::{HostFunctionDeclaration, HostInterface, standard_log};
use std::{iter, sync::Arc};

pub fn load_bytecode_module(
    name: &str,
    bytecode: DefinitionMetadata<BytecodeModule>,
) -> (Runtime, LoadedModule) {
    load_bytecode_module_with_runtime(standard_runtime(Default::default()), name, bytecode)
}

pub fn load_bytecode_module_with_runtime(
    mut runtime: Runtime,
    name: &str,
    bytecode: DefinitionMetadata<BytecodeModule>,
) -> (Runtime, LoadedModule) {
    let loaded = runtime
        .load_program(name, module_program(bytecode))
        .expect("test module should load");
    (runtime, loaded)
}

/// Retain a module's identity table while creating a single-member program.
pub fn module_program(
    module: DefinitionMetadata<BytecodeModule>,
) -> DefinitionMetadata<BytecodeProgram> {
    let (definitions, module) = module.into_parts();
    DefinitionMetadata::checked(
        definitions,
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![module],
        },
        &Default::default(),
    )
    .unwrap()
}

/// Exercise the actual source-free codec and loader validation with a fresh receiving context.
pub fn round_trip_bytecode(
    program: DefinitionMetadata<BytecodeProgram>,
) -> DefinitionMetadata<BytecodeProgram> {
    let context = DefinitionContext::new().unwrap();
    let program = program
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let artifact = KbcArtifact::from_program(program, Default::default(), &context).unwrap();
    let encoded = artifact.to_bytes(&context.snapshot()).unwrap();
    let decoded = KbcArtifact::from_bytes(&encoded).unwrap();
    let receiver = DefinitionContext::new().unwrap();
    let decoded = decoded
        .import_into(&receiver, &Default::default())
        .unwrap()
        .into_records();
    decoded
        .validate_for_loader(&Default::default(), &receiver)
        .unwrap();
    DefinitionMetadata::checked(receiver.snapshot(), decoded.program, &Default::default()).unwrap()
}

pub fn load_bytecode_program(
    name: &str,
    program: DefinitionMetadata<BytecodeProgram>,
) -> (Runtime, LoadedModule) {
    load_bytecode_program_with_runtime(standard_runtime(Default::default()), name, program)
}

pub fn load_bytecode_program_with_runtime(
    mut runtime: Runtime,
    name: &str,
    program: DefinitionMetadata<BytecodeProgram>,
) -> (Runtime, LoadedModule) {
    let loaded = runtime
        .load_program(name, program)
        .expect("test program should load");
    (runtime, loaded)
}

pub fn load_test_module(source_text: &str) -> (Runtime, LoadedModule) {
    load_bytecode_program("test.kgr", compile_test_bytecode(source_text))
}

pub fn compile_test_bytecode(source_text: &str) -> DefinitionMetadata<BytecodeProgram> {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("test.kgr", source_text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        foundation::modules()
            .unwrap()
            .iter()
            .map(|module| Arc::new(module.to_declaration()))
            .collect(),
    );
    let (definitions, log) = standard_log().unwrap().into_parts();
    let interface = DefinitionMetadata::checked(
        definitions,
        HostInterface {
            paths: vec![],
            types: vec![],
            functions: vec![log],
        },
        &Default::default(),
    )
    .unwrap();
    analysis.set_host_declarations(HostDeclarations::new(interface).unwrap());
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
    constants: Vec<ConstantOperand>,
) -> DefinitionMetadata<BytecodeModule> {
    let metadata = FunctionMetadata {
        return_type,
        roots: RootSlotLayout::from_types(&[], &registers),
        registers,
        ..FunctionMetadata::default()
    };
    let module = BytecodeModule {
        constants,
        types: unique_types(
            iter::once(ValueType::Unit)
                .chain(iter::once(metadata.return_type))
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
    };
    DefinitionMetadata::checked(
        DefinitionContext::new().unwrap().snapshot(),
        module,
        &Default::default(),
    )
    .unwrap()
}

/// Adopt each independently authored Host signature before constructing executable imports.
pub fn with_host_imports(
    module: DefinitionMetadata<BytecodeModule>,
    functions: Vec<DefinitionMetadata<HostFunctionDeclaration>>,
) -> DefinitionMetadata<BytecodeModule> {
    let context = DefinitionContext::new().unwrap();
    let mut module = module
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let functions = functions
        .iter()
        .map(|function| {
            function
                .import_into(&context, &Default::default())
                .unwrap()
                .into_records()
        })
        .collect::<Vec<_>>();
    module.native_imports = functions.iter().map(NativeImport::from_host).collect();
    module.host_interface = HostInterface {
        paths: vec![],
        types: vec![],
        functions,
    };
    DefinitionMetadata::checked(context.snapshot(), module, &Default::default()).unwrap()
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
    constants: Vec<ConstantOperand>,
) -> DefinitionMetadata<BytecodeModule> {
    let mut module =
        test_function_module(name, instructions, return_type, registers, constants).into_records();
    let program = compile_test_bytecode("struct Point { var x: i32 }");
    module.structures = program.records().modules[program.records().root.index()]
        .structures
        .clone();
    DefinitionMetadata::checked(program.definitions().clone(), module, &Default::default()).unwrap()
}

/// Explicit standard installation for script fixtures and their source-free products.
pub fn standard_runtime(config: RuntimeConfig) -> Runtime {
    let mut runtime = Runtime::new(config);
    NativeModule::install_all(&foundation::modules().unwrap(), &mut runtime).unwrap();
    runtime
}
