//! Direct host entry routes consume the same source-free public export evidence.

use crate::{
    error::VmError,
    vm::{Vm, native::PreparedNativeEntry, owned::DriveResult},
};
use kagari_abi::native::BackendId;
use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::identity::map::DefinitionContext;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime,
    module::{VerifiedProgram, entry::ProgramEntry},
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use std::num::NonZeroUsize;

#[test]
fn direct_calls_use_public_paths_and_require_explicit_private_entry_selection() {
    let mut source = SourceDatabase::default();
    let root = source
        .set(
            "exports.kgr",
            r#"
            mod implementation {
                pub fn value() -> i32 { 42 }
                pub fn identity(value: i32) -> i32 { value }
                fn secret() -> bool { true }
            }
            pub use crate::implementation::value as answer;
            pub use crate::implementation::identity as echo;
            pub use crate as again;
            pub fn template<T>(value: T) -> T { value }
            pub fn wrapper() -> i32 { template(7) }
            fn main() -> i32 { 3 }
            "#
            .into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    let checked = analysis
        .snapshot(source.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
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
    let vm = Vm::new(runtime);
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
                matches!(vm.execute(owner, path), Err(VmError::MissingFunction(name)) if name == path)
            );
            assert!(
                matches!(vm.execute_typed::<(), i32>(owner, path, ()), Err(VmError::MissingFunction(name)) if name == path)
            );
            assert!(
                matches!(vm.start(owner, path, &[], Default::default()), Err(VmError::MissingFunction(name)) if name == path)
            );
            assert!(
                matches!(vm.execute_prepared(owner, path, &fallback), Err(VmError::MissingFunction(name)) if name == path)
            );
        }
        for (selection, expected) in [
            (ProgramEntry::Export("again::answer"), 42),
            (ProgramEntry::Export("wrapper"), 7),
            (ProgramEntry::Main, 3),
        ] {
            let report = vm.execute(owner, selection).unwrap();
            assert_eq!(
                report
                    .return_value
                    .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
                    .ok(),
                Some(Value::I32(expected))
            );
            assert_eq!(
                vm.execute_typed::<(), i32>(owner, selection, ()).unwrap(),
                expected
            );
            let report = vm.execute_prepared(owner, selection, &fallback).unwrap();
            assert_eq!(
                report
                    .return_value
                    .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
                    .ok(),
                Some(Value::I32(expected))
            );
            let execution = vm.start(owner, selection, &[], Default::default()).unwrap();
            loop {
                match vm.drive(&execution, NonZeroUsize::new(1).unwrap()).unwrap() {
                    DriveResult::Runnable => continue,
                    DriveResult::Complete(result) => {
                        assert_eq!(
                            result
                                .unwrap()
                                .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
                                .ok(),
                            Some(Value::I32(expected))
                        );
                        break;
                    }
                    DriveResult::Waiting => panic!("synchronous fixture"),
                }
            }
        }
        assert_eq!(
            vm.execute_typed::<(i32,), i32>(owner, "echo", (19,))
                .unwrap(),
            19
        );
    }
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}
