use super::*;
use crate::{
    tests::{
        common::{compile_test_bytecode_with_options, round_trip_bytecode, standard_runtime},
        native_program::ProgramFixture,
    },
    vm::fixtures::FixtureExecution,
};
use kagari_bytecode::program::ModuleRef;
use kagari_compiler::source::lower::instances::MirLoweringOptions;
use kagari_contract::ids::FunctionRef;
use kagari_runtime::module::entry::ProgramEntry;

#[test]
fn optimized_breakpoints_preserve_branch_paths_and_binding_state() {
    let source = r#"
fn choose(flag: bool, x: i32) -> i32 {
    val y = x;
    if flag {
        y + 7
    } else {
        y
    }
}
pub fn taken() -> i32 { choose(true, 5) }
pub fn untaken() -> i32 { choose(false, 5) }
pub fn overflow() -> i32 { choose(true, 2147483647) }
"#;
    for optimized in [false, true] {
        let program = compile_test_bytecode_with_options(
            source,
            &MirLoweringOptions {
                optimization: optimized.then(Default::default),
                ..Default::default()
            },
        );
        for program in [program.clone(), round_trip_bytecode(program)] {
            for (entry, should_pause, expected) in [
                ("untaken", false, Some(5)),
                ("taken", true, Some(12)),
                ("overflow", true, None),
            ] {
                let mut runtime = debug_runtime("branch-debug.kgr");
                let loaded = runtime
                    .load_program("branch-debug.kgr", program.clone())
                    .unwrap();
                let _loaded_program = loaded;
                let loaded = runtime.select_program(&_loaded_program).unwrap();
                let mut session = DebugSession::new(&runtime).unwrap();
                let offset = source.find("y + 7").unwrap();
                let breakpoint = session
                    .add_breakpoint(SourceBreakpoint {
                        source_uri: "branch-debug.kgr".into(),
                        line: Some(
                            source[..offset]
                                .bytes()
                                .filter(|&byte| byte == b'\n')
                                .count() as u32
                                + 1,
                        ),
                        column: None,
                        source_offset: None,
                        temporary: false,
                    })
                    .unwrap();
                let mut vm = Vm::new(runtime);
                vm.attach_debug_session(session).unwrap();
                let result = vm.execute(&loaded, entry);
                if let Some(expected) = expected {
                    assert_eq!(
                        result
                            .unwrap()
                            .return_value
                            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
                            .ok(),
                        Some(Value::I32(expected))
                    );
                } else {
                    let error = result.unwrap_err();
                    let trace = error.trace().expect("overflow origin");
                    assert!(
                        trace.frames.iter().any(|frame| frame
                            .source_span
                            .is_some_and(|span| span.start == offset))
                    );
                }
                let debug = vm.debug_session().unwrap();
                let pauses = debug.pauses();
                let hits: Vec<_> = pauses
                    .iter()
                    .filter(|pause| pause.reason == DebugPauseReason::Breakpoint(breakpoint))
                    .collect();
                assert_eq!(
                    !hits.is_empty(),
                    should_pause,
                    "optimized={optimized}, entry={entry}"
                );
                for pause in hits {
                    let frame = pause.top_frame().unwrap();
                    assert_eq!(frame.function_name, "choose");
                    assert_eq!(
                        pause
                            .evaluate_watch(
                                vm.runtime(),
                                frame.id,
                                &DebugWatch::Binding("y".into())
                            )
                            .unwrap()
                            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
                            .unwrap(),
                        Value::I32(if expected.is_some() { 5 } else { i32::MAX })
                    );
                }
            }
        }
    }
}

#[test]
fn debug_session_resolves_breakpoints_and_inspects_live_locals() {
    let source = r#"
fn main() -> i32 {
    val text = "debug constant";
    val value = 3;
    value + 4
}
"#;
    let mut runtime = debug_runtime("debug.kgr");
    let loaded = runtime
        .load_program("debug.kgr", compile_test_bytecode(source))
        .expect("debug module should load");
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut session = DebugSession::new(&runtime).expect("debug session should be allowed");
    let constant_pc = loaded
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap()
        .instructions
        .iter()
        .position(|instruction| matches!(instruction, BytecodeInstruction::LoadConst { .. }))
        .unwrap();
    let before_text = session
        .add_breakpoint(SourceBreakpoint::at_source_offset(
            "debug.kgr",
            source.find("val text").unwrap(),
        ))
        .unwrap();
    let before_store = session
        .add_breakpoint(SourceBreakpoint::at_source_offset(
            "debug.kgr",
            source
                .find("val value")
                .expect("source should contain binding"),
        ))
        .expect("initialization breakpoint should be allowed");
    let breakpoint_id = session
        .add_breakpoint(SourceBreakpoint::at_source_offset(
            "debug.kgr",
            source
                .find("value +")
                .expect("source should contain tail expr"),
        ))
        .expect("breakpoint should be allowed");

    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session)
        .expect("debug attach should be allowed");
    let report = vm
        .execute(&loaded, ProgramEntry::Main)
        .expect("debugged function should execute");

    assert_eq!(
        report
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(7)
    );
    let debug = vm
        .debug_session()
        .expect("debug session should be attached");
    assert_eq!(
        debug
            .pauses()
            .iter()
            .filter(
                |pause| pause.reason == DebugPauseReason::Breakpoint(before_text)
                    && pause.top_frame().unwrap().instruction_offset == constant_pc
            )
            .count(),
        1,
        "cold materialization must consume one logical instruction"
    );
    let initial_pause = debug
        .pauses()
        .iter()
        .find(|pause| pause.reason == DebugPauseReason::Breakpoint(before_store))
        .expect("binding initializer should pause");
    assert!(
        initial_pause
            .top_frame()
            .unwrap()
            .bindings
            .iter()
            .all(|binding| binding.name != "value")
    );
    assert!(
        debug
            .resolved_breakpoints()
            .iter()
            .any(|breakpoint| breakpoint.breakpoint_id == breakpoint_id)
    );
    let pause = debug
        .pauses()
        .iter()
        .find(|pause| pause.reason == DebugPauseReason::Breakpoint(breakpoint_id))
        .expect("breakpoint should pause execution");
    let frame = pause.top_frame().expect("pause should expose a frame");
    assert_eq!(frame.function_name, "main");
    assert_eq!(
        pause
            .evaluate_watch(
                vm.runtime(),
                frame.id,
                &DebugWatch::Binding("value".to_owned()),
            )
            .expect("watch should read live local")
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .unwrap(),
        Value::I32(3)
    );
}

