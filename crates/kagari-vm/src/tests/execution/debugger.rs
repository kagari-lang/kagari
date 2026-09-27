use super::*;

#[test]
fn debug_session_resolves_breakpoints_and_inspects_live_locals() {
    let source = r#"
fn main() -> i32 {
    val value = 3;
    value + 4
}
"#;
    let mut runtime = debug_runtime("debug.kgr");
    let loaded = runtime
        .load_program(
            "debug.kgr",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![compile_test_bytecode(source)],
            },
        )
        .expect("debug module should load");
    let mut session = DebugSession::new(&runtime).expect("debug session should be allowed");
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

    assert_eq!(report.return_value, Value::I32(7));
    let debug = vm
        .debug_session()
        .expect("debug session should be attached");
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
        .load_program(
            "lexical.kgr",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![compile_test_bytecode(source)],
            },
        )
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
        vm.execute(&loaded, "main").unwrap().return_value,
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
                constant: ConstantOperand::I32(1),
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
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![verified_module(vec![main])],
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
fn debugger_attachment_requires_runtime_capability() {
    let runtime = Runtime::default();
    let error = DebugSession::new(&runtime).expect_err("default profile denies debugger attach");

    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("debug_attach")
    ));
}

#[test]
fn debugger_breakpoints_require_capability_and_visible_module() {
    let mut capabilities = debug_capabilities();
    capabilities.debug_breakpoints = false;
    let runtime_without_breakpoints = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_debugger: true,
                ..LanguageProfile::default()
            },
            capabilities,
        },
        debug_visibility: DebugVisibilityPolicy {
            visible_modules: vec!["debug.kgr".to_owned()],
            ..DebugVisibilityPolicy::default()
        },
        ..RuntimeConfig::default()
    });
    let mut session =
        DebugSession::new(&runtime_without_breakpoints).expect("attach should be allowed");
    let error = session
        .add_breakpoint(SourceBreakpoint::at_source_offset("debug.kgr", 0))
        .expect_err("breakpoints should require debugger capability");
    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("debug_breakpoints")
    ));

    let runtime_with_hidden_module = debug_runtime("visible.kgr");
    let mut session =
        DebugSession::new(&runtime_with_hidden_module).expect("attach should be allowed");
    let error = session
        .add_breakpoint(SourceBreakpoint::at_source_offset("hidden.kgr", 0))
        .expect_err("hidden modules should not accept breakpoints");
    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("debug module `hidden.kgr`")
    ));
}

#[test]
fn debugger_pause_control_is_separate_from_breakpoint_capability() {
    let mut capabilities = debug_capabilities();
    capabilities.debug_breakpoints = false;
    let mut runtime = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_debugger: true,
                ..LanguageProfile::default()
            },
            capabilities,
        },
        debug_visibility: DebugVisibilityPolicy {
            visible_modules: vec!["debug_step_only.kgr".to_owned()],
            ..DebugVisibilityPolicy::default()
        },
        ..RuntimeConfig::default()
    });
    let loaded = runtime
        .load_program(
            "debug_step_only.kgr",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![compile_test_bytecode(
                    "fn main() -> i32 { val value = 1; value }",
                )],
            },
        )
        .expect("debug module should load");
    let mut session = DebugSession::new(&runtime).expect("attach should be allowed");
    session
        .step_into()
        .expect("pause control should be allowed");

    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session)
        .expect("debug attach should be allowed");
    let report = vm
        .execute(&loaded, "main")
        .expect("step-only debugger should execute without breakpoint capability");

    assert_eq!(report.return_value, Value::I32(1));
    assert!(
        vm.debug_session()
            .expect("debug session should be attached")
            .pauses()
            .iter()
            .any(|pause| pause.reason == DebugPauseReason::Step)
    );
}

#[test]
fn debugger_watch_evaluation_requires_separate_capability() {
    let source = r#"
fn main() -> i32 {
    val value = 3;
    value + 4
}
"#;
    let mut capabilities = debug_capabilities();
    capabilities.debug_watch_evaluation = false;
    let mut runtime = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_debugger: true,
                ..LanguageProfile::default()
            },
            capabilities,
        },
        debug_visibility: DebugVisibilityPolicy {
            visible_modules: vec!["debug_watch.kgr".to_owned()],
            ..DebugVisibilityPolicy::default()
        },
        ..RuntimeConfig::default()
    });
    let loaded = runtime
        .load_program(
            "debug_watch.kgr",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![compile_test_bytecode(source)],
            },
        )
        .expect("debug module should load");
    let mut session = DebugSession::new(&runtime).expect("debug attach should be allowed");
    let breakpoint = session
        .add_breakpoint(SourceBreakpoint::at_source_offset(
            "debug_watch.kgr",
            source
                .find("value +")
                .expect("source should contain tail expr"),
        ))
        .expect("breakpoints should be allowed");
    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session)
        .expect("debug attach should be allowed");
    vm.execute(&loaded, "main")
        .expect("debugged function should execute");

    let pause = vm
        .debug_session()
        .expect("debug session should be attached")
        .pauses()
        .iter()
        .find(|pause| pause.reason == DebugPauseReason::Breakpoint(breakpoint))
        .expect("breakpoint should pause")
        .clone();
    let frame = pause.top_frame().expect("pause should expose a frame");
    let error = pause
        .evaluate_watch(
            vm.runtime(),
            frame.id,
            &DebugWatch::Binding("value".to_owned()),
        )
        .expect_err("watch evaluation should require its own capability");

    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("debug_watch_evaluation")
    ));
}
