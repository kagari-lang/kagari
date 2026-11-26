//! Checked bytecode fixture preparation for standard-provider integration.
#[path = "../../../kagari-runtime/tests/support/bytecode.rs"]
pub(super) mod bytecode;
pub(super) mod native;

use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_compiler::{
    bytecode::lower_program_to_bytecode,
    source::program::{InstallationTraitRequirements, lower_program_with_installation},
};
use kagari_hir::{compile_input::program::CheckedCompilation, program::CheckedProgram};
use kagari_runtime::{
    Runtime, execution::owned::DriveResult, module::ExecutableModule, native::module::NativeModule,
    value::carriers::RootedValue,
};
use std::num::NonZeroUsize;

/// Include mandatory trait identities for participating importers. Lowering derives
/// their ABIs from the checked owners, not the installation's reference contracts.
pub(super) fn lower_for_installation<'a>(
    checked: &CheckedProgram,
    modules: impl IntoIterator<Item = &'a NativeModule>,
) -> DefinitionMetadata<BytecodeProgram> {
    let compilation = CheckedCompilation::from_program(checked, &Default::default()).unwrap();
    let installation = InstallationTraitRequirements {
        by_importer: modules
            .into_iter()
            .filter_map(|module| {
                let paths = module.required_source_traits();
                let owner = &module.declaration().identity;
                (!paths.is_empty()
                    && compilation
                        .modules()
                        .iter()
                        .any(|m| m.source().module_identity() == owner))
                .then(|| (owner.clone(), paths.clone()))
            })
            .collect(),
    };
    let mir =
        lower_program_with_installation(&compilation, &Default::default(), &installation, None)
            .unwrap();
    lower_program_to_bytecode(&mir, &Default::default())
        .unwrap()
        .into_unverified()
}

/// Drive a checked public script entry with retained arguments and result roots.
pub(super) fn invoke(
    runtime: &mut Runtime,
    module: &ExecutableModule,
    name: &str,
    arguments: &[RootedValue],
) -> RootedValue {
    let execution = runtime
        .start(module, name, arguments, Default::default())
        .unwrap();
    loop {
        match runtime
            .drive(&execution, NonZeroUsize::new(1024).unwrap())
            .unwrap()
        {
            DriveResult::Runnable => {}
            DriveResult::Waiting => panic!("synchronous fixture unexpectedly waited"),
            DriveResult::Complete(result) => return result.unwrap(),
        }
    }
}
