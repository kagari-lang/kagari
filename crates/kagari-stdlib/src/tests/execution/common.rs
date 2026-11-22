#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use crate::{io, test_library};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    host::{HostCallContext, HostError},
    module::{InstalledProgram, VerifiedProgram},
    native::module::NativeModule,
    value::view::ValueView,
};

use kagari_bytecode::{
    artifact::KbcArtifact,
    program::{BytecodeProgram, verified::VerifiedBytecodeProgram},
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_compiler::{
    bytecode::lower_program_to_bytecode,
    source::{lower::instances::MirLoweringOptions, program::lower_program_to_mir},
};
use kagari_hir::compile_input::program::CheckedCompilation;
use kagari_source::source_database::{SourceDatabase, SourceLayer};

use std::sync::Arc;

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
) -> (Runtime, InstalledProgram) {
    load_bytecode_program_with_runtime(standard_runtime(Default::default()), name, program)
}

pub fn load_bytecode_program_with_runtime(
    mut runtime: Runtime,
    name: &str,
    program: DefinitionMetadata<BytecodeProgram>,
) -> (Runtime, InstalledProgram) {
    let loaded = load_bytecode(&mut runtime, name, program).expect("test program should load");
    (runtime, loaded)
}

pub fn load_test_module(source_text: &str) -> (Runtime, InstalledProgram) {
    let program = compile_native_test_bytecode_with_options(source_text, &[], &Default::default());
    let mut runtime = standard_runtime(Default::default());
    let loaded = runtime
        .load_verified_program("test.kgr", VerifiedProgram::from_bytecode(program))
        .expect("test program should load");
    (runtime, loaded)
}

pub fn print_module(
    write: impl for<'call> Fn(
        &mut HostCallContext<'call>,
        &ValueView<'call>,
        &'static str,
    ) -> Result<(), HostError>
    + Send
    + Sync
    + 'static,
) -> NativeModule {
    io::module(test_library::standard().catalog(), write).unwrap()
}

/// Compile against `std::io` without installing the producer's no-op sink.
pub fn compile_print_test_bytecode(source_text: &str) -> DefinitionMetadata<BytecodeProgram> {
    compile_native_test_bytecode(source_text, &[&print_module(|_, _, _| Ok(()))])
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
    let mut declarations = test_library::standard().declarations().to_vec();
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
    let ir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        options,
    )
    .expect("program MIR lowering should succeed");
    lower_program_to_bytecode(&ir, &Default::default())
        .expect("program bytecode lowering should succeed")
}

/// Explicit standard installation for script fixtures and their source-free products.
pub fn standard_runtime(config: RuntimeConfig) -> Runtime {
    let mut runtime = Runtime::new(config);
    NativeModule::install_all(&test_library::modules().unwrap(), &mut runtime).unwrap();
    runtime
}

pub fn compile_test_bytecode_with_options(
    source_text: &str,
    options: &MirLoweringOptions,
) -> DefinitionMetadata<BytecodeProgram> {
    compile_native_test_bytecode_with_options(source_text, &[], options).into_unverified()
}
