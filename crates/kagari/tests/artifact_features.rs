//! The same portable artifact is consumed with source, native, or neither feature.
use kagari::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_bytecode::{
    artifact::{ArtifactBuildOptions, KbcArtifact},
    native_input::PortableMir,
    program::{identity, verify_program},
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_runtime::module::entry::ProgramEntry;
use kagari_types::ty::Ty;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use kagari_runtime::value::Value;
#[cfg(feature = "source")]
use kagari_source::source::SourceFile;
use std::{fs, path::Path, sync::OnceLock};

// Share application code with the emitter while compiling without its frontend.
#[path = "support/native_enums.rs"]
mod native_enums;
#[path = "support/native_provider.rs"]
mod provider;
#[path = "support/try_carrier.rs"]
mod try_carrier;
// A structurally consistent forgery must still fail installed-contract linking.
#[path = "support/provider_contracts.rs"]
mod contracts;

#[path = "artifact_features/async_tasks.rs"]
mod async_tasks;

fn engine(config: EngineConfig, drops: Arc<AtomicUsize>) -> KagariEngine {
    {
        let mut builder = KagariEngine::builder().unwrap();
        builder.config(config);
        builder.install(provider::module(drops)).unwrap();
        builder.install(native_enums::module().unwrap()).unwrap();
        builder.install(try_carrier::module().unwrap()).unwrap();
        builder.build().unwrap()
    }
}

fn artifact_bytes() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let path = workspace.join("target/fixtures/feature_artifact.kbc");
        #[cfg(feature = "source")]
        {
            let source = SourceFile::new(
                "memory://feature-artifact.kgr",
                include_str!("fixtures/feature_artifact.kgr"),
            );
            let artifact = engine(Default::default(), Default::default())
                .compile_to_artifact(source, Default::default())
                .unwrap();
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(
                &path,
                artifact.records().to_bytes(artifact.definitions()).unwrap(),
            )
            .unwrap();
        }
        fs::read(path).expect("generate the target fixture with regenerate_feature_artifact")
    })
}

fn artifact() -> DefinitionMetadata<KbcArtifact> {
    KbcArtifact::from_bytes(artifact_bytes()).unwrap()
}

