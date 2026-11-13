use super::*;
use crate::error::{RuntimeErrorKind, reason::ErrorReason, trap::TrapReason};
use crate::module::entry::ProgramEntry;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    error::RuntimeError,
    execution::fixtures::FixtureExecution,
    frame::ExecutionFrame,
    interpreter_tests::common::{compile_test_bytecode_with_options, round_trip_bytecode},
    session::{ExecutionEvent, ExecutionObserver},
    test_support::native::program::ProgramFixture,
    value::{aggregates::tuples::TupleObject, scalar, strings::StringObject},
};
use kagari_bytecode::program::ModuleRef;
use kagari_compiler::source::lower::instances::MirLoweringOptions;
use kagari_contract::ids::FunctionRef;
use kagari_types::scalar::BuiltinType;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[test]
fn optimized_breakpoints_preserve_branch_paths_and_binding_state() {
    let source = include_str!("../../../tests/fixtures/debug/branch-bindings.kgr");
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
                let loaded =
                    load_bytecode(&mut runtime, "branch-debug.kgr", program.clone()).unwrap();
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
                let mut vm = runtime;
                vm.attach_debug_session(session).unwrap();
                let result = vm.execute_module(&loaded, entry, vm.execution_options());
                if let Some(expected) = expected {
                    assert_eq!(result.unwrap().return_value.as_i32(), Some(expected));
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
                let pauses = debug.pauses().to_vec();
                drop(debug);
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
                            .evaluate_watch(&mut vm, frame.id, &DebugWatch::Binding("y".into()))
                            .unwrap()
                            .as_i32(),
                        Some(if expected.is_some() { 5 } else { i32::MAX })
                    );
                }
            }
        }
    }
}

