use kagari_runtime::{
    CapabilitySet, DebugVisibilityPolicy, LanguageProfile, Runtime, RuntimeConfig, SecurityContext,
    value::Value,
};

use crate::{
    DebugAdapterEvent, DebugAdapterRequest, DebugAdapterResponse, DebugProtocolAdapter, DebugWatch,
    SourceBreakpoint, Vm, tests::common::compile_test_bytecode,
};

fn debug_runtime(module_name: &str) -> Runtime {
    Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_debugger: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                debug_attach: true,
                debug_breakpoints: true,
                debug_pause: true,
                debug_stack_inspection: true,
                debug_value_inspection: true,
                debug_watch_evaluation: true,
                ..CapabilitySet::default()
            },
        },
        debug_visibility: DebugVisibilityPolicy {
            visible_modules: vec![module_name.to_owned()],
            ..DebugVisibilityPolicy::default()
        },
        ..RuntimeConfig::default()
    })
}

#[test]
fn debug_protocol_adapter_smoke_test_attaches_breaks_and_evaluates_watch() {
    let source = r#"
fn main() -> i32 {
    val value = 3;
    value + 4
}
"#;
    let mut runtime = debug_runtime("adapter.kgr");
    let loaded = runtime
        .load_module("adapter.kgr", compile_test_bytecode(source))
        .expect("module should load");
    let mut vm = Vm::new(runtime);
    let mut adapter = DebugProtocolAdapter::recording();

    let response = adapter
        .handle_request(&mut vm, DebugAdapterRequest::Attach)
        .expect("adapter should attach");
    assert!(matches!(response, DebugAdapterResponse::Attached { .. }));

    let breakpoint_response = adapter
        .handle_request(
            &mut vm,
            DebugAdapterRequest::SetBreakpoint(SourceBreakpoint::at_source_offset(
                "adapter.kgr",
                source
                    .find("value +")
                    .expect("source should contain tail expr"),
            )),
        )
        .expect("adapter should set breakpoint");
    let DebugAdapterResponse::BreakpointSet { breakpoint_id } = breakpoint_response else {
        panic!("expected breakpoint response");
    };

    let report = vm
        .execute(&loaded, "main")
        .expect("debugged module should execute");
    assert_eq!(report.return_value, Value::I32(7));
    let flushed = adapter
        .handle_request(&mut vm, DebugAdapterRequest::FlushEvents)
        .expect("adapter should flush debug events");
    let DebugAdapterResponse::EventsFlushed { emitted } = flushed else {
        panic!("expected flushed response");
    };
    assert!(emitted >= 2);

    let events = adapter.sink().events();
    assert!(events.iter().any(|event| matches!(
        event,
        DebugAdapterEvent::BreakpointResolved(resolved)
            if resolved.breakpoint_id == breakpoint_id
    )));
    let pause = events
        .iter()
        .find_map(|event| match event {
            DebugAdapterEvent::Paused(pause) => Some(pause),
            _ => None,
        })
        .expect("adapter should emit pause event");
    let frame = pause.top_frame().expect("pause should expose a frame");

    let watch = adapter
        .handle_request(
            &mut vm,
            DebugAdapterRequest::EvaluateWatch {
                pause_index: 0,
                frame_id: frame.id,
                watch: DebugWatch::Binding("value".to_owned()),
            },
        )
        .expect("adapter should evaluate watch");
    assert_eq!(
        watch,
        DebugAdapterResponse::WatchValue {
            value: Value::I32(3)
        }
    );
}

#[test]
fn debug_protocol_adapter_routes_step_and_run_to_cursor_requests() {
    let source = r#"
fn main() -> i32 {
    val first = 1;
    val second = first + 1;
    second
}
"#;
    let mut runtime = debug_runtime("adapter_step.kgr");
    let loaded = runtime
        .load_module("adapter_step.kgr", compile_test_bytecode(source))
        .expect("module should load");
    let mut vm = Vm::new(runtime);
    let mut adapter = DebugProtocolAdapter::recording();
    adapter
        .handle_request(&mut vm, DebugAdapterRequest::Attach)
        .expect("adapter should attach");

    assert!(matches!(
        adapter
            .handle_request(
                &mut vm,
                DebugAdapterRequest::RunToCursor {
                    source_uri: "adapter_step.kgr".to_owned(),
                    source_offset: source
                        .find("second")
                        .expect("source should contain cursor target"),
                },
            )
            .expect("run to cursor should configure breakpoint"),
        DebugAdapterResponse::BreakpointSet { .. }
    ));
    assert_eq!(
        adapter
            .handle_request(&mut vm, DebugAdapterRequest::StepInto)
            .expect("step into should configure"),
        DebugAdapterResponse::StepConfigured
    );

    vm.execute(&loaded, "main")
        .expect("debugged module should execute");
    let flushed = adapter
        .flush_events(&vm)
        .expect("adapter should flush step and breakpoint pauses");
    assert!(flushed >= 1);
    assert!(
        adapter
            .sink()
            .events()
            .iter()
            .any(|event| matches!(event, DebugAdapterEvent::Paused(_)))
    );
}