#[test]
fn debug_session_hides_inner_binding_after_lexical_scope() {
    let source = r#"
fn main() -> i32 {
    if true {
        val inner = 7;
        inner + 1;
    } else {
        0;
    };
    val after = 2;
    after
}
"#;
    let mut runtime = debug_runtime("lexical.kgr");
    let loaded = runtime
        .load_program("lexical.kgr", compile_test_bytecode(source))
        .unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut session = DebugSession::new(&runtime).unwrap();
    let inside = session
        .add_breakpoint(SourceBreakpoint::at_source_offset(
            "lexical.kgr",
            source.find("inner +").unwrap(),
        ))
        .unwrap();
    let after = session
        .add_breakpoint(SourceBreakpoint::at_source_offset(
            "lexical.kgr",
            source.rfind("after").unwrap(),
        ))
        .unwrap();
    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session).unwrap();
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(2)
    );
    let debug = vm.debug_session().unwrap();
    let pauses = debug.pauses();
    assert!(pauses.iter().any(|pause| {
        pause.reason == DebugPauseReason::Breakpoint(inside)
            && pause
                .top_frame()
                .unwrap()
                .bindings
                .iter()
                .any(|binding| binding.name == "inner")
    }));
    let after = pauses
        .iter()
        .find(|pause| pause.reason == DebugPauseReason::Breakpoint(after))
        .unwrap();
    assert!(
        after
            .top_frame()
            .unwrap()
            .bindings
            .iter()
            .all(|binding| binding.name != "inner")
    );
}

#[test]
fn debug_session_supports_step_into_and_trap_pause_events() {
    let mut runtime = debug_runtime("debug_trap.kbc");
    let mut main = test_function(
        0,
        "main",
        vec![
            BytecodeInstruction::LoadConst {
                dst: Slot::new(0),
                constant: ConstantId::new(0),
            },
            BytecodeInstruction::Unreachable,
        ],
        ValueType::I32,
        vec![ValueType::I32],
    );
    main.metadata.debug.source_spans = vec![
        InstructionSourceSpan {
            instruction_offset: 0,
            span: Span::new(1, 2),
        },
        InstructionSourceSpan {
            instruction_offset: 1,
            span: Span::new(3, 4),
        },
    ];
    main.metadata.debug.safe_debug_points = vec![
        SafeDebugPoint {
            id: DebugPointId::new(0),
            instruction_offset: 0,
            span: Span::new(1, 2),
            kind: SafeDebugPointKind::FunctionEntry,
        },
        SafeDebugPoint {
            id: DebugPointId::new(1),
            instruction_offset: 1,
            span: Span::new(3, 4),
            kind: SafeDebugPointKind::Trap,
        },
    ];
    let loaded = runtime
        .load_program(
            "debug_trap.kbc",
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![verified_module(vec![main], vec![ConstantOperand::I32(1)])],
            }
            .metadata(
                DefinitionContext::new().unwrap().snapshot(),
                &Default::default(),
            )
            .unwrap(),
        )
        .expect("trap module should load");
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut session = DebugSession::new(&runtime).expect("debug session should be allowed");
    session.step_into().expect("step should be allowed");

    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session)
        .expect("debug attach should be allowed");
    let error = vm
        .execute_fixture(&loaded, FunctionRef::new(0))
        .expect_err("unreachable should trap");

    assert!(matches!(error.cause(), VmError::Trap("unreachable")));
    let debug = vm
        .debug_session()
        .expect("debug session should be attached");
    let pauses = debug.pauses();
    assert!(
        pauses
            .iter()
            .any(|pause| pause.reason == DebugPauseReason::Step)
    );
    assert!(
        pauses
            .iter()
            .any(|pause| pause.reason == DebugPauseReason::Trap)
    );
}

#[test]
fn debugger_attachment_needs_no_permission_flags() {
    DebugSession::new(&standard_runtime(Default::default())).unwrap();
}
