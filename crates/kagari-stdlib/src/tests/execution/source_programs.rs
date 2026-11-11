#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use kagari_runtime::{
    RuntimeConfig,
    debug::{DebugSession, SourceBreakpoint},
    error::RuntimeErrorKind,
    execution::JitExecutionStatus,
    module::entry::ProgramEntry,
    value::Value,
};

use crate::tests::execution::{
    common::{round_trip_bytecode, standard_runtime},
    native_fixtures,
};

use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::{ModuleIdentity, PackageId, metadata::DefinitionMetadata};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_source::source_database::{SourceDatabase, SourceLayer};

fn fixture(dependency_source: &str) -> DefinitionMetadata<BytecodeProgram> {
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
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot
        .check_program(root.unwrap(), &Default::default())
        .unwrap();
    let ir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
    lower_program_to_bytecode(&ir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap()
}

#[test]
fn source_and_artifact_cross_module_calls_match_interpreter_and_jit_fallback() {
    for overflow in [false, true] {
        let source = fixture(if overflow {
            "pub fn answer() -> i32 { 2147483647 + 1 }"
        } else {
            "pub fn answer() -> i32 { 40 + 2 }"
        });
        let decoded = round_trip_bytecode(source.clone());
        for program in [source, decoded] {
            for jit in [false, true] {
                let mut runtime = standard_runtime(RuntimeConfig {
                    ..Default::default()
                });
                let loaded = load_bytecode(&mut runtime, "root", program.clone()).unwrap();
                let _loaded_program = loaded;
                let loaded = runtime.select_program(&_loaded_program).unwrap();
                let vm = runtime;
                let report = if jit {
                    vm.execute_prepared_module(
                        &loaded,
                        ProgramEntry::Main,
                        &native_fixtures::unsupported(),
                        vm.execution_options(),
                    )
                } else {
                    vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
                };
                if overflow {
                    assert!(
                        matches!(report, Err(error) if error.kind() == RuntimeErrorKind::ScriptTrap)
                    );
                    continue;
                }
                let report = report.unwrap();
                assert_eq!(
                    report
                        .return_value
                        .value(vm.gc())
                        .expect("retained execution result"),
                    Value::I32(42)
                );
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
    let loaded = load_bytecode(
        &mut runtime,
        "root",
        fixture("pub fn answer() -> i32 { 40 + 2 }"),
    )
    .unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
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
    let mut vm = runtime;
    vm.attach_debug_session(session).unwrap();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .value(vm.gc())
            .expect("retained execution result"),
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