#[test]
fn debug_session_resolves_breakpoints_and_inspects_live_locals() {
    let source = include_str!("../../../tests/fixtures/debug/live-bindings.kgr");
    let mut runtime = debug_runtime("debug.kgr");
    let loaded = load_bytecode(&mut runtime, "debug.kgr", compile_test_bytecode(source))
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

    let mut vm = runtime;
    vm.attach_debug_session(session)
        .expect("debug attach should be allowed");
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect("debugged function should execute");

    assert_eq!(report.return_value.as_i32(), Some(7));
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
        .expect("breakpoint should pause execution")
        .clone();
    drop(debug);
    vm.clear_execution_observer().unwrap();
    vm.collect_garbage().unwrap();
    let roots = vm.gc_stats().root_registrations;
    let frame = pause.top_frame().expect("pause should expose a frame");
    assert_eq!(frame.function_name, "main");
    assert_eq!(
        pause
            .evaluate_watch(&mut vm, frame.id, &DebugWatch::Binding("value".to_owned()))
            .expect("watch should read live local")
            .as_i32(),
        Some(3)
    );
    let text = pause
        .evaluate_watch(&mut vm, frame.id, &DebugWatch::Binding("text".into()))
        .unwrap();
    let alias = pause
        .evaluate_watch(&mut vm, frame.id, &DebugWatch::Binding("alias".into()))
        .unwrap();
    assert_eq!(
        vm.gc_stats().root_registrations,
        roots,
        "watch/alias clones share the requested capture lease"
    );
    vm.memory
        .scope(&vm.resources, |cx, _| {
            let text = cx.downcast::<TupleObject>(text.get(cx)?.object().unwrap())?;
            let alias = cx.downcast::<TupleObject>(alias.get(cx)?.object().unwrap())?;
            assert!(text.ptr_eq(alias));
            let string = TupleObject::get(cx, text, 0)?;
            assert_eq!(
                StringObject::read(cx, string, str::to_owned)?,
                "debug constant"
            );
            assert_eq!(
                scalar::read_integer(BuiltinType::I32, TupleObject::get(cx, text, 1)?).unwrap(),
                5
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(
        Runtime::default()
            .validate_debug_value(&text)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ScriptTrap
    );
    let objects = vm.gc_stats().objects;
    drop(text);
    drop(alias);
    drop(pause);
    vm.collect_garbage().unwrap();
    assert_eq!(
        vm.gc_stats().objects + 1,
        objects,
        "detached diagnostic snapshot was the tuple's final owner"
    );
}

#[test]
fn debug_session_hides_inner_binding_after_lexical_scope() {
    let source = include_str!("../../../tests/fixtures/debug/lexical-bindings.kgr");
    let mut runtime = debug_runtime("lexical.kgr");
    let loaded = load_bytecode(&mut runtime, "lexical.kgr", compile_test_bytecode(source)).unwrap();
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
    let mut vm = runtime;
    vm.attach_debug_session(session).unwrap();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(2)
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
    let loaded = load_bytecode(
        &mut runtime,
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

    let mut vm = runtime;
    vm.attach_debug_session(session)
        .expect("debug attach should be allowed");
    let error = vm
        .execute_fixture(&loaded, FunctionRef::new(0))
        .expect_err("unreachable should trap");

    assert!(
        matches!(error.reason(), ErrorReason::Trap(TrapReason::Provider(reason)) if reason == "unreachable")
    );
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
fn requested_binding_capture_releases_temporary_roots_on_observer_unwind() {
    #[derive(Debug)]
    struct PanickingObserver(Arc<AtomicBool>);

    fn live_text(frames: &[ExecutionFrame]) -> Option<(&ExecutionFrame, Slot)> {
        let frame = frames.last()?;
        let pc = frame.instruction_offset();
        let slot = frame
            .function()?
            .metadata
            .debug
            .local_live_ranges
            .iter()
            .find(|range| range.name == "text" && range.start <= pc && pc < range.end)?
            .slot;
        Some((frame, slot))
    }

    impl ExecutionObserver for PanickingObserver {
        fn capture_binding_values(
            &mut self,
            _: &Runtime,
            _: ExecutionEvent,
            frames: &[ExecutionFrame],
        ) -> bool {
            live_text(frames).is_some()
        }

        fn observe(
            &mut self,
            runtime: &Runtime,
            _: ExecutionEvent,
            frames: &[ExecutionFrame],
        ) -> Result<(), RuntimeError> {
            if let Some((frame, slot)) = live_text(frames) {
                let value = runtime.observed_binding_value(frame, slot)?;
                drop(value);
                self.0.store(true, Ordering::SeqCst);
                panic!("observer failed after actual binding capture");
            }
            Ok(())
        }
    }

    let source = include_str!("../../../tests/fixtures/debug/live-bindings.kgr");
    let mut runtime = debug_runtime("capture-unwind.kgr");
    let installed = load_bytecode(
        &mut runtime,
        "capture-unwind.kgr",
        compile_test_bytecode(source),
    )
    .unwrap();
    let owner = runtime.select_program(&installed).unwrap();
    // Warm real constants/code before establishing the ownership baseline. The
    // second execution creates a fresh tuple; its only extra owner is the capture.
    assert_eq!(
        runtime
            .execute_module(&owner, ProgramEntry::Main, runtime.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(7)
    );
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let captured = Arc::new(AtomicBool::new(false));
    runtime
        .set_execution_observer(PanickingObserver(captured.clone()))
        .unwrap();
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        runtime.execute_module(&owner, ProgramEntry::Main, runtime.execution_options())
    }));
    assert!(outcome.is_err());
    assert!(
        captured.load(Ordering::SeqCst),
        "the intended requested-value callback must run"
    );
    assert!(runtime.execution_root().is_none());
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    runtime.clear_execution_observer().unwrap();
    runtime.collect_garbage().unwrap();
    let after = runtime.gc_stats();
    assert_eq!(
        (after.objects, after.root_registrations),
        (baseline.objects, baseline.root_registrations)
    );
    assert!(!runtime.is_quarantined());
}
