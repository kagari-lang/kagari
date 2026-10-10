use super::*;
use crate::tests::common::standard_runtime;
use kagari_bytecode::program::{BytecodeProgram, ModuleRef};

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
        .execute(&loaded, "main")
        .expect("debugged function should execute");

    assert_eq!(
        report
            .return_value
            .value(vm.runtime().gc())
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
            .expect("watch should read live local"),
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
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
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
                dst: Register::new(0),
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
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![verified_module(vec![main], vec![ConstantOperand::I32(1)])],
            },
        )
        .expect("trap module should load");
    let mut session = DebugSession::new(&runtime).expect("debug session should be allowed");
    session.step_into().expect("step should be allowed");

    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session)
        .expect("debug attach should be allowed");
    let error = vm
        .execute(&loaded, "main")
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