#[test]
fn portable_artifact_executes_without_source_compilation() {
    let program =
        PreparedProgram::from_artifact(artifact(), &Default::default(), &Default::default())
            .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine(Default::default(), Default::default()).runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn source_free_shared_operator_contract_executes() {
    let program =
        PreparedProgram::from_artifact(artifact(), &Default::default(), &Default::default())
            .unwrap();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let context = ExecutionContext::default();
    let mut runtime = engine(config, Default::default()).runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let result = runtime
        .execute(&loaded, "shared_operator_values", &[], &context)
        .unwrap();
    assert_eq!(
        result
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    drop(result);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    assert!(!runtime.runtime().is_quarantined());
}

#[test]
fn source_free_native_bindings_execute_and_release_scopes() {
    let program =
        PreparedProgram::from_artifact(artifact(), &Default::default(), &Default::default())
            .unwrap();
    let context = ExecutionContext::default();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let mut runtime = engine(config, Default::default()).runtime(context.clone());
    let mut loaded = runtime.load_program(&program, Default::default()).unwrap();
    for _ in 0..3 {
        for entry in ["required_methods", "fixed_array_construction"] {
            assert_eq!(
                runtime
                    .execute(&loaded, entry, &[], &context)
                    .unwrap()
                    .return_value
                    .value(runtime.runtime().gc())
                    .expect("retained execution result"),
                Value::I32(42)
            );
        }
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
        assert!(!runtime.runtime().is_quarantined());
        // Release the version's constant roots before checking for leaked native
        // scopes/results; the replacement cache has not been materialized.
        loaded = runtime
            .reload_program(&loaded, &program, Default::default())
            .unwrap();
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
}

#[test]
fn source_free_native_enums_retain_nested_payloads() {
    let program =
        PreparedProgram::from_artifact(artifact(), &Default::default(), &Default::default())
            .unwrap();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let mut runtime = engine(config, Default::default()).runtime(Default::default());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    for _ in 0..3 {
        assert_eq!(
            runtime
                .execute(&loaded, "native_enum_values", &[], &Default::default())
                .unwrap()
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
}

#[test]
fn source_free_try_carriers_execute_with_selected_residual_calls() {
    let prepared =
        PreparedProgram::from_artifact(artifact(), &Default::default(), &Default::default())
            .unwrap();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let mut runtime = engine(config, Default::default()).runtime(Default::default());
    let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
    for entry in ["native_propagation_values", "standard_residual_values"] {
        for _ in 0..3 {
            assert_eq!(
                runtime
                    .execute(&loaded, entry, &[], &Default::default())
                    .unwrap()
                    .return_value
                    .value(runtime.runtime().gc())
                    .expect("retained execution result"),
                Value::I32(42)
            );
            assert_eq!(runtime.runtime().gc().active_roots(), 0);
            assert_eq!(
                runtime.runtime().resources().counters().current_call_depth,
                0
            );
            runtime.runtime().collect_garbage().unwrap();
            assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
        }
    }
}

#[test]
fn source_free_native_imports_reject_forged_signatures() {
    let (context, artifact) = editable_artifact();
    let definitions = context.snapshot();
    for corrupt in 0..3 {
        let mut program = artifact.program.clone();
        let import = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.native_imports)
            .find(|import| match corrupt {
                0 => !import.signature.params.is_empty(),
                1 => definitions
                    .resolve(import.binding)
                    .unwrap()
                    .last()
                    .is_some_and(|part| part.name == "$foundation_list_push_fluent"),
                _ => {
                    matches!(import.signature.params.as_slice(), [Ty::Array(_)])
                        && matches!(import.signature.result, Ty::NativeObject(_))
                }
            })
            .unwrap();
        match corrupt {
            0 => import.signature.params.clear(),
            1 => {
                let Ty::NativeObject(nominal) = &import.signature.params[0] else {
                    panic!("Vec receiver")
                };
                import.signature.params[0] = Ty::Array(Box::new(nominal.arguments[0].clone()));
            }
            _ => import.signature.params[0] = import.signature.result.clone(),
        }
        assert!(
            KbcArtifact::from_program(program, Default::default(), &context).is_err(),
            "forgery {corrupt}"
        );
    }
}

#[test]
fn native_payload_interpretation_follows_the_feature_boundary() {
    let (context, valid) = editable_artifact();
    let malformed = KbcArtifact::from_program(
        valid.program,
        ArtifactBuildOptions {
            portable_mir: Some(PortableMir { bytes: vec![0xff] }),
            ..Default::default()
        },
        &context,
    )
    .unwrap();
    // The envelope is valid in both builds; only native-enabled preparation reads MIR.
    malformed
        .validate_for_loader(&Default::default(), &context)
        .unwrap();
    let malformed =
        DefinitionMetadata::checked(context.snapshot(), malformed, &Default::default()).unwrap();
    let result =
        PreparedProgram::from_artifact(malformed, &Default::default(), &Default::default());
    #[cfg(feature = "native")]
    assert!(matches!(
        result,
        Err(kagari::program::ProgramPreparationError::NativeInput(_))
    ));
    #[cfg(not(feature = "native"))]
    {
        let program = result.unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine(Default::default(), Default::default()).runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, ProgramEntry::Main, &[], &context)
                .unwrap()
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
    }
}

#[cfg(feature = "source")]
#[test]
fn portable_fixture_matches_source_emission() {
    let source = SourceFile::new(
        "memory://feature-artifact.kgr",
        include_str!("fixtures/feature_artifact.kgr"),
    );
    let generated = engine(Default::default(), Default::default())
        .compile_to_artifact(source, Default::default())
        .unwrap();
    assert_eq!(
        generated
            .records()
            .to_bytes(generated.definitions())
            .unwrap(),
        artifact_bytes()
    );
}

#[cfg(feature = "native")]
mod native {
    use super::*;
    use kagari_codegen::{
        BackendConfiguration, BackendFunctionInput, CodegenBackend, diagnostic::BackendCompileError,
    };
    use kagari_mir::instruction::{Constant, Instruction, Terminator};
    use kagari_runtime::jit_abi::jit_poll_execution;
    use kagari_vm::vm::JitExecutionStatus;
    use std::sync::Arc;
    use {
        kagari_abi::{
            native::{BackendId, BackendTarget, ExecutableEntryPoint, NativeCodeOwner},
            native_call::{JIT_STATUS_OK, JitValue, NativeInvocationContext},
        },
        kagari_contract::{
            ids::FunctionRef,
            native::{ExecutableFunctionArtifact, NativeCompilationProduct},
        },
    };

    #[derive(Debug)]
    struct StaticCode;

    impl NativeCodeOwner for StaticCode {}

    struct Backend;

    unsafe extern "C" fn forty_two(
        runtime: *const NativeInvocationContext,
        result: *mut JitValue,
    ) -> i32 {
        for offset in 0..2 {
            let status = unsafe { jit_poll_execution((*runtime).runtime.cast(), offset) };
            if status != JIT_STATUS_OK {
                return status;
            }
        }
        unsafe {
            result.write(JitValue::i32(42));
        }
        JIT_STATUS_OK
    }

    // SAFETY: only the exact two-point constant function is accepted; the process-
    // lifetime C-ABI fixture preserves its charges and result without heap access.
    unsafe impl CodegenBackend for Backend {
        fn configuration(&self) -> BackendConfiguration {
            BackendConfiguration {
                backend: BackendId::new("artifact-fixture"),
                target: BackendTarget::new("host-fixture", usize::BITS as u8),
                options: vec![],
            }
        }

        fn compile_function(
            &mut self,
            input: BackendFunctionInput<'_>,
        ) -> Result<NativeCompilationProduct, BackendCompileError> {
            assert!(input.function().params.is_empty());
            assert_eq!(input.function().blocks.len(), 1);
            let block = &input.function().blocks[0];
            let [
                Instruction::LoadConst {
                    dst,
                    constant: Constant::I32(42),
                },
            ] = block.instructions.as_slice()
            else {
                panic!("constant fixture");
            };
            assert!(
                matches!(block.terminator, Some(Terminator::Return(Some(value))) if value == *dst)
            );
            let config = self.configuration();
            let mut artifact = ExecutableFunctionArtifact::new(
                config.backend,
                config.target,
                FunctionRef::new(input.function_ref().index()),
            );
            artifact.code.entry = ExecutableEntryPoint::Native {
                symbol: "forty_two".into(),
                address: forty_two as *const () as usize,
            };
            Ok(NativeCompilationProduct {
                artifact,
                owner: Arc::new(StaticCode),
            })
        }
    }

    #[test]
    fn native_artifact_preparation_and_execution_need_no_frontend() {
        let program =
            PreparedProgram::from_artifact(artifact(), &Default::default(), &Default::default())
                .unwrap();
        assert!(program.has_native_input());
        let context = ExecutionContext {
            ..Default::default()
        };
        let mut runtime = engine(Default::default(), Default::default()).runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let prepared = runtime
            .prepare_native(
                &program,
                &loaded,
                ProgramEntry::Main,
                &mut Backend,
                &Default::default(),
            )
            .unwrap();
        let report = runtime
            .execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
            .unwrap();
        assert_eq!(
            report
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
        assert_eq!(report.jit.unwrap().status, JitExecutionStatus::Native);
    }
}

#[cfg(feature = "native")]
#[test]
fn real_cranelift_compiles_portable_artifact_without_source() {
    use kagari_codegen_cranelift::CraneliftBackend;
    use kagari_vm::vm::{JitExecutionStatus, native::PreparedNativeEntry};
    let program =
        PreparedProgram::from_artifact(artifact(), &Default::default(), &Default::default())
            .unwrap();
    let context = ExecutionContext::default();

    let mut runtime = engine(Default::default(), Default::default()).runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let prepared = runtime
        .prepare_native(
            &program,
            &loaded,
            ProgramEntry::Main,
            &mut CraneliftBackend::for_host().unwrap(),
            &Default::default(),
        )
        .unwrap();
    assert!(matches!(prepared, PreparedNativeEntry::Native(_)));
    let report = runtime
        .execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        .unwrap();
    assert_eq!(
        report
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(report.jit.unwrap().status, JitExecutionStatus::Native);
    let prepared = runtime
        .prepare_native(
            &program,
            &loaded,
            "required_methods",
            &mut CraneliftBackend::for_host().unwrap(),
            &Default::default(),
        )
        .unwrap();
    assert!(matches!(prepared, PreparedNativeEntry::Unsupported { .. }));
    let report = runtime
        .execute_prepared(&loaded, "required_methods", &[], &context, &prepared)
        .unwrap();
    assert_eq!(
        report
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(
        report.jit.unwrap().status,
        JitExecutionStatus::InterpreterFallback
    );
}

#[test]
fn source_free_algorithms_and_application_payload_share_the_native_boundary() {
    let program =
        PreparedProgram::from_artifact(artifact(), &Default::default(), &Default::default())
            .unwrap();
    let drops = Arc::new(AtomicUsize::new(0));
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let context = ExecutionContext::default();
    let mut runtime = engine(config, drops.clone()).runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    for expected in 1..=3 {
        let result = runtime
            .execute(&loaded, "library_and_object", &[], &context)
            .unwrap();
        assert_eq!(
            result
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
        drop(result);
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(runtime.runtime().collect_garbage().unwrap().live_objects, 0);
        assert_eq!(drops.load(Ordering::SeqCst), expected);
    }
}

#[test]
fn structurally_valid_library_binding_mismatch_is_rejected_on_load() {
    let (context, mut artifact) = editable_artifact();
    let definitions = context.snapshot();
    let mut changed = false;
    contracts::alter_bindings(&mut artifact, |id| {
        let view = definitions.resolve(*id).unwrap();
        if view
            .last()
            .is_some_and(|part| part.name == "$foundation_list_sort")
        {
            let segment = view.last().unwrap();
            *id = context
                .intern_child(
                    definitions.parent(*id).unwrap().unwrap(),
                    segment.kind,
                    "$foundation_list_sort_by",
                    segment.occurrence,
                )
                .unwrap();
        }
    });
    let definitions = context.snapshot();
    for module in &artifact.program.modules {
        changed |= module.native_imports.iter().any(|import| {
            definitions
                .resolve(import.binding)
                .unwrap()
                .last()
                .is_some_and(|part| part.name == "$foundation_list_sort_by")
                && import.signature.params.len() == 1
        });
    }
    assert!(changed);
    let artifact =
        KbcArtifact::from_program(artifact.program, Default::default(), &context).unwrap();
    let artifact =
        DefinitionMetadata::checked(context.snapshot(), artifact, &Default::default()).unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let mut runtime = engine(Default::default(), Default::default()).runtime(Default::default());
    assert!(runtime.load_program(&program, Default::default()).is_err());
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}

#[test]
fn source_free_converted_residual_keeps_its_original_failure_origin() {
    let prepared =
        PreparedProgram::from_artifact(artifact(), &Default::default(), &Default::default())
            .unwrap();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let mut runtime = engine(config, Default::default()).runtime(Default::default());
    let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
    let report = runtime
        .execute(&loaded, "converted_error_origin", &[], &Default::default())
        .unwrap();
    let failure = report
        .failure
        .expect("a returned Err is successful execution with a failure preview");
    assert_eq!(failure.message, "42");
    assert_eq!(
        failure
            .trace
            .frames
            .iter()
            .map(|frame| frame.function_name.as_str())
            .collect::<Vec<_>>(),
        ["origin_residual", "converted_error_origin"]
    );
    assert!(
        failure.trace.frames[0]
            .source_uri
            .ends_with("feature-artifact.kgr")
    );
    drop(report.return_value);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
}

fn editable_artifact() -> (DefinitionContext, KbcArtifact) {
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let table = identity::collect(
        &artifact.program.modules.iter().collect::<Vec<_>>(),
        &context,
    )
    .unwrap();
    verify_program(&artifact.program, &table).unwrap();
    (context, artifact)
}
