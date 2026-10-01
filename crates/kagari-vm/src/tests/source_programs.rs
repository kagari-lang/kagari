use crate::{
    debug::{DebugSession, SourceBreakpoint},
    tests::native_fixtures,
    vm::{JitExecutionStatus, Vm},
};
use kagari_bytecode::{
    artifact::KbcArtifact,
    program::{BytecodeProgram, verify_program},
};
use {
    crate::error::VmError,
    kagari_bytecode::instruction::{BytecodeInstruction, CallTarget},
    kagari_runtime::error::RuntimeErrorKind,
};

use kagari_abi::native_import::EngineNativeOperation;

use kagari_common::{
    identity::{ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_compiler::bytecode::lower_program_to_bytecode;
use {
    kagari_common::capability::CapabilitySet,
    kagari_runtime::{
        Runtime, RuntimeConfig,
        security::{DebugVisibilityPolicy, LanguageProfile, SecurityContext},
        value::Value,
    },
};

fn fixture(dependency_source: &str) -> BytecodeProgram {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        ("dependency", dependency_source),
        (
            "root",
            "use pkg::dependency::answer; fn main() -> i32 { answer() }",
        ),
    ] {
        let uri = format!("mem://{name}");
        sources
            .bind_module(
                &uri,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        root = Some(sources.set(&uri, text.into(), SourceLayer::Base).unwrap());
    }
    let snapshot = kagari_hir::analysis::AnalysisDatabase::default()
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let checked = snapshot
        .check_program(root.unwrap(), &Default::default())
        .unwrap();
    let ir = kagari_compiler::source::program::lower_program_to_mir(&checked, &Default::default())
        .unwrap();
    lower_program_to_bytecode(&ir).unwrap()
}

#[test]
fn source_and_artifact_cross_module_calls_match_interpreter_and_jit_fallback() {
    for overflow in [false, true] {
        let source = fixture(if overflow {
            "pub fn answer() -> i32 { 2147483647 + 1 }"
        } else {
            "pub fn answer() -> i32 { 40 + 2 }"
        });
        let artifact = KbcArtifact::from_program(source.clone(), Default::default()).unwrap();
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        decoded.validate_for_loader(&Default::default()).unwrap();
        for program in [source, decoded.program] {
            for jit in [false, true] {
                let mut runtime = Runtime::new(RuntimeConfig {
                    security: SecurityContext {
                        profile: LanguageProfile {
                            allow_jit: true,
                            ..Default::default()
                        },
                        capabilities: CapabilitySet {
                            jit: true,
                            ..Default::default()
                        },
                    },
                    ..Default::default()
                });
                let loaded = runtime.load_program("root", program.clone()).unwrap();
                let mut vm = Vm::new(runtime);
                let report = if jit {
                    vm.execute_prepared(&loaded, "main", &native_fixtures::unsupported())
                } else {
                    vm.execute(&loaded, "main")
                };
                if overflow {
                    assert!(
                        matches!(report, Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::ScriptTrap)
                    );
                    continue;
                }
                let report = report.unwrap();
                assert_eq!(report.return_value, Value::I32(42));
                if jit {
                    assert_eq!(
                        report.jit.unwrap().status,
                        JitExecutionStatus::InterpreterFallback
                    );
                }
            }
        }
    }
}

#[test]
fn cross_module_debug_frames_keep_their_member_identity() {
    let mut runtime = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_debugger: true,
                ..Default::default()
            },
            capabilities: CapabilitySet {
                debug_attach: true,
                debug_breakpoints: true,
                debug_pause: true,
                debug_stack_inspection: true,
                debug_value_inspection: true,
                ..Default::default()
            },
        },
        debug_visibility: DebugVisibilityPolicy {
            allow_all_modules: true,
            ..Default::default()
        },
        ..Default::default()
    });
    let loaded = runtime
        .load_program("root", fixture("pub fn answer() -> i32 { 40 + 2 }"))
        .unwrap();
    let dependency = loaded
        .members()
        .find(|member| member.bytecode.identity.path == ["dependency"])
        .unwrap();
    let mut session = DebugSession::new(&runtime).unwrap();
    session
        .add_breakpoint(SourceBreakpoint::at_source_offset(
            &dependency.name,
            "pub fn answer() -> i32 { ".len(),
        ))
        .unwrap();
    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session).unwrap();
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
    let session = vm.debug_session().unwrap();
    let pause = session
        .pauses()
        .iter()
        .find(|pause| pause.frames.len() == 2)
        .expect("pause inside dependency call");
    assert_eq!(pause.frames[0].module_id, loaded.id);
    assert_eq!(pause.frames[1].module_id, dependency.id);
    assert_eq!(pause.frames[0].function_name, "main");
    assert_eq!(pause.frames[1].function_name, "answer");
    assert!(
        session
            .resolved_breakpoints()
            .iter()
            .all(|point| point.module_id == dependency.id)
    );
}

