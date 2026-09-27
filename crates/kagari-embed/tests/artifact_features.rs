//! The same portable artifact is consumed with source, native, or neither feature.
use kagari_bytecode::{ArtifactBuildOptions, KbcArtifact, native_input::PortableMir};
use kagari_embed::{ExecutionContext, KagariEngine, program::PreparedProgram};
use kagari_runtime::value::Value;

const ARTIFACT: &[u8] = include_bytes!("fixtures/feature_artifact.kbc");

fn artifact() -> KbcArtifact {
    KbcArtifact::from_bytes(ARTIFACT).unwrap()
}

#[test]
fn portable_artifact_executes_without_source_compilation() {
    let program =
        PreparedProgram::from_artifact(artifact(), &Default::default(), &Default::default())
            .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = KagariEngine::default().runtime(context.clone());
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
        let mut runtime = KagariEngine::default().runtime(context.clone());
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
    use kagari_common::SourceFile;
    let source = SourceFile::new(
        "memory://feature-artifact.kgr",
        include_str!("fixtures/feature_artifact.kgr"),
    );
    let generated = KagariEngine::default()
        .compile_to_artifact(source, Default::default(), Default::default())
        .unwrap();
    assert_eq!(generated.to_bytes().unwrap(), ARTIFACT);
}

#[cfg(feature = "native")]
mod native {
    use super::*;
    use kagari_abi::{
        ids::FunctionRef,
        native::{
            BackendId, BackendTarget, ExecutableEntryPoint, ExecutableFunctionArtifact,
            NativeCodeOwner, NativeCompilationProduct,
        },
        native_call::{JIT_STATUS_OK, JitValue},
    };
    use kagari_codegen::{
        BackendCompileError, BackendConfiguration, BackendFunctionInput, CodegenBackend,
    };
    use kagari_mir::{Constant, Instruction, Terminator};
    use kagari_runtime::{CapabilitySet, LanguageProfile, jit_abi::jit_consume_instruction_step};
    use kagari_vm::JitExecutionStatus;
    use std::{ffi::c_void, rc::Rc};

    #[derive(Debug)]
    struct StaticCode;
    impl NativeCodeOwner for StaticCode {}
    struct Backend;
    unsafe extern "C" fn forty_two(runtime: *const c_void, result: *mut JitValue) -> i32 {
        for offset in 0..2 {
            let status = unsafe { jit_consume_instruction_step(runtime.cast(), offset) };
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
            artifact.entry = ExecutableEntryPoint::Native {
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
            language_profile: LanguageProfile {
                allow_jit: true,
                ..Default::default()
            },
            capabilities: CapabilitySet {
                jit: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut runtime = KagariEngine::default().runtime(context.clone());
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
    use kagari_vm::{JitExecutionStatus, PreparedNativeEntry};
    let program =
        PreparedProgram::from_artifact(artifact(), &Default::default(), &Default::default())
            .unwrap();
    let mut context = ExecutionContext::default();
    context.language_profile.allow_jit = true;
    context.capabilities.jit = true;
    let mut runtime = KagariEngine::default().runtime(context.clone());
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
}
