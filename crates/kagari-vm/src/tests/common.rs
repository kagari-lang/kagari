use crate::tests::native_program::ProgramFixture;
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, ConstantOperand},
    module::{BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, RootSlotLayout},
    program::{BytecodeProgram, ModuleRef, verified::VerifiedBytecodeProgram},
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_compiler::{
    bytecode::lower_program_to_bytecode,
    source::{lower::instances::MirLoweringOptions, program::lower_program_to_mir},
};
use kagari_contract::ids::FunctionRef;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    host::{HostCallContext, HostError},
    module::{Program, VerifiedProgram},
    native::{builder::ModuleBuilder, host::HostCallback, module::NativeModule},
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib as foundation;
use kagari_types::{
    declaration::logging,
    ty::{NominalTy, Ty},
};
use std::{iter, sync::Arc};

pub fn load_bytecode_module(
    name: &str,
    bytecode: DefinitionMetadata<BytecodeModule>,
) -> (Runtime, Program) {
    load_bytecode_module_with_runtime(standard_runtime(Default::default()), name, bytecode)
}

pub fn load_bytecode_module_with_runtime(
    mut runtime: Runtime,
    name: &str,
    bytecode: DefinitionMetadata<BytecodeModule>,
) -> (Runtime, Program) {
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
    ProgramFixture {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![module],
    }
    .metadata(definitions, &Default::default())
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
    let artifact =
        KbcArtifact::from_program(program, Default::default(), &context, &Default::default())
            .unwrap();
    let encoded = artifact.to_bytes(&context.snapshot()).unwrap();
    let decoded = KbcArtifact::from_bytes(&encoded).unwrap();
    let receiver = DefinitionContext::new().unwrap();
    let decoded = decoded
        .import_into(&receiver, &Default::default())
        .unwrap()
        .into_records();
    decoded
        .validate_for_loader(&Default::default(), &receiver, &Default::default())
        .unwrap();
    DefinitionMetadata::checked(receiver.snapshot(), decoded.program, &Default::default()).unwrap()
}

pub fn load_bytecode_program(
    name: &str,
    program: DefinitionMetadata<BytecodeProgram>,
) -> (Runtime, Program) {
    load_bytecode_program_with_runtime(standard_runtime(Default::default()), name, program)
}

pub fn load_bytecode_program_with_runtime(
    mut runtime: Runtime,
    name: &str,
    program: DefinitionMetadata<BytecodeProgram>,
) -> (Runtime, Program) {
    let loaded = runtime
        .load_program(name, program)
        .expect("test program should load");
    (runtime, loaded)
}

pub fn load_test_module(source_text: &str) -> (Runtime, Program) {
    let program = compile_native_test_bytecode_with_options(source_text, &[], &Default::default());
    let mut runtime = standard_runtime(Default::default());
    let loaded = runtime
        .load_verified_program("test.kgr", VerifiedProgram::from_bytecode(program))
        .expect("test program should load");
    (runtime, loaded)
}

/// Bind the fixed print service through ordinary common native registration.
pub fn logging_module(
    entry: HostCallback<
        impl for<'call> Fn(&HostCallContext<'call>, &[Value]) -> Result<Value, HostError>
        + Send
        + Sync
        + 'static,
    >,
) -> NativeModule {
    let mut builder = ModuleBuilder::from_declaration(
        logging::declaration().unwrap(),
        &foundation::declarations::StandardDeclarations::default()
            .catalog()
            .unwrap(),
        Default::default(),
    )
    .unwrap();
    let log = builder.declared_function("log").unwrap();
    builder.bind(log, entry).unwrap();
    builder.finish().unwrap()
}

/// Compile the explicit logging contract; callers install their own logging implementation.
pub fn compile_logging_test_bytecode(source_text: &str) -> DefinitionMetadata<BytecodeProgram> {
    compile_native_test_bytecode(
        source_text,
        &[&logging_module(unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(|_, _| Ok(Value::Unit))
        })],
    )
}

pub fn compile_test_bytecode(source_text: &str) -> DefinitionMetadata<BytecodeProgram> {
    compile_native_test_bytecode(source_text, &[])
}

