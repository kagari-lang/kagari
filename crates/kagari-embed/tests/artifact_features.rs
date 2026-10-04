//! The same portable artifact is consumed with source, native, or neither feature.
use kagari_bytecode::{
    artifact::{ArtifactBuildOptions, KbcArtifact},
    native_input::PortableMir,
};
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};

use kagari_runtime::value::Value;
#[cfg(feature = "source")]
use kagari_source::source::SourceFile;
use std::{cell::Cell, fs, path::Path, rc::Rc, sync::OnceLock};

// Share application code with the emitter while compiling without its frontend.
#[path = "support/native_enums.rs"]
mod native_enums;
#[path = "support/native_provider.rs"]
mod provider;
#[path = "support/try_carrier.rs"]
mod try_carrier;
// A structurally consistent forgery must still fail installed-contract linking.
#[path = "native_provider_reset/contracts.rs"]
mod contracts;

fn engine(config: EngineConfig, drops: Rc<Cell<usize>>) -> KagariEngine {
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
            fs::write(&path, artifact.to_bytes().unwrap()).unwrap();
        }
        fs::read(path).expect("generate the target fixture with regenerate_feature_artifact")
    })
}

fn artifact() -> KbcArtifact {
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
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
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
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    for _ in 0..3 {
        assert_eq!(
            runtime
                .execute(&loaded, "required_methods", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
        assert!(!runtime.runtime().is_quarantined());
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
                .return_value,
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
                    .return_value,
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
    let mut program = artifact().program;
    let import = program
        .modules
        .iter_mut()
        .flat_map(|module| &mut module.native_imports)
        .find(|import| !import.signature.params.is_empty())
        .unwrap();
    import.signature.params.clear();
    assert!(KbcArtifact::from_program(program, Default::default()).is_err());
}

#[test]
fn native_payload_interpretation_follows_the_feature_boundary() {
    let valid = artifact();
    let malformed = KbcArtifact::from_program(
        valid.program,
        ArtifactBuildOptions {
            portable_mir: Some(PortableMir { bytes: vec![0xff] }),
            ..Default::default()
        },
    )
    .unwrap();
    // The envelope is valid in both builds; only native-enabled preparation reads MIR.
    malformed.validate_for_loader(&Default::default()).unwrap();
    let result =
        PreparedProgram::from_artifact(malformed, &Default::default(), &Default::default());
    #[cfg(feature = "native")]
    assert!(matches!(
        result,
        Err(kagari_embed::program::ProgramPreparationError::NativeInput(
            _
        ))
    ));
    #[cfg(not(feature = "native"))]
    {
        let program = result.unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine(Default::default(), Default::default()).runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap()
                .return_value,
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
    assert_eq!(generated.to_bytes().unwrap(), artifact_bytes());
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
    use std::{ffi::c_void, rc::Rc};
    use {
        kagari_abi::{
            native::{BackendId, BackendTarget, ExecutableEntryPoint, NativeCodeOwner},
            native_call::{JIT_STATUS_OK, JitValue},
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

    unsafe extern "C" fn forty_two(runtime: *const c_void, result: *mut JitValue) -> i32 {
        for offset in 0..2 {
            let status = unsafe { jit_poll_execution(runtime.cast(), offset) };
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
                owner: Rc::new(StaticCode),
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
            .prepare_native(&program, &loaded, "main", &mut Backend, &Default::default())
            .unwrap();
        let report = runtime
            .execute_prepared(&loaded, "main", &[], &context, &prepared)
            .unwrap();
        assert_eq!(report.return_value, Value::I32(42));
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
            "main",
            &mut CraneliftBackend::for_host().unwrap(),
            &Default::default(),
        )
        .unwrap();
    assert!(matches!(prepared, PreparedNativeEntry::Native(_)));
    let report = runtime
        .execute_prepared(&loaded, "main", &[], &context, &prepared)
        .unwrap();
    assert_eq!(report.return_value, Value::I32(42));
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
    assert_eq!(report.return_value, Value::I32(42));
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
    let drops = Rc::new(Cell::new(0));
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let context = ExecutionContext::default();
    let mut runtime = engine(config, drops.clone()).runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    for expected in 1..=3 {
        let result = runtime
            .execute(&loaded, "library_and_object", &[], &context)
            .unwrap();
        assert_eq!(result.return_value, Value::I32(42));
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(runtime.runtime().collect_garbage().unwrap().live_objects, 0);
        assert_eq!(drops.get(), expected);
    }
}

#[test]
fn structurally_valid_library_binding_mismatch_is_rejected_on_load() {
    let mut artifact = artifact();
    let mut changed = false;
    contracts::alter_bindings(&mut artifact, |id| {
        if id
            .path
            .last()
            .is_some_and(|part| part.name == "$foundation_list_sort")
        {
            id.path.last_mut().unwrap().name = "$foundation_list_sort_by".into();
        }
    });
    for module in &artifact.program.modules {
        changed |= module.native_imports.iter().any(|import| {
            import
                .binding
                .path
                .last()
                .is_some_and(|part| part.name == "$foundation_list_sort_by")
                && import.signature.params.len() == 1
        });
    }
    assert!(changed);
    let artifact = KbcArtifact::from_program(artifact.program, Default::default()).unwrap();
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
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
}
