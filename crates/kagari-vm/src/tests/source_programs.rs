use crate::{
    debug::{DebugSession, SourceBreakpoint},
    error::VmError,
    tests::{common::standard_runtime, native_fixtures},
    vm::{JitExecutionStatus, Vm},
};
use kagari_bytecode::{artifact::KbcArtifact, program::BytecodeProgram};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_compiler::bytecode::lower_program_to_bytecode;
use kagari_runtime::{RuntimeConfig, error::RuntimeErrorKind, value::Value};
use kagari_source::source_database::{SourceDatabase, SourceLayer};

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
        .snapshot(sources.snapshot(), &Default::default())
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
                let mut runtime = standard_runtime(RuntimeConfig {
                    ..Default::default()
                });
                let loaded = runtime.load_program("root", program.clone()).unwrap();
                let vm = Vm::new(runtime);
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
    let mut runtime = standard_runtime(RuntimeConfig {
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
