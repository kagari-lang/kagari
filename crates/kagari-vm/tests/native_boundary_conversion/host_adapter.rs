use super::source_free_vm;
use kagari_runtime::{
    error::RuntimeErrorKind,
    host::{HostCallContext, HostError},
    native::{
        builder::ModuleBuilder, context::CallContext, declarations::FunctionDecl, types::Type,
    },
    session::{DeterministicInputs, ExecutionPhase, TraceValue},
    value::Value,
};
use kagari_types::{
    declaration::host::{HostCallPolicy, HostFunctionEffects, HostPassingStyle},
    scalar::BuiltinType,
};
use kagari_vm::error::VmError;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn host_policy_uses_native_entry_with_rooted_inputs_and_execution_session() {
    let catalog = kagari_stdlib::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let mut builder = ModuleBuilder::new("example::services", &catalog).unwrap();
    let echo = builder
        .define_function(
            FunctionDecl::new("echo")
                .parameter("text", Type::scalar(BuiltinType::String))
                .returns(Type::scalar(BuiltinType::String))
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::SharedBorrow],
                    effects: HostFunctionEffects {
                        may_allocate: true,
                        may_call_host_services: true,
                        ..Default::default()
                    },
                }),
        )
        .unwrap();
    assert!(
        builder
            .bind(echo.clone(), |cx: &mut CallContext<'_>| cx.argument(0))
            .is_err()
    );
    builder
        .bind(echo.clone(), |cx: &HostCallContext<'_>, args: &[Value]| {
            cx.runtime().collect_garbage().unwrap();
            let Value::Str(id) = args[0] else {
                panic!("string argument")
            };
            let text = cx.runtime().gc().string(id).unwrap().to_owned();
            let time = cx.execution_time_millis().unwrap();
            cx.runtime()
                .gc()
                .alloc_string(format!("{text}:{time}"))
                .map_err(|error| HostError::new(error.to_string()))
        })
        .unwrap();
    let entered = Arc::new(AtomicUsize::new(0));
    for name in ["configuration", "fail", "cancel", "panic"] {
        let function = builder
            .define_function(FunctionDecl::new(name).returns(Type::i32()).host_policy(
                HostCallPolicy {
                    passing: vec![],
                    effects: HostFunctionEffects {
                        may_trap: true,
                        may_read_immutable_configuration: name == "configuration",
                        may_call_host_services: name != "configuration",
                        ..Default::default()
                    },
                },
            ))
            .unwrap();
        let entered = entered.clone();
        builder
            .bind(function, move |cx: &HostCallContext<'_>, _: &[Value]| {
                entered.fetch_add(1, Ordering::Relaxed);
                match name {
                    "configuration" => Ok(Value::I32(7)),
                    "fail" => Err(HostError::new("service failed")),
                    "cancel" => {
                        cx.runtime().execution_options().cancellation.cancel();
                        Err(HostError::new("failure after cancellation"))
                    }
                    _ => panic!("service unwind"),
                }
            })
            .unwrap();
    }
    let missing_policy = builder
        .define_function(FunctionDecl::new("unclassified"))
        .unwrap();
    assert!(
        builder
            .bind(
                missing_policy.clone(),
                |_: &HostCallContext<'_>, _: &[Value]| Ok(Value::Unit)
            )
            .is_err()
    );
    // A rejected binding does not consume the declaration; complete it normally.
    builder
        .bind(missing_policy, |_: &mut CallContext<'_>| Ok(Value::Unit))
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, owner) = source_free_vm(
        r#"
        use example::services;
        pub fn run() -> String { services::echo("hello") }
        pub fn configuration() -> i32 { services::configuration() }
        pub fn fail() -> i32 { services::fail() }
        pub fn cancel() -> i32 { services::cancel() }
        pub fn unwind() -> i32 { services::panic() }
        "#,
        &module,
    );
    {
        let mut options = vm.runtime().execution_options();
        options.record_host_calls = true;
        options.inputs = DeterministicInputs {
            unix_time_millis: 123,
            random_seed: 9,
        };
        let session = vm.runtime().begin_execution(&owner, options).unwrap();
        let report = vm.execute(&owner, "run").unwrap();
        let Value::Str(id) = report.return_value.value(vm.runtime().gc()).unwrap() else {
            panic!("string result")
        };
        assert_eq!(&*vm.runtime().gc().string(id).unwrap(), "hello:123");
        assert_eq!(session.host_scope_count(), 0);
        let error = vm.execute(&owner, "fail").unwrap_err();
        assert!(
            matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::HostCallFailure && error.message() == "host call failed: service failed"),
            "{error:?}"
        );
        let trace = session.trace().unwrap();
        assert_eq!(trace.host_calls.len(), 2);
        assert!(
            trace.host_calls[0]
                .symbol
                .ends_with("example::services::echo")
        );
        assert_eq!(
            trace.host_calls[0].arguments,
            vec![TraceValue::Str {
                prefix: "hello".into(),
                truncated: false
            }]
        );
        assert_eq!(
            trace.host_calls[0].outcome,
            Some(Ok(TraceValue::Str {
                prefix: "hello:123".into(),
                truncated: false
            }))
        );
        assert_eq!(
            trace.host_calls[1].outcome,
            Some(Err(RuntimeErrorKind::HostCallFailure))
        );
        assert_eq!(session.host_scope_count(), 0);
    }
    {
        let mut options = vm.runtime().execution_options();
        options.phase = ExecutionPhase::CandidateInitialization;
        let session = vm.runtime().begin_execution(&owner, options).unwrap();
        let before = entered.load(Ordering::Relaxed);
        let report = vm.execute(&owner, "configuration").unwrap();
        assert_eq!(
            report.return_value.value(vm.runtime().gc()),
            Some(Value::I32(7))
        );
        let error = vm.execute(&owner, "fail").unwrap_err();
        assert!(
            matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::ExecutionPhaseViolation)
        );
        assert_eq!(entered.load(Ordering::Relaxed), before + 1);
        assert_eq!(session.host_scope_count(), 0);
    }
    {
        let session = vm
            .runtime()
            .begin_execution(&owner, vm.runtime().execution_options())
            .unwrap();
        let error = vm.execute(&owner, "cancel").unwrap_err();
        assert!(
            matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::Cancelled)
        );
        assert_eq!(session.host_scope_count(), 0);
    }
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        vm.execute(&owner, "unwind")
    }));
    assert!(unwind.is_err());
    assert!(!vm.runtime().is_quarantined());
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert!(vm.execute(&owner, "run").is_ok());
}
