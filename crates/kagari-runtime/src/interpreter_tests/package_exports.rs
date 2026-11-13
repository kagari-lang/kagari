//! Direct host entry routes consume the same source-free public export evidence.

use crate::error::reason::ErrorReason;
use crate::execution::{native::PreparedNativeEntry, owned::DriveResult};
use crate::{
    Runtime,
    module::{VerifiedProgram, entry::ProgramEntry},
};
use kagari_abi::native::BackendId;
use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::identity::map::DefinitionContext;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use std::num::NonZeroUsize;

#[test]
fn direct_calls_use_public_paths_and_require_explicit_private_entry_selection() {
    let mut source = SourceDatabase::default();
    let root = source
        .set(
            "exports.kgr",
            include_str!("../../tests/fixtures/execution/direct-export-routes.kgr").into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    let checked = analysis
        .snapshot(source.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    let mir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
    let context = DefinitionContext::new().unwrap();
    let bytecode = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let artifact =
        KbcArtifact::from_program(bytecode, Default::default(), &context, &Default::default())
            .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    let receiver = DefinitionContext::new().unwrap();
    let decoded = KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&receiver, &Default::default())
        .unwrap()
        .into_records()
        .into_verified(&Default::default(), &receiver, &Default::default())
        .unwrap();
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_verified_program(
            "exports",
            VerifiedProgram::from_bytecode(decoded.into_bytecode()),
        )
        .unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let roots_before = vm.gc_stats().root_registrations;
    let fallback = PreparedNativeEntry::Unsupported {
        backend: BackendId::new("export-fixture"),
        diagnostics: vec!["interpreter route".into()],
    };
    let (child, _) = loaded.exported_function("answer").unwrap();
    for owner in [&loaded, &child] {
        for path in [
            "main",
            "value",
            "implementation::value",
            "template",
            "secret",
        ] {
            assert!(
                matches!(vm.execute_module(owner, path, vm.execution_options()), Err(error) if matches!(error.reason(), ErrorReason::MissingFunction(name) if name == path))
            );
            assert!(
                matches!(vm.execute_module_typed::<(), i32>(owner, path, ()), Err(error) if matches!(error.reason(), ErrorReason::MissingFunction(name) if name == path))
            );
            assert!(
                matches!(vm.start(owner, path, &[], Default::default()), Err(error) if matches!(error.reason(), ErrorReason::MissingFunction(name) if name == path))
            );
            assert!(
                matches!(vm.execute_prepared_module(owner, path, &fallback, vm.execution_options()), Err(error) if matches!(error.reason(), ErrorReason::MissingFunction(name) if name == path))
            );
        }
        for (selection, expected) in [
            (ProgramEntry::Export("again::answer"), 42),
            (ProgramEntry::Export("wrapper"), 7),
            (ProgramEntry::Main, 3),
        ] {
            let report = vm
                .execute_module(owner, selection, vm.execution_options())
                .unwrap();
            assert_eq!(report.return_value.as_i32(), Some(expected));
            assert_eq!(
                vm.execute_module_typed::<(), i32>(owner, selection, ())
                    .unwrap(),
                expected
            );
            let report = vm
                .execute_prepared_module(owner, selection, &fallback, vm.execution_options())
                .unwrap();
            assert_eq!(report.return_value.as_i32(), Some(expected));
            let execution = vm.start(owner, selection, &[], Default::default()).unwrap();
            loop {
                match vm.drive(&execution, NonZeroUsize::new(1).unwrap()).unwrap() {
                    DriveResult::Runnable => continue,
                    DriveResult::Complete(result) => {
                        assert_eq!(result.unwrap().as_i32(), Some(expected));
                        break;
                    }
                    DriveResult::Waiting => panic!("synchronous fixture"),
                }
            }
        }
        assert_eq!(
            vm.execute_module_typed::<(i32,), i32>(owner, "echo", (19,))
                .unwrap(),
            19
        );
    }
    assert_eq!(vm.gc_stats().root_registrations, roots_before);
}