#[test]
fn direct_native_imports_run_from_source_and_decoded_artifacts() {
    let program = fixture(
        r#"
        pub fn answer() -> i32 {
            if "abc".len_bytes() == 3usize {
                std::math::clamp((42i32).wrapping_add(0), 0, 100)
            } else { 0 }
        }
    "#,
    );
    let imports: Vec<_> = program
        .modules
        .iter()
        .flat_map(|module| &module.native_imports)
        .collect();
    assert_eq!(imports.len(), 3);
    assert!(
        imports
            .iter()
            .all(|import| matches!(import.resolve(), Some(EngineNativeOperation::Direct(_))))
    );
    assert!(imports.iter().any(|import| !import.requirements.is_empty()));
    let artifact = KbcArtifact::from_program(program.clone(), Default::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    decoded.validate_for_loader(&Default::default()).unwrap();
    for program in [program, decoded.program] {
        let mut runtime = Runtime::default();
        let loaded = runtime.load_program("native-imports", program).unwrap();
        let mut vm = Vm::new(runtime);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        assert_eq!(vm.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn forged_native_imports_reject_versions_providers_signatures_and_obligations() {
    let program = fixture("pub fn answer() -> i32 { std::math::clamp(42, 0, 100) }");
    let member = program
        .modules
        .iter()
        .position(|module| !module.native_imports.is_empty())
        .unwrap();
    for corrupt in 0..7 {
        let mut forged = program.clone();
        let import = &mut forged.modules[member].native_imports[0];
        match corrupt {
            0 => import.binding_version += 1,
            1 => import.signature.params.pop().map(|_| ()).unwrap(),
            2 => {
                import.signature.result =
                    kagari_abi::types::AbiType::Builtin(kagari_abi::scalar::BuiltinType::Bool)
            }
            3 => import.requirements.clear(),
            4 => import.instance.arguments.clear(),
            5 => import.instance.declaration.path.last_mut().unwrap().name = "unpublished".into(),
            6 => {
                let instruction = forged.modules[member]
                    .functions
                    .iter_mut()
                    .flat_map(|function| &mut function.instructions)
                    .find(|instruction| {
                        matches!(
                            instruction,
                            BytecodeInstruction::Call {
                                callee: CallTarget::Native(
                                    kagari_abi::callable::NativeCall::Provider(_)
                                ),
                                ..
                            }
                        )
                    })
                    .unwrap();
                let BytecodeInstruction::Call { callee, .. } = instruction else {
                    unreachable!()
                };
                *callee = CallTarget::Native(kagari_abi::callable::NativeCall::Host(
                    kagari_bytecode::HostImportId::new(0),
                ));
            }
            _ => unreachable!(),
        }
        assert!(
            verify_program(&forged).is_err(),
            "accepted corruption {corrupt}"
        );
        assert!(
            KbcArtifact::from_program(forged.clone(), Default::default()).is_err(),
            "artifact accepted corruption {corrupt}"
        );
        let mut runtime = Runtime::default();
        assert!(
            runtime.load_program("forged-native", forged).is_err(),
            "runtime accepted corruption {corrupt}"
        );
        assert_eq!(runtime.gc().active_roots(), 0);
    }
}

#[test]
fn concrete_collection_native_signatures_preserve_element_types() {
    let program = fixture(
        r#"pub fn answer() -> i32 {
        val values = [40]; values.push(2);
        values[0] + values.get(1usize).unwrap_or(0)
    }"#,
    );
    assert!(
        program
            .modules
            .iter()
            .flat_map(|module| &module.native_imports)
            .any(|import| matches!(
                import.signature.params.first(),
                Some(kagari_abi::types::AbiType::Array(_, _))
            ))
    );
    let artifact = KbcArtifact::from_program(program.clone(), Default::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    for program in [program, decoded.program] {
        let mut runtime = Runtime::default();
        let loaded = runtime.load_program("native-collections", program).unwrap();
        let mut vm = Vm::new(runtime);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        assert_eq!(vm.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn hash_storage_native_imports_carry_and_validate_selected_witnesses() {
    let program = fixture(
        r#"pub fn answer() -> i32 {
        val values: LinkedHashSet<i32> = LinkedHashSet::new();
        values.insert(42);
        if values.contains(42) { 42 } else { 0 }
    }"#,
    );
    let (owner, import) = program
        .modules
        .iter()
        .enumerate()
        .find_map(|(index, module)| {
            module
                .native_imports
                .iter()
                .position(|import| !import.witnesses.is_empty())
                .map(|import| (index, import))
        })
        .unwrap();
    assert!(
        program.modules[owner].native_imports[import]
            .witnesses
            .len()
            >= 2
    );
    let mut missing = program.clone();
    missing.modules[owner].native_imports[import]
        .witnesses
        .clear();
    assert!(verify_program(&missing).is_err());
    let mut forged = program.clone();
    forged.modules[owner].native_imports[import].witnesses[0].implementation =
        kagari_abi::native_import::NativeWitnessImplementation::Host;
    assert!(verify_program(&forged).is_err());
    let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_program("native-witnesses", decoded.program)
        .unwrap();
    let mut vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn low_level_storage_writes_reject_element_type_forgery() {
    use kagari_abi::{
        callable::NativeCall, scalar::BuiltinType, standard::RuntimePrimitive, types::AbiType,
    };
    use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget, Register};
    let mut program = fixture(
        r#"pub fn answer() -> i32 {
        val values = [[42]];
        val wrong = ["bad"];
        wrong.push("still bad");
        values.push([1]);
        values[0][0]
    }"#,
    );
    let member = program
        .modules
        .iter()
        .position(|module| {
            module.native_imports.iter().any(|import| {
                import.resolve() == Some(EngineNativeOperation::Direct(RuntimePrimitive::ArrayPush))
            })
        })
        .unwrap();
    let imports = program.modules[member].native_imports.clone();
    let function = program.modules[member]
        .functions
        .iter_mut()
        .find(|function| function.name == "answer")
        .unwrap();
    let wrong = function.metadata.semantic.registers.iter().find_map(|(index,ty)| matches!(ty,AbiType::Array(item,_) if item.as_ref() == &AbiType::Builtin(BuiltinType::String)).then_some(Register::new(*index))).unwrap();
    let instruction = function.instructions.iter_mut().find(|instruction| matches!(instruction,BytecodeInstruction::Call {callee:CallTarget::Native(id),..} if imports[id.index()].resolve()==Some(EngineNativeOperation::Direct(RuntimePrimitive::ArrayPush)) && matches!(&imports[id.index()].signature.params[0],AbiType::Array(item,_) if matches!(item.as_ref(),AbiType::Array(_, _))))).unwrap();
    let BytecodeInstruction::Call { callee, .. } = instruction else {
        unreachable!()
    };
    *callee = CallTarget::RuntimePrimitive(RuntimePrimitive::ArrayPush);
    // The low-level form still has a valid physical contract and semantic slot
    // facts; this exercises the storage verifier independently of native imports.
    verify_program(&program).unwrap();
    let function = program.modules[member]
        .functions
        .iter_mut()
        .find(|function| function.name == "answer")
        .unwrap();
    let BytecodeInstruction::Call { args, .. } = function
        .instructions
        .iter_mut()
        .find(|instruction| {
            matches!(
                instruction,
                BytecodeInstruction::Call {
                    callee: CallTarget::RuntimePrimitive(RuntimePrimitive::ArrayPush),
                    ..
                }
            )
        })
        .unwrap()
    else {
        unreachable!()
    };
    args[1] = wrong;
    assert!(verify_program(&program).is_err());
    assert!(KbcArtifact::from_program(program.clone(), Default::default()).is_err());
    let mut runtime = Runtime::default();
    assert!(runtime.load_program("forged-storage", program).is_err());
    assert_eq!(runtime.gc().active_roots(), 0);
}