/// Compile common native declarations without reading runtime registration state.
pub fn compile_native_test_bytecode(
    source_text: &str,
    modules: &[&NativeModule],
) -> DefinitionMetadata<BytecodeProgram> {
    compile_native_test_bytecode_with_options(source_text, modules, &Default::default())
        .into_unverified()
}

pub fn compile_test_bytecode_with_options(
    source_text: &str,
    options: &MirLoweringOptions,
) -> DefinitionMetadata<BytecodeProgram> {
    compile_native_test_bytecode_with_options(source_text, &[], options).into_unverified()
}

fn compile_native_test_bytecode_with_options(
    source_text: &str,
    modules: &[&NativeModule],
    options: &MirLoweringOptions,
) -> VerifiedBytecodeProgram {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("test.kgr", source_text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    let mut declarations = foundation::modules()
        .unwrap()
        .iter()
        .map(|module| Arc::new(module.to_declaration()))
        .collect::<Vec<_>>();
    declarations.extend(
        modules
            .iter()
            .map(|module| Arc::new(module.to_declaration())),
    );
    analysis.set_native_modules(declarations);
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .expect("analysis snapshot should succeed");
    let checked = snapshot
        .check_program(root, &Default::default())
        .expect("program analysis should succeed");
    let ir = lower_program_to_mir(&checked, options).expect("program MIR lowering should succeed");
    lower_program_to_bytecode(&ir, &Default::default())
        .expect("program bytecode lowering should succeed")
}

pub fn test_function_module(
    name: &str,
    instructions: Vec<BytecodeInstruction>,
    return_type: ValueType,
    registers: Vec<ValueType>,
    constants: Vec<ConstantOperand>,
) -> DefinitionMetadata<BytecodeModule> {
    let metadata = FunctionMetadata {
        roots: RootSlotLayout::from_types(&[registers.to_vec(), [].to_vec()].concat()),
        return_type,
        slots: registers,
        ..FunctionMetadata::default()
    };
    let module = BytecodeModule {
        constants,
        types: unique_types(
            iter::once(ValueType::Unit)
                .chain(iter::once(metadata.return_type))
                .chain(metadata.slots.iter().copied()),
        ),
        function_table: vec![FunctionRecord {
            id: FunctionRef::new(0),
            identity: None,
            name: name.to_owned(),
            params: metadata.params.clone(),
            return_type: metadata.return_type,
            effects: metadata.effects,
        }],
        functions: vec![{
            let fixture_instructions = instructions;
            let mut fixture_metadata: kagari_bytecode::module::FunctionMetadata = metadata;

            if fixture_metadata.origins.is_empty() {
                fixture_metadata.origins = (0..fixture_instructions.len())
                    .map(|instruction| kagari_bytecode::module::InstructionOrigin {
                        span: Default::default(),
                        line: None,
                        column: None,
                        instruction,
                    })
                    .collect();
            }
            BytecodeFunction {
                id: FunctionRef::new(0),
                identity: None,
                name: name.to_owned(),
                parameter_count: 0,
                slot_count: u32::try_from(fixture_metadata.slots.len()).unwrap(),
                metadata: fixture_metadata,
                instructions: fixture_instructions,
            }
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
    let point = Ty::Struct(NominalTy {
        declaration: module.structures[0].declaration,
        arguments: vec![],
        associated_types: Default::default(),
    });
    let metadata = &mut module.functions[0].metadata;
    if return_type == ValueType::HeapObject {
        metadata.semantic.result = Some(point.clone());
    }
    for (index, ty) in metadata.slots.iter().enumerate() {
        if *ty == ValueType::HeapObject {
            metadata.semantic.slots.insert(index, point.clone());
        }
    }
    DefinitionMetadata::checked(program.definitions().clone(), module, &Default::default()).unwrap()
}

/// Explicit standard installation for script fixtures and their source-free products.
pub fn standard_runtime(config: RuntimeConfig) -> Runtime {
    let mut runtime = Runtime::new(config);
    NativeModule::install_all(&foundation::modules().unwrap(), &mut runtime).unwrap();
    runtime
}
